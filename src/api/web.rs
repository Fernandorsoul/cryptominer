use axum::response::Html;

/// Serve the embedded web dashboard.
pub async fn serve_dashboard() -> Html<&'static str> {
    Html(include_str!("dashboard.html"))
}