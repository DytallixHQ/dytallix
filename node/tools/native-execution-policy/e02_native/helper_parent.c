/* E02 native diagnostic: synthetic supervisor and application owner around the
 * real Go root verifier. Built static twice (as E02_ROOT/bin/supervisor and
 * E02_ROOT/bin/app). Modes:
 *   good | bad    supervisor execs app; app launches the helper with the
 *                 DYTOWN01 owner frame (FD 7 seqpacket, FD 8 pidfd), sends GO
 *                 and a valid or changed request, reads the result and ACKs.
 *   direct        supervisor launches the helper itself; the helper's label is
 *                 incomplete and its guard must refuse before READY.
 *   owner-death   app waits for READY, prints the helper PID and exits without
 *                 GO; the helper must exit when its owner dies.
 * The helper SHA-512 is argv[2]. Diagnostic only. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <linux/filter.h>
#include <linux/seccomp.h>
#include <poll.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#ifndef E02_ROOT
#error "E02_ROOT must name the install directory"
#endif
#define BASE E02_ROOT "/"
#define ROOT BASE "bin/"
#ifndef ROLE_ID
#define ROLE_ID 0
#endif

static void put32(unsigned char *p, uint32_t v) {
    for (int i = 3; i >= 0; --i) { p[i] = (unsigned char)v; v >>= 8; }
}
static void put64(unsigned char *p, uint64_t v) {
    for (int i = 7; i >= 0; --i) { p[i] = (unsigned char)v; v >>= 8; }
}
static uint32_t get32(const unsigned char *p) {
    return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) |
           ((uint32_t)p[2] << 8) | p[3];
}
static int hex_digit(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    return -1;
}
static int decode_hash(const char *hex, unsigned char *out) {
    if (strlen(hex) != 128) return -1;
    for (int i = 0; i < 64; ++i) {
        int hi = hex_digit(hex[2*i]), lo = hex_digit(hex[2*i+1]);
        if (hi < 0 || lo < 0) return -1;
        out[i] = (unsigned char)((hi << 4) | lo);
    }
    return 0;
}
static unsigned char *read_file(const char *path, size_t limit, size_t *length) {
    struct stat st;
    if (stat(path, &st) || !S_ISREG(st.st_mode) || st.st_size <= 0 ||
        (uint64_t)st.st_size > limit) return NULL;
    FILE *file = fopen(path, "rb");
    if (!file) return NULL;
    unsigned char *raw = calloc((size_t)st.st_size + 1, 1);
    if (!raw) { fclose(file); return NULL; }
    *length = fread(raw, 1, (size_t)st.st_size, file);
    int okay = *length == (size_t)st.st_size && fgetc(file) == EOF;
    fclose(file);
    if (!okay) { free(raw); return NULL; }
    return raw;
}
static int write_all(int fd, const unsigned char *p, size_t n) {
    while (n) {
        ssize_t count = write(fd, p, n);
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) return -1;
        p += count; n -= (size_t)count;
    }
    return 0;
}
static int read_exact(int fd, unsigned char *p, size_t n) {
    while (n) {
        ssize_t count = read(fd, p, n);
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) return -1;
        p += count; n -= (size_t)count;
    }
    return 0;
}
static void label(const char *stage) {
    char value[512] = {0};
    FILE *file = fopen("/proc/self/attr/current", "r");
    if (file && fgets(value, sizeof(value), file))
        printf("stage=%s role_id=%d label=%s", stage, ROLE_ID, value);
    if (file) fclose(file);
    fflush(stdout);
}
static int launch(const char *mode, const char *hash) {
    const char *request_path = !strcmp(mode, "bad") ? BASE "bad-request.json" : BASE "request.json";
    size_t policy_len = 0, request_len = 0;
    unsigned char *policy = read_file(BASE "policy.json", 16384, &policy_len);
    unsigned char *request = read_file(request_path, 131072, &request_len);
    if (!policy || !request || policy_len == 0 || request_len > UINT32_MAX) return 51;
    int sockets[2], input[2], output[2];
    if (socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0, sockets) ||
        pipe2(input, O_CLOEXEC) || pipe2(output, O_CLOEXEC)) return 52;
    int pidfd = (int)syscall(SYS_pidfd_open, getpid(), 0);
    struct stat identity;
    struct timespec now;
    if (pidfd < 0 || fstat(pidfd, &identity) || clock_gettime(CLOCK_MONOTONIC, &now)) return 53;
    uint64_t deadline = (uint64_t)now.tv_sec * 1000000000ULL + (uint64_t)now.tv_nsec + 8000000000ULL;
    unsigned char frame[128] = {0};
    memcpy(frame, "DYTOWN01", 8);
    put32(frame + 8, 5);
    put32(frame + 12, (uint32_t)getpid());
    put32(frame + 16, (uint32_t)syscall(SYS_gettid));
    put32(frame + 20, (uint32_t)getuid());
    put64(frame + 24, deadline);
    put64(frame + 32, (uint64_t)identity.st_dev);
    put64(frame + 40, (uint64_t)identity.st_ino);
    if (decode_hash(hash, frame + 48) || send(sockets[0], frame, sizeof(frame), 0) != sizeof(frame)) return 54;
    pid_t child = fork();
    if (child < 0) return 55;
    if (child == 0) {
        close(sockets[0]); close(input[1]); close(output[0]);
        int control = fcntl(sockets[1], F_DUPFD_CLOEXEC, 12);
        int parent = fcntl(pidfd, F_DUPFD_CLOEXEC, 12);
        if (control < 0 || parent < 0 || dup2(input[0], 0) < 0 ||
            dup2(output[1], 1) < 0 || dup2(control, 7) < 0 || dup2(parent, 8) < 0)
            _exit(61);
        close(control); close(parent); close(sockets[1]); close(pidfd);
        if (input[0] != 0 && input[0] != 7 && input[0] != 8) close(input[0]);
        if (output[1] != 1 && output[1] != 7 && output[1] != 8) close(output[1]);
        execl(ROOT "root_helper", ROOT "root_helper",
              "--profile", "SLH-DSA-SHAKE-256s",
              "--execution-profile", "linux-immutable-observed-helper-v1",
              "--policy-json", (char *)policy,
              "--max-input-bytes", "131072", (char *)0);
        _exit(62);
    }
    close(sockets[1]); close(pidfd); close(input[0]); close(output[1]);
    struct pollfd ready_poll = { .fd = sockets[0], .events = POLLIN };
    unsigned char ready[128];
    int got_ready = poll(&ready_poll, 1, 3000) > 0 && (ready_poll.revents & POLLIN) &&
                    recv(sockets[0], ready, sizeof(ready), 0) == sizeof(ready) &&
                    !memcmp(ready, "DYTRDY01", 8);
    if (!strcmp(mode, "direct")) {
        close(sockets[0]); close(input[1]); close(output[0]);
        int status = 0;
        if (waitpid(child, &status, 0) != child) return 68;
        int exit_code = WIFEXITED(status) ? WEXITSTATUS(status) : -1;
        printf("direct_ready=%d helper_exit=%d\n", got_ready, exit_code);
        fflush(stdout);
        return !got_ready && exit_code != 0 ? 0 : 70;
    }
    if (!got_ready) return 57;
    printf("guard_ready=1 helper_pid=%ld\n", (long)child); fflush(stdout);
    if (!strcmp(mode, "owner-death")) _exit(0);
    if (send(sockets[0], "DYTGO001", 8, 0) != 8) return 58;
    static const unsigned char helper_ready[] = "DYTALLIX-ROOT-READY-v2\n";
    unsigned char helper_ready_raw[sizeof(helper_ready)-1];
    if (read_exact(output[0], helper_ready_raw, sizeof(helper_ready_raw)) ||
        memcmp(helper_ready_raw, helper_ready, sizeof(helper_ready_raw))) return 59;
    printf("protocol_ready=1\n"); fflush(stdout);
    unsigned char length[4]; put32(length, (uint32_t)request_len);
    if (write_all(input[1], length, sizeof(length)) ||
        write_all(input[1], request, request_len)) return 60;
    unsigned char header[5];
    if (read_exact(output[0], header, sizeof(header))) return 63;
    uint32_t result_len = get32(header + 1);
    if (result_len > 4096 || (header[0] == 0 && result_len == 0) ||
        (header[0] == 2 && result_len != 0) ||
        (header[0] != 0 && header[0] != 2)) return 64;
    unsigned char result[4096];
    if (read_exact(output[0], result, result_len)) return 65;
    printf("result_status=%u result_bytes=%u\n", header[0], result_len);
    fflush(stdout);
    static const unsigned char ack[] = "DYTALLIX-ROOT-ACK-v2\n";
    if (write_all(input[1], ack, sizeof(ack)-1)) return 66;
    close(input[1]);
    unsigned char trailing;
    if (read(output[0], &trailing, 1) != 0) return 67;
    close(output[0]); close(sockets[0]);
    int status = 0;
    if (waitpid(child, &status, 0) != child) return 68;
    int exit_code = WIFEXITED(status) ? WEXITSTATUS(status) : -1;
    printf("helper_exit=%d child_signal=%d\n", exit_code,
           WIFSIGNALED(status) ? WTERMSIG(status) : 0);
    fflush(stdout);
    free(policy); free(request);
    int bad = !strcmp(mode, "bad");
    return (header[0] == (bad ? 2 : 0) && exit_code == (bad ? 2 : 0)) ? 0 : 69;
}
int main(int argc, char **argv) {
    if (argc != 3 || (strcmp(argv[1], "good") && strcmp(argv[1], "bad") &&
                      strcmp(argv[1], "direct") && strcmp(argv[1], "owner-death"))) return 40;
    struct sock_filter filter = BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW);
    struct sock_fprog program = { .len = 1, .filter = &filter };
    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) ||
        prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &program)) return 41;
    const char *name = strrchr(argv[0], '/'); name = name ? name + 1 : argv[0];
    label(name);
    if (!strcmp(name, "supervisor")) {
        if (!strcmp(argv[1], "direct")) return launch(argv[1], argv[2]);
        execl(ROOT "app", ROOT "app", argv[1], argv[2], (char *)0);
        return 42;
    }
    if (!strcmp(name, "app")) return launch(argv[1], argv[2]);
    return 43;
}
