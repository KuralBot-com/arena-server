# Stage 1: Build
FROM rust:1.93-slim AS builder

RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Cache dependencies by building with dummy source first
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

# Build actual source
COPY src/ src/
COPY migrations/ migrations/
RUN touch src/main.rs && cargo build --release

# Stage 2: Runtime
FROM debian:trixie-slim

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
