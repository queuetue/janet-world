# ── Stage 1: build ───────────────────────────────────────────────────────────
# Pure-Rust binary; no CGO / shared-lib linkage required.
FROM rust:1.93-slim-bookworm AS builder

RUN apt-get update && apt-get install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# ── 1a. Dependency-caching layer ─────────────────────────────────────────────
# Copy manifests and the janet-lib submodule (changes infrequently).
# This layer is rebuilt only when Cargo.toml, Cargo.lock, or janet-lib changes.
COPY Cargo.toml Cargo.lock ./
COPY janet-lib/ ./janet-lib/

# Create minimal stub sources so "cargo build" can compile all external deps
# even before the real source is present.
RUN mkdir -p src/bin && \
    printf 'pub mod protocol{}\npub mod types{}\n' > src/lib.rs && \
    printf 'fn main(){}\n' > src/bin/world.rs

# Pre-compile all dependencies (ignore errors from the stub binary itself).
RUN cargo build --release --bin janet-world-server 2>&1; exit 0

# ── 1b. Real build ────────────────────────────────────────────────────────────
# Copy true source; touch binary entry-point so Cargo sees it as changed.
COPY src/ ./src/
RUN touch src/lib.rs src/bin/world.rs
RUN cargo build --release --bin janet-world-server

# ── Stage 2: runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/janet-world-server /usr/local/bin/janet-world-server

# Run as a non-root user.
RUN useradd --uid 1000 --no-create-home --shell /usr/sbin/nologin janet-world
USER janet-world

ENTRYPOINT ["/usr/local/bin/janet-world-server"]
