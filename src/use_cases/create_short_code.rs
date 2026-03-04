use crate::domain::entities::url::Url;
use crate::repositories::url_repository::{RepositoryError, UrlRepositoryPort};
use crate::services::short_code::ShortCodeService;
use crate::utils::hash::sha256_hex;

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
                Ok(Some(record)) => return Ok(record.short_code),
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
                    return Ok(code);
                }
                Ok(Some(_)) => {
                    if caller_provided {
                        return Err(CreateShortCodeError::ShortCodeConflict);
                    }
                    if attempt + 1 == max_attempts {
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
            .save_with_short_code(&url, &code)
            .await
            .map_err(CreateShortCodeError::Repository)?;

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
    use crate::repositories::url_repository::{RepositoryError, UrlRecord};
    use std::sync::Mutex;
    use uuid::Uuid;

    struct MockUrlRepository {
        find_responses: Mutex<Vec<Result<Option<UrlRecord>, RepositoryError>>>,
        find_calls: Mutex<Vec<String>>,
        save_calls: Mutex<Vec<(String, String)>>,
        save_error: Option<String>,
        find_by_hash_response: Option<Result<Option<UrlRecord>, RepositoryError>>,
        find_by_hash_call_count: Mutex<usize>,
    }

    impl MockUrlRepository {
        fn always_empty() -> Self {
            MockUrlRepository {
                find_responses: Mutex::new(vec![]),
                find_calls: Mutex::new(vec![]),
                save_calls: Mutex::new(vec![]),
                save_error: None,
                find_by_hash_response: None,
                find_by_hash_call_count: Mutex::new(0),
            }
        }

        fn with_find_responses(responses: Vec<Result<Option<UrlRecord>, RepositoryError>>) -> Self {
            MockUrlRepository {
                find_responses: Mutex::new(responses),
                find_calls: Mutex::new(vec![]),
                save_calls: Mutex::new(vec![]),
                save_error: None,
                find_by_hash_response: None,
                find_by_hash_call_count: Mutex::new(0),
            }
        }

        fn with_save_error(message: &str) -> Self {
            MockUrlRepository {
                find_responses: Mutex::new(vec![]),
                find_calls: Mutex::new(vec![]),
                save_calls: Mutex::new(vec![]),
                save_error: Some(message.to_string()),
                find_by_hash_response: None,
                find_by_hash_call_count: Mutex::new(0),
            }
        }

        fn with_find_by_hash_response(
            mut self,
            response: Result<Option<UrlRecord>, RepositoryError>,
        ) -> Self {
            self.find_by_hash_response = Some(response);
            self
        }
    }

    impl UrlRepositoryPort for MockUrlRepository {
        async fn find_by_short_code(
            &self,
            short_code: &str,
        ) -> Result<Option<UrlRecord>, RepositoryError> {
            self.find_calls.lock().unwrap().push(short_code.to_string());
            let mut responses = self.find_responses.lock().unwrap();
            if responses.is_empty() {
                Ok(None)
            } else {
                responses.remove(0)
            }
        }

        async fn find_by_hash(&self, _hash: &str) -> Result<Option<UrlRecord>, RepositoryError> {
            *self.find_by_hash_call_count.lock().unwrap() += 1;
            match &self.find_by_hash_response {
                Some(Ok(Some(record))) => Ok(Some(record.clone())),
                Some(Ok(None)) => Ok(None),
                Some(Err(e)) => Err(RepositoryError::Other(format!("{:?}", e))),
                None => Ok(None),
            }
        }

        async fn save_with_short_code(
            &self,
            url: &Url,
            short_code: &str,
        ) -> Result<Uuid, RepositoryError> {
            self.save_calls
                .lock()
                .unwrap()
                .push((url.to_canonical(), short_code.to_string()));
            if let Some(ref msg) = self.save_error {
                Err(RepositoryError::Other(msg.clone()))
            } else {
                Ok(Uuid::new_v4())
            }
        }
    }

    fn make_record(url_str: &str, short_code: &str) -> UrlRecord {
        let url = Url::parse(url_str).unwrap();
        UrlRecord {
            id: Uuid::new_v4(),
            canonical: url.to_canonical(),
            url_hash: "testhash".to_string(),
            parsed_url: serde_json::Value::Null,
            short_code: short_code.to_string(),
        }
    }

    fn use_case_with(repo: MockUrlRepository) -> CreateShortCodeUseCase<MockUrlRepository> {
        let svc = ShortCodeService::new(4);
        CreateShortCodeUseCase::new(repo, svc)
    }

    #[tokio::test]
    async fn execute_with_valid_url_and_no_short_code_returns_ok() {
        let repo = MockUrlRepository::always_empty();
        let uc = use_case_with(repo);
        let result = uc.execute("https://example.com/", None).await;
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let code = result.unwrap();
        assert!(!code.is_empty(), "returned short code must not be empty");
    }

    #[tokio::test]
    async fn execute_with_explicit_short_code_returns_that_code() {
        let repo = MockUrlRepository::always_empty();
        let uc = use_case_with(repo);
        let result = uc.execute("https://example.com/", Some("mycode")).await;
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        assert_eq!(result.unwrap(), "mycode");
    }

    #[tokio::test]
    async fn execute_with_invalid_url_returns_invalid_url_error() {
        let repo = MockUrlRepository::always_empty();
        let uc = use_case_with(repo);
        let result = uc.execute("not-a-valid-url!!!", Some("code")).await;
        assert!(
            matches!(result, Err(CreateShortCodeError::InvalidUrl(_))),
            "expected Err(InvalidUrl), got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn execute_calls_save_with_short_code_on_repo() {
        let svc = ShortCodeService::new(4);
        let inner_repo = MockUrlRepository::always_empty();
        let uc = CreateShortCodeUseCase::new(inner_repo, svc);
        let result = uc.execute("https://example.com/save-check", Some("scode")).await;
        assert!(result.is_ok(), "expected Ok after save, got {:?}", result);
    }

    #[tokio::test]
    async fn execute_returns_conflict_when_short_code_used_by_different_url() {
        let existing = make_record("https://other.com/", "taken");
        let repo = MockUrlRepository::with_find_responses(vec![Ok(Some(existing))]);
        let uc = use_case_with(repo);
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
        let repo = MockUrlRepository::with_find_responses(vec![Ok(Some(existing))]);
        let uc = use_case_with(repo);
        let result = uc.execute(url_str, Some("idem")).await;
        assert!(result.is_ok(), "expected Ok for idempotent call, got {:?}", result);
        assert_eq!(result.unwrap(), "idem");
        let save_calls = uc.repo.save_calls.lock().unwrap();
        assert!(
            save_calls.is_empty(),
            "save_with_short_code must NOT be called on idempotent path, got {:?}",
            *save_calls
        );
    }

    /// When all 10 generated code attempts collide with different URLs, execute must
    /// return Err(ShortCodeConflict) rather than silently succeeding or panicking.
    ///
    /// Business rule: the use case guarantees uniqueness but will not loop forever.
    /// After 10 failed attempts it surfaces a conflict error to the caller.
    #[tokio::test]
    async fn execute_returns_conflict_after_exhausting_all_retries() {
        // Feed 10 conflict responses (all different URL, same short_code scenario).
        let responses: Vec<_> = (0..10)
            .map(|_| Ok(Some(make_record("https://other.com/", "taken"))))
            .collect();
        let repo = MockUrlRepository::with_find_responses(responses);
        let uc = use_case_with(repo);
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
        let repo = MockUrlRepository::with_find_responses(vec![
            Ok(Some(conflict)),
            Ok(None),
        ]);
        let uc = use_case_with(repo);
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
        let repo = MockUrlRepository::always_empty()
            .with_find_by_hash_response(Ok(Some(existing)));
        let uc = use_case_with(repo);

        let result = uc.execute("https://example.com/existing", None).await;

        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        assert_eq!(
            result.unwrap(),
            existing_code,
            "must return the existing short code, not a newly generated one"
        );
        let save_calls = uc.repo.save_calls.lock().unwrap();
        assert!(
            save_calls.is_empty(),
            "save_with_short_code must NOT be called when URL already exists, got {:?}",
            *save_calls
        );
    }

    /// When `short_code` is `Some`, `find_by_hash` must not be called at all.
    ///
    /// Business rule: hash-based deduplication only applies when no explicit
    /// short_code is requested. Explicit codes take a different path and must
    /// not trigger unnecessary database lookups.
    #[tokio::test]
    async fn execute_does_not_call_find_by_hash_when_short_code_provided() {
        let repo = MockUrlRepository::always_empty();
        let uc = use_case_with(repo);

        let result = uc.execute("https://example.com/", Some("explicit")).await;

        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let call_count = *uc.repo.find_by_hash_call_count.lock().unwrap();
        assert_eq!(
            call_count, 0,
            "find_by_hash must NOT be called when short_code is Some, called {} time(s)",
            call_count
        );
    }
}
