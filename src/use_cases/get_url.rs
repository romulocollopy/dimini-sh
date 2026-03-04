use crate::repositories::url_repository::{RepositoryError, UrlRecord};

// ---------------------------------------------------------------------------
// Re-export the port trait so tests can import it from `super`
// ---------------------------------------------------------------------------

pub use crate::repositories::url_repository::UrlRepositoryPort;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum GetUrlError {
    NotFound,
    Repository(RepositoryError),
}

// ---------------------------------------------------------------------------
// Use case
// ---------------------------------------------------------------------------

pub struct GetUrlUseCase<R: UrlRepositoryPort> {
    repo: R,
}

impl<R: UrlRepositoryPort> GetUrlUseCase<R> {
    pub fn new(repo: R) -> Self {
        GetUrlUseCase { repo }
    }

    pub async fn execute(&self, short_code: &str) -> Result<UrlRecord, GetUrlError> {
        match self.repo.find_by_short_code(short_code).await {
            Ok(Some(record)) => Ok(record),
            Ok(None) => Err(GetUrlError::NotFound),
            Err(e) => Err(GetUrlError::Repository(e)),
        }
    }

    pub fn repo(&self) -> &R {
        &self.repo
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{GetUrlError, GetUrlUseCase, UrlRepositoryPort};
    use crate::repositories::url_repository::{RepositoryError, UrlRecord};

    use uuid::Uuid;

    // -----------------------------------------------------------------------
    // Mock repository
    // -----------------------------------------------------------------------

    /// A test double for `UrlRepositoryPort`.
    ///
    /// Tracks the argument passed to `find_by_short_code` and returns a
    /// configurable response so each test can control the scenario.
    struct MockUrlRepository {
        /// If `Some`, the mock matches this short_code and returns `returned_record`.
        known_short_code: Option<String>,
        /// The record to return when the short_code matches.
        returned_record: Option<UrlRecord>,
        /// Records the last short_code argument received by `find_by_short_code`.
        last_called_with: std::sync::Mutex<Option<String>>,
    }

    impl MockUrlRepository {
        fn new_with_record(short_code: &str, record: UrlRecord) -> Self {
            MockUrlRepository {
                known_short_code: Some(short_code.to_string()),
                returned_record: Some(record),
                last_called_with: std::sync::Mutex::new(None),
            }
        }

        fn new_empty() -> Self {
            MockUrlRepository {
                known_short_code: None,
                returned_record: None,
                last_called_with: std::sync::Mutex::new(None),
            }
        }
    }

    impl UrlRepositoryPort for MockUrlRepository {
        async fn find_by_short_code(
            &self,
            short_code: &str,
        ) -> Result<Option<UrlRecord>, RepositoryError> {
            *self.last_called_with.lock().unwrap() = Some(short_code.to_string());
            if self.known_short_code.as_deref() == Some(short_code) {
                Ok(self.returned_record.as_ref().cloned())
            } else {
                Ok(None)
            }
        }

        async fn save_with_short_code(
            &self,
            _url: &crate::domain::entities::url::Url,
            _short_code: &str,
        ) -> Result<uuid::Uuid, RepositoryError> {
            Ok(uuid::Uuid::new_v4())
        }
    }

    /// Helper: build a minimal `UrlRecord` with the given short_code.
    ///
    /// `UrlRecord` will need a `short_code: String` field added by the implementer.
    fn make_url_record(short_code: &str) -> UrlRecord {
        UrlRecord {
            id: Uuid::new_v4(),
            canonical: "https://example.com/".to_string(),
            url_hash: "abc123".to_string(),
            parsed_url: serde_json::Value::Null,
            short_code: short_code.to_string(),
        }
    }

    // -----------------------------------------------------------------------
    // execute — known short_code
    // -----------------------------------------------------------------------

    /// A known short_code must cause `execute` to return `Ok(UrlRecord)`.
    ///
    /// Business rule: `GetUrlUseCase::execute` is the primary lookup entry
    /// point. When the short_code exists in the repository the use case must
    /// return the corresponding record so callers can redirect the user.
    #[tokio::test]
    async fn execute_with_known_short_code_returns_ok_url_record() {
        let short_code = "abc123";
        let expected = make_url_record(short_code);
        let repo = MockUrlRepository::new_with_record(short_code, make_url_record(short_code));
        let use_case = GetUrlUseCase::new(repo);

        let result = use_case.execute(short_code).await;

        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let record = result.unwrap();
        assert_eq!(record.short_code, expected.short_code);
        assert_eq!(record.canonical, expected.canonical);
    }

    // -----------------------------------------------------------------------
    // execute — repository receives the correct argument
    // -----------------------------------------------------------------------

    /// `execute` must call the repository with exactly the short_code it received.
    ///
    /// Business rule: the use case is a thin orchestration layer. It must
    /// pass the caller's short_code through to the repository unchanged.
    /// Mutating or ignoring the argument would produce silent bugs.
    #[tokio::test]
    async fn execute_calls_repository_with_the_provided_short_code() {
        let short_code = "xyz789";
        let repo = MockUrlRepository::new_with_record(short_code, make_url_record(short_code));
        let use_case = GetUrlUseCase::new(repo);

        let _ = use_case.execute(short_code).await;

        // Access the inner repo to verify argument tracking.
        // GetUrlUseCase must expose `repo` or we verify indirectly via the
        // use_case's internal borrow — adapter provides inner field access.
        assert_eq!(
            use_case.repo().last_called_with.lock().unwrap().as_deref(),
            Some(short_code),
            "repository was not called with the short_code passed to execute"
        );
    }

    // -----------------------------------------------------------------------
    // execute — unknown short_code
    // -----------------------------------------------------------------------

    /// An unknown short_code must cause `execute` to return `Err(GetUrlError::NotFound)`.
    ///
    /// Business rule: when no URL is registered for a given short_code the
    /// use case must signal `NotFound` so the HTTP layer can respond with 404.
    /// Returning `Ok(None)` or panicking are both incorrect behaviours.
    #[tokio::test]
    async fn execute_with_unknown_short_code_returns_not_found_error() {
        let repo = MockUrlRepository::new_empty();
        let use_case = GetUrlUseCase::new(repo);

        let result = use_case.execute("does_not_exist").await;

        assert!(
            matches!(result, Err(GetUrlError::NotFound)),
            "expected Err(GetUrlError::NotFound), got {:?}",
            result
        );
    }
}
