/* E02 native diagnostic: one role binary of the signal matrix.
 * Built static once per role (-DROLE_ID) and installed under E02_ROOT/bin/.
 * `role TARGET wait` enters TARGET through the directed transitions and waits;
 * `role TARGET check PID` enters TARGET and reports kill(0)/STOP/CONT results
 * against PID. Diagnostic only; the test seccomp filter allows every syscall. */
#define _GNU_SOURCE
#include <errno.h>
#include <linux/filter.h>
#include <linux/seccomp.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <unistd.h>

#ifndef E02_ROOT
#error "E02_ROOT must name the install directory"
#endif
#define ROOT E02_ROOT "/bin/"
#ifndef ROLE_ID
#define ROLE_ID 0
#endif
static volatile const int role_id = ROLE_ID;

static const char *name(const char *path) {
    const char *slash = strrchr(path, '/');
    return slash ? slash + 1 : path;
}

static void exec_role(const char *binary, int argc, char **argv) {
    char *next[6] = {0};
    for (int i = 0; i < argc && i < 5; i++) next[i] = argv[i];
    next[0] = (char *)binary;
    execv(binary, next);
    perror("execv");
    _exit(70);
}

int main(int argc, char **argv) {
    if ((argc != 4 && argc != 5) || strcmp(argv[1], "role") ||
        prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0)) return 40;
    struct sock_filter filter = BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW);
    struct sock_fprog program = { .len = 1, .filter = &filter };
    if (prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &program)) return 41;
    const char *self = name(argv[0]);
    const char *target = argv[2];
    if (strcmp(target, "supervisor") && strcmp(target, "app") &&
        strcmp(target, "bridge") && strcmp(target, "root_helper")) return 42;
    if (strcmp(argv[3], "wait") && strcmp(argv[3], "check")) return 43;
    if ((!strcmp(argv[3], "wait") && argc != 4) ||
        (!strcmp(argv[3], "check") && argc != 5)) return 44;
    if (!strcmp(self, "supervisor")) {
        if (!strcmp(target, "app") || !strcmp(target, "root_helper"))
            exec_role(ROOT "app", argc, argv);
        if (!strcmp(target, "bridge")) exec_role(ROOT "bridge", argc, argv);
    }
    if (!strcmp(self, "app") && !strcmp(target, "root_helper"))
        exec_role(ROOT "root_helper", argc, argv);
    if (strcmp(self, target)) return 45;
    char label[512] = {0};
    FILE *file = fopen("/proc/self/attr/current", "r");
    if (!file || !fgets(label, sizeof(label), file)) return 46;
    fclose(file);
    printf("ROLE name=%s id=%d pid=%ld label=%s", self, role_id, (long)getpid(), label);
    if (!strcmp(argv[3], "wait")) {
        printf("READY pid=%ld\n", (long)getpid());
        fflush(stdout);
        sleep(30);
        return 0;
    }
    char *end = NULL;
    long parsed = strtol(argv[4], &end, 10);
    if (!end || *end || parsed <= 1) return 47;
    pid_t victim = (pid_t)parsed;
    int zero = kill(victim, 0), zero_errno = zero ? errno : 0;
    int stop = kill(victim, SIGSTOP), stop_errno = stop ? errno : 0;
    int cont = kill(victim, SIGCONT), cont_errno = cont ? errno : 0;
    printf("CHECK self=%ld target=%ld zero=%d:%d stop=%d:%d cont=%d:%d\n",
           (long)getpid(), (long)victim, zero, zero_errno,
           stop, stop_errno, cont, cont_errno);
    fflush(stdout);
    return 0;
}
