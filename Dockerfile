# Stage 1: Build
FROM --platform=$BUILDPLATFORM rust:1.93-slim AS builder

ARG TARGETARCH

RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev && \
    rm -rf /var/lib/apt/lists/*

# Install cross-compilation toolchain for ARM64 when building on AMD64
RUN if [ "$TARGETARCH" = "arm64" ] && [ "$(dpkg --print-architecture)" != "arm64" ]; then \
        dpkg --add-architecture arm64 && \
        apt-get update && \
        apt-get install -y --no-install-recommends gcc-aarch64-linux-gnu libssl-dev:arm64 && \
        rm -rf /var/lib/apt/lists/* && \
        rustup target add aarch64-unknown-linux-gnu; \
    fi

ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc

WORKDIR /app

# Cache dependencies by building with dummy source first
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    if [ "$TARGETARCH" = "arm64" ] && [ "$(dpkg --print-architecture)" != "arm64" ]; then \
        PKG_CONFIG_SYSROOT_DIR=/usr/aarch64-linux-gnu \
        cargo build --release --target aarch64-unknown-linux-gnu; \
    else \
        cargo build --release; \
    fi && \
    rm -rf src

# Build actual source
COPY src/ src/
COPY migrations/ migrations/
RUN touch src/main.rs && \
    if [ "$TARGETARCH" = "arm64" ] && [ "$(dpkg --print-architecture)" != "arm64" ]; then \
        PKG_CONFIG_SYSROOT_DIR=/usr/aarch64-linux-gnu \
        cargo build --release --target aarch64-unknown-linux-gnu && \
        cp target/aarch64-unknown-linux-gnu/release/arena-server target/release/arena-server; \
    else \
        cargo build --release; \
    fi

# Stage 2: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

RUN groupadd --system arena && useradd --system --gid arena arena

COPY --from=builder /app/target/release/arena-server /usr/local/bin/arena-server

USER arena
WORKDIR /app

EXPOSE 3000

HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:3000/health/live || exit 1

CMD ["arena-server"]
