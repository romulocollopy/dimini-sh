use crate::domain::entities::url::Url;
use crate::repositories::url_repository::{RepositoryError, UrlRepositoryPort};
use crate::services::short_code::ShortCodeService;
use crate::utils::hash::sha256_hex;
use tracing::{info, warn, instrument};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum CreateShortCodeError {
    InvalidUrl(String),
    ShortCodeConflict,
    Repository(RepositoryError),
}

// ---------------------------------------------------------------------------
// Use case
// ---------------------------------------------------------------------------

pub struct CreateShortCodeUseCase<R: UrlRepositoryPort> {
    repo: R,
    short_code_service: ShortCodeService,
}

impl<R: UrlRepositoryPort> CreateShortCodeUseCase<R> {
    pub fn new(repo: R, short_code_service: ShortCodeService) -> Self {
        CreateShortCodeUseCase { repo, short_code_service }
    }

    #[instrument(skip(self, url_str, short_code), fields(caller_provided = short_code.is_some()))]
    pub async fn execute(
        &self,
        url_str: &str,
        short_code: Option<&str>,
    ) -> Result<String, CreateShortCodeError> {
        let url = Url::parse_strict(url_str)
            .map_err(|e| CreateShortCodeError::InvalidUrl(e.to_string()))?;

        let caller_provided = short_code.is_some();

        if !caller_provided {
            let hash = sha256_hex(&url.to_canonical());
            match self.repo.find_by_hash(&hash).await {
                Ok(Some(record)) if !record.caller_provided => {
                    info!(short_code = %record.short_code, caller_provided = false, "dedup hit: returning existing short code");
                    return Ok(record.short_code);
                }
                Ok(Some(_)) => {}
                Ok(None) => {}
                Err(e) => return Err(CreateShortCodeError::Repository(e)),
            }
        }

        let mut code = short_code
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.short_code_service.generate());

        let max_attempts = if caller_provided { 1 } else { 10 };

        for attempt in 0..max_attempts {
            match self.repo.find_by_short_code(&code).await {
                Ok(Some(record)) if record.canonical == url.to_canonical() => {
                    info!(short_code = %code, caller_provided, "idempotent: short code already maps to same URL");
                    return Ok(code);
                }
                Ok(Some(_)) => {
                    if caller_provided {
                        warn!(short_code = %code, "conflict: caller-provided short code already taken by different URL");
                        return Err(CreateShortCodeError::ShortCodeConflict);
                    }
                    if attempt + 1 == max_attempts {
                        warn!(attempts = max_attempts, "retry exhaustion: all generated short codes conflicted");
                        return Err(CreateShortCodeError::ShortCodeConflict);
                    }
                    code = self.short_code_service.generate();
                }
                Ok(None) => {
                    // proceed to save
                    break;
                }
                Err(e) => return Err(CreateShortCodeError::Repository(e)),
            }
        }

        self.repo
            .save_with_short_code(&url, &code, caller_provided)
            .await
            .map_err(CreateShortCodeError::Repository)?;

        info!(short_code = %code, caller_provided, "short code created successfully");
        Ok(code)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------



// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::url_repository::{MockUrlRepositoryPort, RepositoryError, UrlRecord};
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn make_record(url_str: &str, short_code: &str) -> UrlRecord {
        make_record_with_caller_provided(url_str, short_code, false)
    }

    fn make_record_with_caller_provided(
        url_str: &str,
        short_code: &str,
        caller_provided: bool,
    ) -> UrlRecord {
        let url = Url::parse(url_str).unwrap();
        UrlRecord {
            id: Uuid::new_v4(),
            canonical: url.to_canonical(),
            url_hash: "testhash".to_string(),
            parsed_url: serde_json::Value::Null,
            short_code: short_code.to_string(),
            caller_provided,
        }
    }

    fn use_case_with(
        repo: MockUrlRepositoryPort,
    ) -> CreateShortCodeUseCase<MockUrlRepositoryPort> {
        let svc = ShortCodeService::new(4);
        CreateShortCodeUseCase::new(repo, svc)
    }

    /// Build a mock whose `find_by_short_code` drains a pre-loaded queue of
    /// responses on successive calls. All other methods succeed silently.
    fn mock_with_find_responses(
        responses: Vec<Result<Option<UrlRecord>, RepositoryError>>,
    ) -> MockUrlRepositoryPort {
        let queue = Arc::new(Mutex::new(responses));
        let mut mock = MockUrlRepositoryPort::new();
        mock.expect_find_by_short_code().returning(move |_| {
            let result = {
                let mut q = queue.lock().unwrap();
                if q.is_empty() {
                    Ok(None)
                } else {
                    q.remove(0)
                }
            };
            Box::pin(async move { result })
        });
        mock.expect_find_by_hash()
            .returning(|_| Box::pin(async { Ok(None) }));
        mock.expect_save_with_short_code()
            .returning(|_, _, _| Box::pin(async { Ok(Uuid::new_v4()) }));
        mock
    }

