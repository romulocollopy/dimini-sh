pub mod domain;
pub mod repositories;
pub mod services;
pub mod settings;
pub mod use_cases;
pub mod utils;
pub mod webapp;

use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
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
    use use_cases::create_short_code::CreateShortCodeUseCase;
    use use_cases::get_url::GetUrlUseCase;
    use webapp::{app, AppState};

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
