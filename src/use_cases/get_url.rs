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
    use super::{GetUrlError, GetUrlUseCase};
    use crate::repositories::url_repository::{MockUrlRepositoryPort, UrlRecord};

    use uuid::Uuid;

    /// Helper: build a minimal `UrlRecord` with the given short_code.
    fn make_url_record(short_code: &str) -> UrlRecord {
        UrlRecord {
            id: Uuid::new_v4(),
            canonical: "https://example.com/".to_string(),
            url_hash: "abc123".to_string(),
            parsed_url: serde_json::Value::Null,
            short_code: short_code.to_string(),
            caller_provided: false,
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
        let mut repo = MockUrlRepositoryPort::new();
        repo.expect_find_by_short_code()
            .returning(|code| {
                let record = make_url_record(code);
                Box::pin(async move { Ok(Some(record)) })
            });
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
        let mut repo = MockUrlRepositoryPort::new();
        repo.expect_find_by_short_code()
            .with(mockall::predicate::eq(short_code))
            .times(1)
            .returning(|code| {
                let record = make_url_record(code);
                Box::pin(async move { Ok(Some(record)) })
            });
        let use_case = GetUrlUseCase::new(repo);

        let _ = use_case.execute(short_code).await;
        // Verification that repository was called with the correct short_code
        // is enforced by mockall on drop (with(eq(short_code)) + times(1)).
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
        let mut repo = MockUrlRepositoryPort::new();
        repo.expect_find_by_short_code()
            .returning(|_| Box::pin(async { Ok(None) }));
        let use_case = GetUrlUseCase::new(repo);

        let result = use_case.execute("does_not_exist").await;

        assert!(
            matches!(result, Err(GetUrlError::NotFound)),
            "expected Err(GetUrlError::NotFound), got {:?}",
            result
        );
    }
}