    /// Build an always-empty mock (every method returns Ok(None) / Ok(uuid)).
    fn mock_empty() -> MockUrlRepositoryPort {
        mock_with_find_responses(vec![])
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn execute_with_valid_url_and_no_short_code_returns_ok() {
        let uc = use_case_with(mock_empty());
        let result = uc.execute("https://example.com/", None).await;
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let code = result.unwrap();
        assert!(!code.is_empty(), "returned short code must not be empty");
    }

    #[tokio::test]
    async fn execute_with_explicit_short_code_returns_that_code() {
        let uc = use_case_with(mock_empty());
        let result = uc.execute("https://example.com/", Some("mycode")).await;
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        assert_eq!(result.unwrap(), "mycode");
    }

    #[tokio::test]
    async fn execute_with_invalid_url_returns_invalid_url_error() {
        let uc = use_case_with(mock_empty());
        let result = uc.execute("not-a-valid-url!!!", Some("code")).await;
        assert!(
            matches!(result, Err(CreateShortCodeError::InvalidUrl(_))),
            "expected Err(InvalidUrl), got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn execute_calls_save_with_short_code_on_repo() {
        let uc = use_case_with(mock_empty());
        let result = uc.execute("https://example.com/save-check", Some("scode")).await;
        assert!(result.is_ok(), "expected Ok after save, got {:?}", result);
    }

    #[tokio::test]
    async fn execute_returns_conflict_when_short_code_used_by_different_url() {
        let existing = make_record("https://other.com/", "taken");
        let uc = use_case_with(mock_with_find_responses(vec![Ok(Some(existing))]));
        let result = uc.execute("https://example.com/", Some("taken")).await;
        assert!(
            matches!(result, Err(CreateShortCodeError::ShortCodeConflict)),
            "expected Err(ShortCodeConflict), got {:?}",
            result
        );
    }

    /// When the short code already maps to the same URL, execute must return Ok without
    /// calling save_with_short_code again — no duplicate rows should be written.
    #[tokio::test]
    async fn execute_is_idempotent_when_short_code_matches_same_url() {
        let url_str = "https://example.com/idempotent";
        let existing = make_record(url_str, "idem");
        // find_by_short_code returns existing → idempotent early return
        // save_with_short_code must NOT be called: expect it 0 times
        let queue = Arc::new(Mutex::new(vec![Ok(Some(existing))]));
        let mut mock = MockUrlRepositoryPort::new();
        mock.expect_find_by_short_code().returning(move |_| {
            let result = {
                let mut q = queue.lock().unwrap();
                if q.is_empty() { Ok(None) } else { q.remove(0) }
            };
            Box::pin(async move { result })
        });
        mock.expect_find_by_hash()
            .returning(|_| Box::pin(async { Ok(None) }));
        mock.expect_save_with_short_code().times(0);
        let uc = use_case_with(mock);

        let result = uc.execute(url_str, Some("idem")).await;

        assert!(result.is_ok(), "expected Ok for idempotent call, got {:?}", result);
        assert_eq!(result.unwrap(), "idem");
        // mockall verifies save_with_short_code was called 0 times on drop
    }

    /// When all 10 generated code attempts collide with different URLs, execute must
    /// return Err(ShortCodeConflict) rather than silently succeeding or panicking.
    ///
    /// Business rule: the use case guarantees uniqueness but will not loop forever.
    /// After 10 failed attempts it surfaces a conflict error to the caller.
    #[tokio::test]
    async fn execute_returns_conflict_after_exhausting_all_retries() {
        let responses: Vec<_> = (0..10)
            .map(|_| Ok(Some(make_record("https://other.com/", "taken"))))
            .collect();
        let uc = use_case_with(mock_with_find_responses(responses));
        let result = uc.execute("https://example.com/exhausted", None).await;
        assert!(
            matches!(result, Err(CreateShortCodeError::ShortCodeConflict)),
            "expected Err(ShortCodeConflict) after 10 retries, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn execute_retries_when_generated_code_is_taken() {
        let conflict = make_record("https://other.com/", "auto1");
        let uc = use_case_with(mock_with_find_responses(vec![Ok(Some(conflict)), Ok(None)]));
        let result = uc.execute("https://example.com/retry", None).await;
        assert!(result.is_ok(), "expected Ok after retry, got {:?}", result);
    }

    /// When `short_code` is None and the URL already exists in the database,
    /// `execute` must return the existing short code without calling
    /// `save_with_short_code`.
    ///
    /// Business rule: submitting the same URL twice with no explicit short_code
    /// must be idempotent — the original short code is returned and no duplicate
    /// row is written.
    #[tokio::test]
    async fn execute_returns_existing_short_code_when_url_already_in_db() {
        let existing = make_record("https://example.com/existing", "exist1");
        let existing_code = existing.short_code.clone();

        let mut mock = MockUrlRepositoryPort::new();
        mock.expect_find_by_hash()
            .returning(move |_| {
                let rec = existing.clone();
                Box::pin(async move { Ok(Some(rec)) })
            });
        // save must NOT be called
        mock.expect_save_with_short_code().times(0);
        // find_by_short_code not reached (early return after hash hit)
        mock.expect_find_by_short_code()
            .returning(|_| Box::pin(async { Ok(None) }));
        let uc = use_case_with(mock);

        let result = uc.execute("https://example.com/existing", None).await;

        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        assert_eq!(
            result.unwrap(),
            existing_code,
            "must return the existing short code, not a newly generated one"
        );
        // mockall verifies save_with_short_code was called 0 times on drop
    }

    /// When `short_code` is `Some`, `find_by_hash` must not be called at all.
    ///
    /// Business rule: hash-based deduplication only applies when no explicit
    /// short_code is requested. Explicit codes take a different path and must
    /// not trigger unnecessary database lookups.
    #[tokio::test]
    async fn execute_does_not_call_find_by_hash_when_short_code_provided() {
        let mut mock = MockUrlRepositoryPort::new();
        // find_by_hash must NOT be called
        mock.expect_find_by_hash().times(0);
        mock.expect_find_by_short_code()
            .returning(|_| Box::pin(async { Ok(None) }));
        mock.expect_save_with_short_code()
            .returning(|_, _, _| Box::pin(async { Ok(Uuid::new_v4()) }));
        let uc = use_case_with(mock);

        let result = uc.execute("https://example.com/", Some("explicit")).await;

        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        // mockall verifies find_by_hash was called 0 times on drop
    }

    /// When the caller supplies a short_code (`Some`), `save_with_short_code`
    /// must be called with `caller_provided = true`.
    ///
    /// Business rule: the `caller_provided` flag records the origin of the
    /// short code. A caller-supplied code must always be tagged `true` so the
    /// database accurately reflects how each short link was created.
    #[tokio::test]
    async fn execute_saves_with_caller_provided_true_when_short_code_given() {
        let saved_calls: Arc<Mutex<Vec<(String, String, bool)>>> =
            Arc::new(Mutex::new(vec![]));
        let saved_calls_clone = Arc::clone(&saved_calls);
        let mut mock = MockUrlRepositoryPort::new();
        mock.expect_find_by_hash()
            .returning(|_| Box::pin(async { Ok(None) }));
        mock.expect_find_by_short_code()
            .returning(|_| Box::pin(async { Ok(None) }));
        mock.expect_save_with_short_code()
            .times(1)
            .returning(move |url, sc, cp| {
                saved_calls_clone
                    .lock()
                    .unwrap()
                    .push((url.to_canonical(), sc.to_string(), cp));
                Box::pin(async { Ok(Uuid::new_v4()) })
            });
        let uc = use_case_with(mock);

        let result = uc.execute("https://example.com/cp-true", Some("mycode")).await;

        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let save_calls = saved_calls.lock().unwrap();
        assert_eq!(save_calls.len(), 1, "expected exactly one save call, got {:?}", *save_calls);
        let (_, _, caller_provided) = &save_calls[0];
        assert!(
            *caller_provided,
            "expected caller_provided == true when short_code is Some, got false"
        );
    }

    /// When `find_by_hash` returns a record where `caller_provided = false`,
    /// the existing auto-generated short code IS returned and no new row is saved.
    ///
    /// Business rule: an auto-generated short code is a safe, recyclable alias.
    /// Re-using it avoids link proliferation while keeping the URL space tidy.
    /// Only records that were NOT explicitly chosen by a caller qualify for reuse.
    #[tokio::test]
    async fn execute_returns_existing_short_code_when_hash_match_is_auto_generated() {
        let url_str = "https://example.com/auto-dedup";
        let existing = make_record_with_caller_provided(url_str, "auto1", false);
        let existing_code = existing.short_code.clone();

        let mut mock = MockUrlRepositoryPort::new();
        mock.expect_find_by_hash()
            .returning(move |_| {
                let rec = existing.clone();
                Box::pin(async move { Ok(Some(rec)) })
            });
        mock.expect_find_by_short_code()
            .returning(|_| Box::pin(async { Ok(None) }));
        mock.expect_save_with_short_code().times(0);
        let uc = use_case_with(mock);

        let result = uc.execute(url_str, None).await;

        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        assert_eq!(
            result.unwrap(),
            existing_code,
            "must return the existing auto-generated short code"
        );
        // mockall verifies save_with_short_code was called 0 times on drop
    }

    /// When `find_by_hash` returns a record where `caller_provided = true`,
    /// the use case must NOT reuse that short code. Instead it must generate a
    /// new random short code and save it with `caller_provided = false`.
    ///
    /// Business rule: a caller-provided (vanity) code is the caller's chosen
    /// alias and belongs to them. Silently handing it back to a different
    /// anonymous request would undermine the caller's intent and could expose
    /// private or branded links to unrelated traffic. Auto-generate a fresh
    /// code instead.
    #[tokio::test]
    async fn execute_generates_new_short_code_when_hash_match_is_caller_provided() {
        let url_str = "https://example.com/vanity-dedup";
        let vanity = make_record_with_caller_provided(url_str, "vanity1", true);
        let vanity_code = vanity.short_code.clone();

        let saved_calls: Arc<Mutex<Vec<(String, String, bool)>>> =
            Arc::new(Mutex::new(vec![]));
        let saved_calls_clone = Arc::clone(&saved_calls);

        let mut mock = MockUrlRepositoryPort::new();
        mock.expect_find_by_hash()
            .returning(move |_| {
                let rec = vanity.clone();
                Box::pin(async move { Ok(Some(rec)) })
            });
        mock.expect_find_by_short_code()
            .returning(|_| Box::pin(async { Ok(None) }));
        mock.expect_save_with_short_code()
            .times(1)
            .returning(move |url, sc, cp| {
                saved_calls_clone
                    .lock()
                    .unwrap()
                    .push((url.to_canonical(), sc.to_string(), cp));
                Box::pin(async { Ok(Uuid::new_v4()) })
            });
        let uc = use_case_with(mock);

        let result = uc.execute(url_str, None).await;

        assert!(
            result.is_ok(),
            "expected Ok after falling through to new code generation, got {:?}",
            result
        );
        let returned_code = result.unwrap();
        assert_ne!(
            returned_code,
            vanity_code,
            "must NOT return the caller-provided short code; got the vanity code back"
        );
        let save_calls = saved_calls.lock().unwrap();
        assert_eq!(
            save_calls.len(),
            1,
            "save_with_short_code must be called exactly once for the new code, got {:?}",
            *save_calls
        );
        let (_, saved_code, saved_caller_provided) = &save_calls[0];
        assert_eq!(
            saved_code, &returned_code,
            "the saved code must match the returned code"
        );
        assert!(
            !saved_caller_provided,
            "new auto-generated code must be saved with caller_provided = false"
        );
    }

    /// When no short_code is supplied (`None`), `save_with_short_code` must
    /// be called with `caller_provided = false`.
    ///
    /// Business rule: a generated short code was not chosen by the caller.
    /// The `caller_provided` flag must be `false` so analytics and auditing
    /// can distinguish vanity codes from auto-generated ones.
    #[tokio::test]
    async fn execute_saves_with_caller_provided_false_when_short_code_generated() {
        let saved_calls: Arc<Mutex<Vec<(String, String, bool)>>> =
            Arc::new(Mutex::new(vec![]));
        let saved_calls_clone = Arc::clone(&saved_calls);
        let mut mock = MockUrlRepositoryPort::new();
        mock.expect_find_by_hash()
            .returning(|_| Box::pin(async { Ok(None) }));
        mock.expect_find_by_short_code()
            .returning(|_| Box::pin(async { Ok(None) }));
        mock.expect_save_with_short_code()
            .times(1)
            .returning(move |url, sc, cp| {
                saved_calls_clone
                    .lock()
                    .unwrap()
                    .push((url.to_canonical(), sc.to_string(), cp));
                Box::pin(async { Ok(Uuid::new_v4()) })
            });
        let uc = use_case_with(mock);

        let result = uc.execute("https://example.com/cp-false", None).await;

        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let save_calls = saved_calls.lock().unwrap();
        assert_eq!(save_calls.len(), 1, "expected exactly one save call, got {:?}", *save_calls);
        let (_, _, caller_provided) = &save_calls[0];
        assert!(
            !*caller_provided,
            "expected caller_provided == false when short_code is None, got true"
        );
    }
}
