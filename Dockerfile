FROM rust:1.93.1 AS builder
WORKDIR /app

RUN cargo install sqlx-cli --no-default-features --features rustls,postgres

RUN useradd --create-home --shell /bin/bash app
WORKDIR /home/app

COPY Cargo.toml Cargo.lock ./

RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release && rm -rf src
COPY src ./src
COPY public ./public
RUN touch src/main.rs && cargo build --release

EXPOSE 3000


FROM builder AS dev

# System packages
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        git \
        lldb \
        postgresql-client \
    && rm -rf /var/lib/apt/lists/*

RUN cargo install cargo-watch \
    && cargo install cargo-tarpaulin \
    && cargo install sqlx-cli --no-default-features --features rustls,postgres

# Default: live-reload on source changes.
# Override CMD to run tests, tarpaulin, etc.:
#   cargo test
#   cargo tarpaulin --out Html
#   cargo build
#
#
COPY . .
CMD ["cargo", "watch", "-w", "src", "-w", "Cargo.toml", "-w", "Cargo.lock", "--", "cargo", "run"]

# ── production runtime ────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS prod
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --shell /bin/bash app

WORKDIR /home/app

# App binary
COPY --from=builder /home/app/target/release/dimini-sh /home/app/dimini-sh
# sqlx-cli for running migrations (make migrate-prod / entrypoint scripts)
COPY --from=builder /usr/local/cargo/bin/sqlx /usr/local/bin/sqlx
# Migration files so sqlx can find them at runtime
COPY --chown=app:app . .

RUN rm -rf src docs

RUN chown app:app -R /home/app/
USER app
CMD ["./dimini-sh"]
