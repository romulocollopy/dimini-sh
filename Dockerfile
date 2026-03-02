# ── dev stage ────────────────────────────────────────────────────────────────
# Usage:  docker build --target dev -t dimini-sh:dev .
#         docker run --rm -it -v $(pwd):/app -p 3000:3000 dimini-sh:dev
FROM rust:1.93.1 AS dev

# Dev tooling: cargo-watch for live-reload, cargo-tarpaulin for coverage
RUN cargo install cargo-watch cargo-tarpaulin 2>/dev/null; true \
    && apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        git \
        lldb \
        postgresql-client \
    && rm -rf /var/lib/apt/lists/*

# Create non-root app user
RUN useradd --create-home --shell /bin/bash app

# All cargo activity must happen under the app user's home
# so the registry cache is owned by app from the start
ENV CARGO_HOME=/home/app/.cargo
ENV PATH=/home/app/.cargo/bin:$PATH

USER app
WORKDIR /home/app

# Pre-fetch dependencies so rebuilds inside the container are fast
COPY --chown=app:app Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main(){}" > src/main.rs \
    && cargo build && cargo test --no-run \
    && rm -rf src

EXPOSE 3000

# Default: live-reload on source changes.
# Override CMD to run tests, tarpaulin, etc.
#   cargo test
#   cargo tarpaulin --out Html
#   cargo build
CMD ["cargo", "watch", "-w", "src", "-w", "Cargo.toml", "-w", "Cargo.lock", "-x", "run"]

# ── production builder ────────────────────────────────────────────────────────
FROM rust:1.93.1 AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
# Pre-fetch deps
RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release && rm -rf src
COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --shell /bin/bash app
COPY --from=builder /app/target/release/dimini-sh /home/app/dimini-sh
RUN chown app:app /home/app/dimini-sh && chmod 500 /home/app/dimini-sh
USER app
WORKDIR /home/app
EXPOSE 3000
CMD ["./dimini-sh"]
