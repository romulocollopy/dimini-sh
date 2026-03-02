use axum::{routing::get, Router};

async fn root() -> &'static str {
    "Welcome to dimini.sh"
}

/// Build the application router.
pub fn app() -> Router {
    Router::new().route("/", get(root))
}

#[tokio::main]
async fn main() {
    let app = app();
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum_test::TestServer;

    /// GET / must return HTTP 200.
    ///
    /// Business rule: the landing page is publicly accessible and always
    /// returns a successful response. This is the entry point for all users.
    #[tokio::test]
    async fn get_root_returns_200() {
        let server = TestServer::new(app()).unwrap();
        let response = server.get("/").await;
        response.assert_status_ok();
    }
}
