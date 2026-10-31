//! {{name}}: an HTTP service.

mod app;

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,{{name_snake}}=debug")))
        .init();

    let port = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or({{port}});
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    tracing::info!("listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app::router())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutting down");
        })
        .await
}
