use axum::http::{HeaderMap, HeaderValue, Request, Response};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Instant,
};
use tower::{Layer, Service};
use tracing::{info, info_span, Instrument};
use uuid::Uuid;

pub const TRACEPARENT_HEADER: &str = "traceparent";
pub const X_REQUEST_ID_HEADER: &str = "x-request-id";

/// Extract a correlation ID from incoming request headers.
///
/// Priority:
/// 1. `traceparent` (W3C Trace Context) — extracts the trace-id segment
/// 2. `X-Request-ID`
/// 3. Freshly generated UUID v4
pub fn extract_request_id(headers: &HeaderMap) -> String {
    // traceparent format: 00-{trace-id}-{parent-id}-{flags}
    if let Some(val) = headers.get(TRACEPARENT_HEADER) {
        if let Ok(s) = val.to_str() {
            let mut parts = s.splitn(4, '-');
            let version = parts.next();
            let trace_id = parts.next();
            let _parent_id = parts.next();
            let _flags = parts.next();
            if version == Some("00") {
                if let Some(id) = trace_id {
                    if !id.is_empty() {
                        return id.to_string();
                    }
                }
            }
        }
    }

    if let Some(val) = headers.get(X_REQUEST_ID_HEADER) {
        if let Ok(s) = val.to_str() {
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }

    Uuid::new_v4().to_string()
}

/// Axum/Tower layer that adds structured request/response logging with a correlation ID.
#[derive(Clone, Default)]
pub struct RequestLoggingLayer;

impl RequestLoggingLayer {
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for RequestLoggingLayer {
    type Service = RequestLoggingService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestLoggingService { inner }
    }
}

/// Tower service that logs each request and response with a shared correlation ID.
#[derive(Clone)]
pub struct RequestLoggingService<S> {
    inner: S,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for RequestLoggingService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let request_id = extract_request_id(req.headers());
        let method = req.method().to_string();
        let uri = req.uri().to_string();

        let span = info_span!(
            "request",
            request_id = %request_id,
            method = %method,
            uri = %uri,
        );

        let mut inner = self.inner.clone();

        Box::pin(
            async move {
                let start = Instant::now();

                info!(
                    request_id = %request_id,
                    method = %method,
                    uri = %uri,
                    "incoming request"
                );

                let result = inner.call(req).await;
                let latency_ms = start.elapsed().as_millis() as u64;

                match result {
                    Ok(mut response) => {
                        let status = response.status().as_u16();
                        info!(
                            status = status,
                            latency_ms = latency_ms,
                            "request completed"
                        );
                        if let Ok(val) = HeaderValue::from_str(&request_id) {
                            response.headers_mut().insert(X_REQUEST_ID_HEADER, val);
                        }
                        Ok(response)
                    }
                    Err(err) => {
                        info!(latency_ms = latency_ms, "request failed");
                        Err(err)
                    }
                }
            }
            .instrument(span),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Request, StatusCode};
    use axum::{body::Body, routing::get, Router};
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;
    use tracing_subscriber::fmt::MakeWriter;
    use uuid::Uuid;

    // ---- Log capture infrastructure ----

    #[derive(Clone)]
    struct TestWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for TestWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for TestWriter {
        type Writer = TestWriter;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    fn capture_logs() -> (impl tracing::Subscriber + Send + Sync, Arc<Mutex<Vec<u8>>>) {
        let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
        let writer = TestWriter(buf.clone());
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_writer(writer)
            .finish();
        (subscriber, buf)
    }

    fn buf_to_string(buf: &Arc<Mutex<Vec<u8>>>) -> String {
        String::from_utf8(buf.lock().unwrap().clone()).unwrap_or_default()
    }

    // ---- Unit tests: extract_request_id ----

    #[test]
    fn extracts_trace_id_from_traceparent() {
        let mut headers = HeaderMap::new();
        headers.insert(
            TRACEPARENT_HEADER,
            HeaderValue::from_static("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
        );
        assert_eq!(
            extract_request_id(&headers),
            "4bf92f3577b34da6a3ce929d0e0e4736"
        );
    }

    #[test]
    fn uses_x_request_id_when_no_traceparent() {
        let mut headers = HeaderMap::new();
        headers.insert(
            X_REQUEST_ID_HEADER,
            HeaderValue::from_static("my-custom-id"),
        );
        assert_eq!(extract_request_id(&headers), "my-custom-id");
    }

    #[test]
    fn generates_uuid_when_no_headers() {
        let id = extract_request_id(&HeaderMap::new());
        assert!(
            Uuid::parse_str(&id).is_ok(),
            "expected valid UUID, got: {id}"
        );
    }

    #[test]
    fn traceparent_takes_precedence_over_x_request_id() {
        let mut headers = HeaderMap::new();
        headers.insert(
            TRACEPARENT_HEADER,
            HeaderValue::from_static("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
        );
        headers.insert(
            X_REQUEST_ID_HEADER,
            HeaderValue::from_static("should-be-ignored"),
        );
        assert_eq!(
            extract_request_id(&headers),
            "4bf92f3577b34da6a3ce929d0e0e4736"
        );
    }

    #[test]
    fn malformed_traceparent_falls_back_to_x_request_id() {
        let mut headers = HeaderMap::new();
        headers.insert(
            TRACEPARENT_HEADER,
            HeaderValue::from_static("not-a-valid-traceparent"),
        );
        headers.insert(X_REQUEST_ID_HEADER, HeaderValue::from_static("fallback-id"));
        assert_eq!(extract_request_id(&headers), "fallback-id");
    }

    // ---- Integration tests: middleware behavior ----

    fn test_app() -> Router {
        Router::new()
            .route("/test", get(|| async { "hello" }))
            .layer(RequestLoggingLayer::new())
    }

    #[tokio::test]
    async fn echoes_x_request_id_in_response() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .uri("/test")
                    .header(X_REQUEST_ID_HEADER, "echo-me-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(X_REQUEST_ID_HEADER)
                .expect("missing x-request-id"),
            "echo-me-123"
        );
    }

    #[tokio::test]
    async fn echoes_traceparent_trace_id_as_x_request_id() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .uri("/test")
                    .header(
                        TRACEPARENT_HEADER,
                        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response
                .headers()
                .get(X_REQUEST_ID_HEADER)
                .expect("missing x-request-id"),
            "4bf92f3577b34da6a3ce929d0e0e4736"
        );
    }

    #[tokio::test]
    async fn generates_and_echoes_uuid_when_no_request_id_header() {
        let response = test_app()
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();

        let val = response
            .headers()
            .get(X_REQUEST_ID_HEADER)
            .expect("missing x-request-id")
            .to_str()
            .unwrap();
        assert!(
            Uuid::parse_str(val).is_ok(),
            "expected UUID in response, got: {val}"
        );
    }

    // ---- Log field tests ----

    #[tokio::test]
    async fn request_log_contains_method_and_uri() {
        let (subscriber, buf) = capture_logs();
        let app = test_app();

        // `set_default` holds the subscriber across `.await` points; `with_default` would
        // restore the previous subscriber before the future is polled.
        let _guard = tracing::subscriber::set_default(subscriber);
        app.oneshot(
            Request::builder()
                .method("GET")
                .uri("/test")
                .header(X_REQUEST_ID_HEADER, "log-field-test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        drop(_guard);

        let logs = buf_to_string(&buf);
        let request_line = logs
            .lines()
            .find(|l| l.contains("incoming request"))
            .unwrap_or_else(|| panic!("no 'incoming request' log line found in:\n{logs}"));

        assert!(
            request_line.contains("GET"),
            "method missing: {request_line}"
        );
        assert!(
            request_line.contains("/test"),
            "uri missing: {request_line}"
        );
        assert!(
            request_line.contains("log-field-test"),
            "request_id missing: {request_line}"
        );
    }

    #[tokio::test]
    async fn response_log_contains_status_and_latency() {
        let (subscriber, buf) = capture_logs();
        let app = test_app();

        let _guard = tracing::subscriber::set_default(subscriber);
        app.oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        drop(_guard);

        let logs = buf_to_string(&buf);
        let response_line = logs
            .lines()
            .find(|l| l.contains("request completed"))
            .unwrap_or_else(|| panic!("no 'request completed' log line found in:\n{logs}"));

        assert!(
            response_line.contains("200") || response_line.contains("\"status\""),
            "status missing: {response_line}"
        );
        assert!(
            response_line.contains("latency_ms"),
            "latency_ms missing: {response_line}"
        );
    }

    #[tokio::test]
    async fn request_id_propagates_to_handler_logs() {
        async fn logging_handler() -> &'static str {
            tracing::info!("handler executed");
            "ok"
        }

        let (subscriber, buf) = capture_logs();
        let app = Router::new()
            .route("/test", get(logging_handler))
            .layer(RequestLoggingLayer::new());

        let _guard = tracing::subscriber::set_default(subscriber);
        app.oneshot(
            Request::builder()
                .uri("/test")
                .header(X_REQUEST_ID_HEADER, "propagate-me-789")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        drop(_guard);

        let logs = buf_to_string(&buf);
        let handler_line = logs
            .lines()
            .find(|l| l.contains("handler executed"))
            .unwrap_or_else(|| panic!("no 'handler executed' log line found in:\n{logs}"));

        assert!(
            handler_line.contains("propagate-me-789"),
            "request_id not in handler log span: {handler_line}"
        );
    }
}
