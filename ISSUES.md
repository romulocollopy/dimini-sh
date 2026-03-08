# Issues

Tracked issues from TDD review cycles.

---

## [OPEN] Missing SAFETY annotations on ClonableMock methods — v0.1.1

**Severity:** SHOULD
**Source:** reviewer feedback
**Description:** `src/webapp/mod.rs:154–170` — `find_by_hash` and `save_with_short_code` in the `ClonableMock` impl use the same `MutexGuard`-across-`impl Future` pattern as `find_by_short_code`, but only the latter carries a `SAFETY` comment explaining why this is sound.
**Suggested fix:** Copy or cross-reference the existing `SAFETY` comment above `find_by_hash` and `save_with_short_code`.

---

## [OPEN] Duplicate sha256_hex helper in test module — v0.1.1

**Severity:** CONSIDER
**Source:** reviewer feedback
**Description:** `src/repositories/url_repository.rs:243–248` defines a local `sha256_hex` helper in the test module that duplicates the already-available `crate::utils::hash::sha256_hex` utility.
**Suggested fix:** Remove the local helper and import `crate::utils::hash::sha256_hex` in the test module instead.
