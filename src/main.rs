//! microtak-admin-web: a small, standalone web UI for managing a
//! microtak-server instance -- enrollment invite tokens (with QR codes)
//! and mission role assignment. See README.md for configuration and the
//! "separate optional tool" design rationale.

use std::sync::Arc;

use axum::middleware;
use axum::Router;

use microtak_admin_web::client::MicrotakClient;
use microtak_admin_web::config::Config;
use microtak_admin_web::{auth, pages};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let config = Config::from_env();

    let client = MicrotakClient::new(
        &config.server_url,
        &config.admin_cert_path,
        &config.admin_key_path,
        &config.ca_cert_path,
    )
    .unwrap_or_else(|error| panic!("failed to build microtak-server client: {error}"));

    let state = pages::AppState {
        client: Arc::new(client),
        enrollment_url: config.enrollment_url,
    };

    let password = Arc::new(config.web_password);
    let app: Router = pages::router(state).layer(middleware::from_fn_with_state(
        password,
        auth::require_password,
    ));

    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await
        .unwrap_or_else(|error| panic!("failed to bind {}: {error}", config.bind_addr));

    tracing::info!(bind_addr = %config.bind_addr, server_url = %config.server_url, "microtak-admin-web starting");
    axum::serve(listener, app)
        .await
        .unwrap_or_else(|error| panic!("server error: {error}"));
}
