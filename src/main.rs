pub mod domain;
pub mod repositories;
pub mod services;
pub mod settings;
pub mod use_cases;
pub mod utils;

use axum::{
    extract::{Json, Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing_subscriber::EnvFilter;
use use_cases::create_short_code::{CreateShortCodeError, CreateShortCodeUseCase};
use use_cases::get_url::{GetUrlError, GetUrlUseCase, UrlRepositoryPort};

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct AppState<R: UrlRepositoryPort + Clone + Send + Sync + 'static> {
    pub get_url: Arc<GetUrlUseCase<R>>,
    pub create_short_code: Arc<CreateShortCodeUseCase<R>>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn root() -> &'static str {
    "Welcome to dimini.sh"
}

/// Stub: redirect a short_code to its canonical URL.
/// Implementation intentionally omitted — tests drive the green phase.
async fn redirect_short_code<R: UrlRepositoryPort + Clone + Send + Sync + 'static>(
    State(state): State<AppState<R>>,
    Path(short_code): Path<String>,
) -> axum::response::Response {
    match state.get_url.execute(&short_code).await {
        Ok(record) => (StatusCode::FOUND, [(header::LOCATION, record.canonical)]).into_response(),
        Err(GetUrlError::NotFound) => StatusCode::NOT_FOUND.into_response(),
        Err(GetUrlError::Repository(_)) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Stub: return URL record details as JSON for a given short_code.
/// Implementation intentionally omitted — tests drive the green phase.
async fn about_short_code<R: UrlRepositoryPort + Clone + Send + Sync + 'static>(
    State(state): State<AppState<R>>,
    Path(short_code): Path<String>,
) -> axum::response::Response {
    match state.get_url.execute(&short_code).await {
        Ok(record) => (StatusCode::OK, Json(record)).into_response(),
        Err(GetUrlError::NotFound) => StatusCode::NOT_FOUND.into_response(),
        Err(GetUrlError::Repository(_)) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
struct CreateShortCodeRequest {
    url: String,
    short_code: Option<String>,
}

#[derive(Serialize)]
struct CreateShortCodeResponse {
    short_code: String,
}

async fn create_short_code<R: UrlRepositoryPort + Clone + Send + Sync + 'static>(
    State(state): State<AppState<R>>,
    Json(body): Json<CreateShortCodeRequest>,
) -> axum::response::Response {
    match state
        .create_short_code
        .execute(&body.url, body.short_code.as_deref())
        .await
    {
        Ok(code) => (
            StatusCode::CREATED,
            Json(CreateShortCodeResponse { short_code: code }),
        )
            .into_response(),
        Err(CreateShortCodeError::InvalidUrl(_)) => {
            StatusCode::UNPROCESSABLE_ENTITY.into_response()
        }
        Err(CreateShortCodeError::ShortCodeConflict) => StatusCode::CONFLICT.into_response(),
        Err(CreateShortCodeError::Repository(_)) => {
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Build the application router.
pub fn app<R>(state: AppState<R>) -> Router
where
    R: UrlRepositoryPort + Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/", get(root))
        .route("/", post(create_short_code::<R>))
        .route("/{short_code}", get(redirect_short_code::<R>))
        .route("/{short_code}/about", get(about_short_code::<R>))
        .with_state(state)
}

#[tokio::main]
async fn main() {
    // Log level is controlled via RUST_LOG (default: info).
    // Example: RUST_LOG=debug cargo run
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(true)
        .with_current_span(true)
        .with_span_list(true)
        .init();

    use repositories::url_repository::UrlRepository;
    use services::short_code::ShortCodeService;
    use settings::Settings;

    let settings = Settings::load();
    let pool = sqlx::PgPool::connect(settings.get_database_url())
        .await
        .expect("failed to connect to database");
    let state = AppState {
        get_url: Arc::new(GetUrlUseCase::new(UrlRepository::new(pool.clone()))),
        create_short_code: Arc::new(CreateShortCodeUseCase::new(
            UrlRepository::new(pool),
            ShortCodeService::new(settings.get_short_code_length()),
        )),
    };
    let router = app(state);
    let listener = tokio::net::TcpListener::bind(settings.get_host())
        .await
        .expect("failed to bind");
    axum::serve(listener, router).await.expect("server error");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::url_repository::{MockUrlRepositoryPort, UrlRecord};
    use crate::services::short_code::ShortCodeService;
    use crate::use_cases::create_short_code::CreateShortCodeUseCase;
    use axum_test::TestServer;
    use serde_json::{json, Value};
    use uuid::Uuid;

    // -----------------------------------------------------------------------
    // Mock helpers
    // -----------------------------------------------------------------------

    /// Newtype wrapper that makes `MockUrlRepositoryPort` satisfy the
    /// `R: Clone` bound on `AppState<R>` by sharing the mock behind `Arc`.
    /// Cloning the wrapper shares the same underlying mock.
    #[derive(Clone)]
    struct ClonableMock(Arc<std::sync::Mutex<MockUrlRepositoryPort>>);

    impl ClonableMock {
        fn new(mock: MockUrlRepositoryPort) -> Self {
            ClonableMock(Arc::new(std::sync::Mutex::new(mock)))
        }
    }

    impl UrlRepositoryPort for ClonableMock {
        fn find_by_short_code(
            &self,
            short_code: &str,
        ) -> impl std::future::Future<
            Output = Result<Option<UrlRecord>, crate::repositories::url_repository::RepositoryError>,
        > + Send {
            // Acquire lock only long enough to produce the pinned future;
            // the MutexGuard is released before the future is awaited.
            self.0.lock().unwrap().find_by_short_code(short_code)
        }

        fn find_by_hash(
            &self,
            hash: &str,
        ) -> impl std::future::Future<
            Output = Result<Option<UrlRecord>, crate::repositories::url_repository::RepositoryError>,
        > + Send {
            self.0.lock().unwrap().find_by_hash(hash)
        }

        fn save_with_short_code(
            &self,
            url: &crate::domain::entities::url::Url,
            short_code: &str,
            caller_provided: bool,
        ) -> impl std::future::Future<
            Output = Result<uuid::Uuid, crate::repositories::url_repository::RepositoryError>,
        > + Send {
            self.0
                .lock()
                .unwrap()
                .save_with_short_code(url, short_code, caller_provided)
        }
    }

    /// Build a `ClonableMock` that returns `Some(record)` for one known
    /// short_code and `Ok(None)` for everything else.
    fn make_repo(known_short_code: &str, canonical: &str) -> ClonableMock {
        let sc = known_short_code.to_string();
        let record = UrlRecord {
            id: Uuid::new_v4(),
            canonical: canonical.to_string(),
            url_hash: "mockhash".to_string(),
            short_code: known_short_code.to_string(),
            parsed_url: serde_json::Value::Null,
            caller_provided: false,
        };
        let mut mock = MockUrlRepositoryPort::new();
        mock.expect_find_by_short_code()
            .returning(move |code| {
                let result = if code == sc {
                    Ok(Some(record.clone()))
                } else {
                    Ok(None)
                };
                Box::pin(async move { result })
            });
        mock.expect_find_by_hash()
            .returning(|_| Box::pin(async { Ok(None) }));
        mock.expect_save_with_short_code()
            .returning(|_, _, _| Box::pin(async { Ok(Uuid::new_v4()) }));
        ClonableMock::new(mock)
    }

    /// Build an `AppState` and `TestServer` with two mock-backed use cases.
    ///
    /// Both repos must be configured with matching behaviour because the
    /// same request may pass through either use case.
    fn test_server(get_url_repo: ClonableMock, create_repo: ClonableMock) -> TestServer {
        let state = AppState {
            get_url: Arc::new(GetUrlUseCase::new(get_url_repo)),
            create_short_code: Arc::new(CreateShortCodeUseCase::new(
                create_repo,
                ShortCodeService::new(4),
            )),
        };
        TestServer::new(app(state))
    }

    // -----------------------------------------------------------------------
    // GET / tests
    // -----------------------------------------------------------------------

    /// GET / must return HTTP 200.
    ///
    /// Business rule: the landing page is publicly accessible and always
    /// returns a successful response. This is the entry point for all users.
    #[tokio::test]
    async fn get_root_returns_200() {
        let server = test_server(
            make_repo("irrelevant", "https://example.com/"),
            make_repo("irrelevant", "https://example.com/"),
        );
        let response = server.get("/").await;
        response.assert_status_ok();
    }

    // -----------------------------------------------------------------------
    // GET /:short_code tests
    // -----------------------------------------------------------------------

    /// GET /:short_code for a known short_code must return HTTP 302 with the
    /// canonical URL in the `Location` header.
    ///
    /// Business rule: the primary function of this service is URL redirection.
    /// A client following a short link must be sent to the canonical destination
    /// via a 302 Found response so that it always follows the latest target.
    #[tokio::test]
    async fn get_short_code_returns_302_with_location_header() {
        let canonical = "https://example.com/destination";
        let server = test_server(
            make_repo("abc123", canonical),
            make_repo("abc123", canonical),
        );

        let response = server.get("/abc123").await;

        response.assert_status(axum::http::StatusCode::FOUND);
        assert_eq!(
            response
                .headers()
                .get("Location")
                .and_then(|v| v.to_str().ok()),
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
        let server = test_server(
            make_repo("known", "https://example.com/"),
            make_repo("known", "https://example.com/"),
        );

        let response = server.get("/no-such-code").await;

        response.assert_status(axum::http::StatusCode::NOT_FOUND);
    }

    // -----------------------------------------------------------------------
    // POST / tests
    // -----------------------------------------------------------------------

    /// POST / with a valid URL and no short_code must return HTTP 201 with a
    /// JSON body containing a `short_code` key.
    ///
    /// Business rule: clients that do not care which short code is assigned
    /// must receive a freshly generated one. The response must be 201 Created
    /// and the body must expose the assigned `short_code` so the client can
    /// share it.
    #[tokio::test]
    async fn post_url_returns_201_with_short_code() {
        // "noslot" will never be generated; all generated codes hit Ok(None).
        let server = test_server(
            make_repo("noslot", "https://other.com/"),
            make_repo("noslot", "https://other.com/"),
        );

        let response = server
            .post("/")
            .json(&json!({ "url": "https://example.com/" }))
            .await;

        response.assert_status(axum::http::StatusCode::CREATED);
        let body: Value = response.json();
        assert!(
            body.get("short_code").is_some(),
            "response body must contain a `short_code` key, got: {body}"
        );
    }

    /// POST / with a valid URL and an explicit short_code must return HTTP 201
    /// with the exact short_code echoed in the response body.
    ///
    /// Business rule: clients may supply a preferred vanity short_code. When
    /// that code is available the service must honour it and confirm it in the
    /// response, so the client can construct the final short URL deterministically.
    #[tokio::test]
    async fn post_url_with_explicit_short_code_returns_201_with_that_code() {
        // "taken" is the known code for a different URL; "mycode" is free (Ok(None)).
        let server = test_server(
            make_repo("taken", "https://other.com/"),
            make_repo("taken", "https://other.com/"),
        );

        let response = server
            .post("/")
            .json(&json!({ "url": "https://example.com/", "short_code": "mycode" }))
            .await;

        response.assert_status(axum::http::StatusCode::CREATED);
        let body: Value = response.json();
        assert_eq!(
            body.get("short_code").and_then(|v| v.as_str()),
            Some("mycode"),
            "response body must echo the supplied short_code, got: {body}"
        );
    }

    /// POST / with an invalid URL must return HTTP 422 Unprocessable Entity.
    ///
    /// Business rule: the service must reject malformed URLs before attempting
    /// to store them. A 422 tells the client the request was understood but the
    /// payload is semantically invalid, which is distinct from a syntax error (400).
    #[tokio::test]
    async fn post_url_with_invalid_url_returns_422() {
        let server = test_server(
            make_repo("irrelevant", "https://example.com/"),
            make_repo("irrelevant", "https://example.com/"),
        );

        let response = server
            .post("/")
            .json(&json!({ "url": "not-a-valid-url!!!" }))
            .await;

        response.assert_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    }

    /// POST / with a short_code that is already taken by a different URL must
    /// return HTTP 409 Conflict.
    ///
    /// Business rule: when a client requests a specific short_code that already
    /// maps to a different URL the service must refuse with 409 Conflict so the
    /// client knows to pick a different vanity code rather than assume success.
    #[tokio::test]
    async fn post_url_with_conflicting_short_code_returns_409() {
        // "taken" maps to "https://other.com/" — a different canonical than the
        // one being submitted, so CreateShortCodeUseCase returns ShortCodeConflict.
        let server = test_server(
            make_repo("taken", "https://other.com/"),
            make_repo("taken", "https://other.com/"),
        );

        let response = server
            .post("/")
            .json(&json!({ "url": "https://example.com/", "short_code": "taken" }))
            .await;

        response.assert_status(axum::http::StatusCode::CONFLICT);
    }

    // -----------------------------------------------------------------------
    // GET /:short_code/about tests
    // -----------------------------------------------------------------------

    /// GET /:short_code/about for a known short_code must return HTTP 200 with
    /// a JSON body containing the full URL record fields.
    ///
    /// Business rule: clients need metadata about a short link (canonical URL,
    /// hash, parsed URL structure) without being redirected. The /about endpoint
    /// exposes all UrlRecord fields as JSON so clients can inspect link details.
    #[tokio::test]
    async fn get_about_returns_200_with_url_details() {
        let canonical = "https://example.com/destination";
        let server = test_server(
            make_repo("abc1", canonical),
            make_repo("abc1", canonical),
        );

        let response = server.get("/abc1/about").await;

        response.assert_status(axum::http::StatusCode::OK);
        let body: Value = response.json();
        assert_eq!(
            body.get("canonical").and_then(|v| v.as_str()),
            Some(canonical),
            "response body must contain the canonical URL, got: {body}"
        );
        assert_eq!(
            body.get("short_code").and_then(|v| v.as_str()),
            Some("abc1"),
            "response body must contain the short_code, got: {body}"
        );
    }

    /// GET /:short_code/about for an unknown short_code must return HTTP 404.
    ///
    /// Business rule: if a short_code has no corresponding URL record the
    /// /about endpoint must return 404 Not Found, consistent with the redirect
    /// endpoint behaviour.
    #[tokio::test]
    async fn get_about_returns_404_for_unknown_short_code() {
        let server = test_server(
            make_repo("known", "https://example.com/"),
            make_repo("known", "https://example.com/"),
        );

        let response = server.get("/no-such-code/about").await;

        response.assert_status(axum::http::StatusCode::NOT_FOUND);
    }
}
