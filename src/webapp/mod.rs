use crate::use_cases::create_short_code::{CreateShortCodeError, CreateShortCodeUseCase};
use crate::use_cases::get_url::{GetUrlError, GetUrlUseCase, UrlRepositoryPort};
use axum::{
    extract::{Json, Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, get_service, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::services::{ServeDir, ServeFile};

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
        .route("/", get_service(ServeFile::new("public/index.html")))
        .route("/create/", post(create_short_code::<R>))
        .nest_service("/assets/", ServeDir::new("public/assets"))
        .route("/{short_code}", get(redirect_short_code::<R>))
        .route("/{short_code}/about", get(about_short_code::<R>))
        .with_state(state)
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
            Output = Result<
                Option<UrlRecord>,
                crate::repositories::url_repository::RepositoryError,
            >,
        > + Send {
            self.0.lock().unwrap().find_by_short_code(short_code)
        }

        fn find_by_hash(
            &self,
            hash: &str,
        ) -> impl std::future::Future<
            Output = Result<
                Option<UrlRecord>,
                crate::repositories::url_repository::RepositoryError,
            >,
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
        mock.expect_find_by_short_code().returning(move |code| {
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

    #[tokio::test]
    async fn post_url_returns_201_with_short_code() {
        let server = test_server(
            make_repo("noslot", "https://other.com/"),
            make_repo("noslot", "https://other.com/"),
        );

        let response = server
            .post("/create/")
            .json(&json!({ "url": "https://example.com/" }))
            .await;

        response.assert_status(axum::http::StatusCode::CREATED);
        let body: Value = response.json();
        assert!(
            body.get("short_code").is_some(),
            "response body must contain a `short_code` key, got: {body}"
        );
    }

    #[tokio::test]
    async fn post_url_with_explicit_short_code_returns_201_with_that_code() {
        let server = test_server(
            make_repo("taken", "https://other.com/"),
            make_repo("taken", "https://other.com/"),
        );

        let response = server
            .post("/create/")
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

    #[tokio::test]
    async fn post_url_with_invalid_url_returns_422() {
        let server = test_server(
            make_repo("irrelevant", "https://example.com/"),
            make_repo("irrelevant", "https://example.com/"),
        );

        let response = server
            .post("/create/")
            .json(&json!({ "url": "not-a-valid-url!!!" }))
            .await;

        response.assert_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn post_url_with_conflicting_short_code_returns_409() {
        let server = test_server(
            make_repo("taken", "https://other.com/"),
            make_repo("taken", "https://other.com/"),
        );

        let response = server
            .post("/create/")
            .json(&json!({ "url": "https://example.com/", "short_code": "taken" }))
            .await;

        response.assert_status(axum::http::StatusCode::CONFLICT);
    }

    // -----------------------------------------------------------------------
    // GET /:short_code/about tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_about_returns_200_with_url_details() {
        let canonical = "https://example.com/destination";
        let server = test_server(make_repo("abc1", canonical), make_repo("abc1", canonical));

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
