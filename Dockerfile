# ── dev stage ────────────────────────────────────────────────────────────────
# Usage:  docker build --target dev -t dimini-sh:dev .
#         docker run --rm -it -v $(pwd):/app -p 3000:3000 dimini-sh:dev
FROM rust:1.93.1 AS dev

# Dev tooling: cargo-watch for live-reload, cargo-tarpaulin for coverage, sqlx-cli for migrations
RUN cargo install cargo-watch cargo-tarpaulin sqlx-cli --no-default-features --features rustls,postgres 2>/dev/null; true \
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

# Build sqlx-cli (no default features — only rustls + postgres to keep it lean)
RUN cargo install sqlx-cli --no-default-features --features rustls,postgres

COPY Cargo.toml Cargo.lock ./
# Pre-fetch deps
RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release && rm -rf src
COPY src ./src
RUN touch src/main.rs && cargo build --release

# ── production runtime ────────────────────────────────────────────────────────
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --shell /bin/bash app

# App binary
COPY --from=builder /app/target/release/dimini-sh /home/app/dimini-sh
# sqlx-cli for running migrations (make migrate-prod / entrypoint scripts)
COPY --from=builder /usr/local/cargo/bin/sqlx /usr/local/bin/sqlx
# Migration files so sqlx can find them at runtime
COPY --chown=app:app migrations /home/app/migrations
# Migration script
COPY --chown=app:app scripts/migrate-prod.sh /home/app/scripts/migrate-prod.sh

RUN chown app:app /home/app/dimini-sh && chmod 500 /home/app/dimini-sh \
    && chmod 500 /home/app/scripts/migrate-prod.sh

USER app
WORKDIR /home/app
EXPOSE 3000
CMD ["./dimini-sh"]
