# Lanes — multi-stage Docker build (D-22, PLAT-02)
#
# Stage 1: Builder (nightly-trixie — matches Leptos official Dockerfile)
#   Installs cargo-leptos, wasm32 target, binaryen, sass, clang, then
#   builds the release binary + WASM site assets via `cargo leptos build --release`.
#
# Stage 2: Runtime (debian:trixie-slim)
#   Copies only the binary and site assets; runs as non-root `lanes` user.
#   All secrets injected at runtime via ENV — nothing baked into the image (T-07-23).
#
# Usage:
#   docker compose up --build           # production (see compose.yml)
#   docker build -t lanes .             # standalone build

# ── Builder stage ─────────────────────────────────────────────────────────────
FROM rustlang/rust:nightly-trixie AS builder

# System deps for cargo-leptos: clang (for ring/openssl), sass, binaryen (wasm-opt)
RUN apt-get update -y \
    && apt-get install -y --no-install-recommends \
        clang \
        pkg-config \
        libssl-dev \
    && apt-get autoremove -y \
    && apt-get clean -y \
    && rm -rf /var/lib/apt/lists/*

# Install cargo-binstall for fast binary installs
RUN wget -q https://github.com/cargo-bins/cargo-binstall/releases/latest/download/cargo-binstall-x86_64-unknown-linux-musl.tgz \
    && tar -xvf cargo-binstall-x86_64-unknown-linux-musl.tgz \
    && cp cargo-binstall /usr/local/cargo/bin \
    && rm cargo-binstall-x86_64-unknown-linux-musl.tgz

# Install cargo-leptos (0.3.6 — compatible with Leptos 0.8, CLAUDE.md)
RUN cargo binstall cargo-leptos --version 0.3.6 -y

# WASM compilation target
RUN rustup target add wasm32-unknown-unknown

WORKDIR /app
COPY . .

# Build release: binary → target/release/lanes; site assets → target/site/
RUN cargo leptos build --release -vv

# ── Runtime stage ─────────────────────────────────────────────────────────────
FROM debian:trixie-slim AS runtime

WORKDIR /app

# Runtime deps: openssl (TLS), ca-certificates (HTTPS/S3 root CAs)
RUN apt-get update -y \
    && apt-get install -y --no-install-recommends \
        openssl \
        ca-certificates \
        curl \
    && apt-get autoremove -y \
    && apt-get clean -y \
    && rm -rf /var/lib/apt/lists/*

# Non-root user for container isolation (D-22, T-07-22)
RUN useradd -r -s /bin/false lanes

# Copy binary and static site assets from builder
COPY --from=builder /app/target/release/lanes /app/lanes
COPY --from=builder /app/target/site /app/site

# Runtime defaults — all overridable via ENV at compose/run time (D-24)
ENV RUST_LOG="info"
ENV LEPTOS_SITE_ADDR="0.0.0.0:3000"
ENV LEPTOS_SITE_ROOT="site"

EXPOSE 3000

USER lanes

CMD ["/app/lanes"]
