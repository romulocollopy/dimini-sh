### Issues

[SHOULD] create_short_code.rs — No logging on key operations. Add tracing::info!/warn!  
 at: successful creation (short_code + caller_provided), dedup hit (returning existing  
 code), retry exhaustion, and caller-provided conflict. Without these, production  
 diagnosis is blind.

[SHOULD] url_repository.rs — Repository errors should log at error! level before  
 mapping to RepositoryError. sqlx errors swallowed into a generic variant lose context —
at minimum log the raw error with the operation name before converting.

[CONSIDER] create_short_code.rs — The ShortCodeConflict error variant carries no  
 context. Including the conflicting short_code would give the handler richer messaging  
 for the 409 response.

[CONSIDER] url.rs — to_canonical has no doc comment. The seven normalisation rules only
live in the spec; they should be documented there for future maintainers.

[CONSIDER] create_short_code.rs — The 10-attempt retry count is hardcoded. Worth making
it a config value alongside the short_code length — easier to tune without a recompile.

[SHOULD] src/main.rs — Consequence of the above: document RUST_LOG=info as the default  
 somewhere (README or a comment) so operators know how to control log verbosity.

[CONSIDER] #[instrument] on execute skips url_str, so the root span has no URL context  
 at all. If URLs aren't sensitive here, adding fields(url = %url_str, caller_provided =  
 ...) makes the span self-describing in traces.

[CONSIDER] Minor asymmetry in the repository: find_by_short_code is implemented  
 directly on the trait impl while find_by_hash and save_with_short_code delegate. Not a  
 logging issue — just a note for the next refactor pass.

[SHOULD] main.rs — ClonableMock comment missing  
 The MutexGuard is released before the boxed future is awaited, which is safe because  
 mockall's .returning() closures always produce immediately-ready futures. This is  
 non-obvious and load-bearing — if any expectation ever returned a genuinely async  
 future capturing the guard, it would deadlock or fail to compile with Send. The code is
correct; it just needs a short comment explaining the invariant.

[CONSIDER] create_short_code.rs — mock_with_find_responses allows unbounded  
 find_by_hash calls  
 The helper sets up find_by_hash with no .times() constraint, so tests that go through  
 the short_code = Some(…) path don't catch spurious hash lookups. This was the same gap  
 in the old code — not a regression — but adding .times(0) to the caller-provided tests  
 would make the "must not call find_by_hash" assertion explicit and free.
