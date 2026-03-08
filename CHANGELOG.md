# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-03-08

First stable release of dimini.sh — a self-hosted URL shortener built on Rust, Axum, SQLx, and PostgreSQL.

### Added
- **URL shortening** — `POST /create/` accepts a URL and an optional caller-supplied short code; returns the short code.
- **Redirect** — `GET /:short_code` resolves a short code and issues a `302` redirect to the canonical URL.
- **Info endpoint** — `GET /:short_code/about` returns JSON with full URL metadata (canonical, url_hash, short_code, caller_provided).
- **Frontend** — static UI served from `public/` via `GET /`.
- **Deduplication** — submitting the same URL twice (no explicit short code) returns the existing auto-generated code; no duplicate rows are written.
- **Caller-provided short codes** — callers may supply a custom short code; a `caller_provided` flag is persisted and used to prevent reuse of vanity codes by anonymous requests.
- **Short code retry** — up to 10 random short codes are attempted before surfacing a conflict error.
- **Schema constraints** — `UNIQUE` constraint on `urls.short_code`; index on `urls.url_hash` for efficient dedup lookups.
- **Structured logging** — JSON log output via `tracing`/`tracing-subscriber`; log level configurable via `RUST_LOG` or `settings.yaml`.
- **Request logging middleware** — all inbound HTTP requests are logged with method, path, status, and latency.
- **Docker / Traefik** — production `compose.yaml` with Traefik labels; dev `compose.dev.yaml`.
- **SQLx migrations** — managed schema via `sqlx migrate`; applied automatically on startup.

### Architecture
- Layered: domain entities → repository port (trait) → use cases → Axum handlers.
- External dependencies (SQLx) wrapped behind `UrlRepositoryPort`; use cases depend only on the trait.
- Mock-based unit tests (mockall + axum-test) for all handler and use-case behaviour.
- Integration tests for repository layer against a real PostgreSQL test database.

### Known Issues
- `src/webapp/mod.rs` — `find_by_hash` and `save_with_short_code` in `ClonableMock` use the same `MutexGuard` pattern as `find_by_short_code` but carry no `SAFETY` annotation. Comment should be copied or cross-referenced above those two methods.
- `src/repositories/url_repository.rs` — Test module defines a local `sha256_hex` helper that duplicates `crate::utils::hash::sha256_hex`. Should use the existing utility instead.
- `src/use_cases/create_short_code.rs` — The 10-attempt retry count is hardcoded. Worth making it a config value alongside the short code length.
- `src/use_cases/create_short_code.rs` — `ShortCodeConflict` error variant carries no context (conflicting code not included in the 409 response).
- `src/main.rs` — `RUST_LOG=info` default should be documented in the README or a comment for operators.

## [0.1.1] - 2026-03-08

### Added
- Migration `20260307000000_add_short_code_unique_and_url_hash_index.sql`: adds a UNIQUE constraint on `urls.short_code` and a hash index on `urls.url_hash` to harden schema integrity and improve lookup performance.

### Changed
- Added `SAFETY` comment in `src/webapp/mod.rs` inside the `ClonableMock` impl above `find_by_short_code`, documenting why holding a `MutexGuard` across the `impl Future` return is sound.

### Known Issues
- `src/webapp/mod.rs:154–170` — `find_by_hash` and `save_with_short_code` in the same `ClonableMock` impl use the identical `MutexGuard` pattern as `find_by_short_code` but carry no `SAFETY` annotation. Comment should be copied or cross-referenced above those methods.
- `src/repositories/url_repository.rs:243–248` — Test module defines a local `sha256_hex` helper that duplicates `crate::utils::hash::sha256_hex`. Should use the existing utility instead.
