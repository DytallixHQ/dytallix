# The pinned Dytallix release builder (E06; P01, 5 October 2026).
#
# Every input is pinned: the Rust image by digest, the Debian packages to a
# snapshot.debian.org date, and Go by its published SHA-256. Two builds of a
# release in this image, on any x86_64 or emulated x86_64 Docker host, give
# the same bytes. The image itself need not be byte-identical; its toolchain
# versions are recorded in each build record.
FROM --platform=linux/amd64 rust:1.88.0-slim-bookworm@sha256:a6cab604fa016ac022e78c24038497eb7617ab59150ca4c3dd2ede0fbd514d4b

# The Debian archive as of the base image's own build (debian.sh @1753056000).
ARG DEBIAN_SNAPSHOT=20250721T000000Z
ARG GO_VERSION=1.25.14
ARG GO_SHA256=a21ae5633a269bcd7e90cf767e48225633795e99d831742cbf3397064fee7712

RUN set -eux; \
    rm -f /etc/apt/sources.list.d/debian.sources; \
    printf '%s\n' \
      "deb [check-valid-until=no] https://snapshot.debian.org/archive/debian/${DEBIAN_SNAPSHOT} bookworm main" \
      "deb [check-valid-until=no] https://snapshot.debian.org/archive/debian/${DEBIAN_SNAPSHOT} bookworm-updates main" \
      "deb [check-valid-until=no] https://snapshot.debian.org/archive/debian-security/${DEBIAN_SNAPSHOT} bookworm-security main" \
      > /etc/apt/sources.list; \
    apt-get -o Acquire::Retries=5 update; \
    apt-get install -y --no-install-recommends \
      binutils clang curl g++ libclang-dev make pkg-config python3; \
    rm -rf /var/lib/apt/lists/*

RUN set -eux; \
    curl -fsSLo /tmp/go.tgz "https://go.dev/dl/go${GO_VERSION}.linux-amd64.tar.gz"; \
    echo "${GO_SHA256}  /tmp/go.tgz" | sha256sum -c -; \
    tar -C /usr/local -xzf /tmp/go.tgz; \
    rm /tmp/go.tgz

# Fixed paths and settings, so every build sees the same environment. The
# installed toolchain overrides each workspace's rust-toolchain.toml (whose
# rustfmt and clippy components a release build does not need); the driver
# checks that the two name the same version.
ENV PATH=/usr/local/go/bin:/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    GOTOOLCHAIN=local \
    GOMODCACHE=/go/pkg/mod \
    GOFLAGS=-mod=readonly \
    CGO_ENABLED=0 \
    CARGO_INCREMENTAL=0 \
    RUSTUP_TOOLCHAIN=1.88.0-x86_64-unknown-linux-gnu \
    SOURCE_DATE_EPOCH=0 \
    TZ=UTC \
    LC_ALL=C.UTF-8
