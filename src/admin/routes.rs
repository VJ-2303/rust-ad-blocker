use axum::{Router, response::Html, routing::get};

async fn health_check() -> &'static str {
    "Amin API is running!"
}

pub fn app() -> Router {
    Router::new()
        .route("/", get(index))
        .route("/health", get(health_check))
}

async fn index() -> Html<&'static str> {
    Html("<h1>Rusthole Admin</h1><p>UI coming soon...</p>")
}
