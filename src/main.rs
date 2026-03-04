pub mod domain;
pub mod repositories;
pub mod services;
pub mod settings;
pub mod use_cases;

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};
use std::sync::Arc;
use use_cases::get_url::{GetUrlError, GetUrlUseCase, UrlRepositoryPort};

async fn root() -> &'static str {
    "Welcome to dimini.sh"
}

/// Stub: redirect a short_code to its canonical URL.
/// Implementation intentionally omitted — tests drive the green phase.
async fn redirect_short_code<R: UrlRepositoryPort + Send + Sync + 'static>(
    State(use_case): State<Arc<GetUrlUseCase<R>>>,
    Path(short_code): Path<String>,
) -> axum::response::Response {
    match use_case.execute(&short_code).await {
        Ok(record) => (StatusCode::FOUND, [(header::LOCATION, record.canonical)]).into_response(),
        Err(GetUrlError::NotFound) => StatusCode::NOT_FOUND.into_response(),
        Err(GetUrlError::Repository(_)) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Build the application router.
///
/// Accepts an `Arc<GetUrlUseCase<R>>` so that tests can inject a mock-backed
/// use case while `main()` provides the real Postgres-backed one.
pub fn app<R>(use_case: Arc<GetUrlUseCase<R>>) -> Router
where
    R: UrlRepositoryPort + Send + Sync + 'static,
{
    Router::new()
        .route("/", get(root))
        .route("/:short_code", get(redirect_short_code::<R>))
        .with_state(use_case)
}

#[tokio::main]
async fn main() {
    use repositories::url_repository::UrlRepository;
    use settings::Settings;

    let settings = Settings::load();
    let pool = sqlx::PgPool::connect(settings.get_database_url())
        .await
        .expect("failed to connect to database");
    let repo = UrlRepository::new(pool);
    let use_case = Arc::new(GetUrlUseCase::new(repo));
    let router = app(use_case);
    let listener = tokio::net::TcpListener::bind(settings.get_host())
        .await
        .expect("failed to bind");
    axum::serve(listener, router).await.expect("server error");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::url_repository::{RepositoryError, UrlRecord, UrlRepositoryPort};
    use axum_test::TestServer;
    use uuid::Uuid;

    // -----------------------------------------------------------------------
    // Mock repository
    // -----------------------------------------------------------------------

    /// Test double for `UrlRepositoryPort`.
    ///
    /// Returns a hardcoded `UrlRecord` for one known short_code; returns
    /// `Ok(None)` for everything else.
    #[derive(Clone)]
    struct MockUrlRepository {
        known_short_code: String,
        record: UrlRecord,
    }

    impl MockUrlRepository {
        fn new(short_code: &str, canonical: &str) -> Self {
            MockUrlRepository {
                known_short_code: short_code.to_string(),
                record: UrlRecord {
                    id: Uuid::new_v4(),
                    canonical: canonical.to_string(),
                    url_hash: "mockhash".to_string(),
                    short_code: short_code.to_string(),
                    parsed_url: serde_json::Value::Null,
                },
            }
        }
    }

    impl UrlRepositoryPort for MockUrlRepository {
        async fn find_by_short_code(
            &self,
            short_code: &str,
        ) -> Result<Option<UrlRecord>, RepositoryError> {
            if short_code == self.known_short_code {
                Ok(Some(self.record.clone()))
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

    /// Build a `TestServer` with a mock-backed use case.
    fn test_server(repo: MockUrlRepository) -> TestServer {
        let use_case = Arc::new(GetUrlUseCase::new(repo));
        TestServer::new(app(use_case)).unwrap()
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    /// GET / must return HTTP 200.
    ///
    /// Business rule: the landing page is publicly accessible and always
    /// returns a successful response. This is the entry point for all users.
    #[tokio::test]
    async fn get_root_returns_200() {
        let repo = MockUrlRepository::new("irrelevant", "https://example.com/");
        let server = test_server(repo);
        let response = server.get("/").await;
        response.assert_status_ok();
    }

    /// GET /:short_code for a known short_code must return HTTP 302 with the
    /// canonical URL in the `Location` header.
    ///
    /// Business rule: the primary function of this service is URL redirection.
    /// A client following a short link must be sent to the canonical destination
    /// via a 302 Found response so that it always follows the latest target.
    #[tokio::test]
    async fn get_short_code_returns_302_with_location_header() {
        let canonical = "https://example.com/destination";
        let repo = MockUrlRepository::new("abc123", canonical);
        let server = test_server(repo);

        let response = server.get("/abc123").await;

        response.assert_status(axum::http::StatusCode::FOUND);
        assert_eq!(
            response.headers().get("Location").and_then(|v| v.to_str().ok()),
            Some(canonical),
            "Location header must be the canonical URL"
        );
    }

    /// GET /:short_code for an unknown short_code must return HTTP 404.
    ///
    /// Business rule: if a short_code has no corresponding URL record the
    /// client must receive a 404 Not Found so it can surface a meaningful
    /// error page rather than silently failing.
    #[tokio::test]
    async fn get_short_code_returns_404_for_unknown_short_code() {
        let repo = MockUrlRepository::new("known", "https://example.com/");
        let server = test_server(repo);

        let response = server.get("/no-such-code").await;

        response.assert_status(axum::http::StatusCode::NOT_FOUND);
    }
}
