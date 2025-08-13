//! Run a tiny Axum server that exposes a unified webhook endpoint for multiple providers.
//! For now we register only Plivo. Add others by calling `.with(Arc::new(ProviderClient{...}))`.

use std::sync::Arc;
use axum::{routing::post, Router};
use sms_core::InboundRegistry;
use sms_web_axum::{unified_webhook, AppState};
use sms_plivo::PlivoClient;

#[tokio::main]
async fn main() {
    let plivo = PlivoClient::with_base_url("auth_id", "auth_token", "https://api.plivo.com".into());
    let registry = InboundRegistry::new().with(Arc::new(plivo));
    let state = AppState { registry };

    let app = Router::new()
        .route("/webhooks/:provider", post(unified_webhook))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
