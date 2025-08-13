//! Example SMS webhook server using Warp
use sms_core::InboundRegistry;
use sms_plivo::PlivoClient;
use sms_web_warp::{webhook_filter, AppState};
use std::sync::Arc;
use warp::Filter;

#[tokio::main]
async fn main() {
    let plivo = PlivoClient::new("auth_id", "auth_token");
    let registry = InboundRegistry::new().with(Arc::new(plivo));
    let state = AppState { registry };

    let routes = webhook_filter(state).with(warp::log("webhooks"));

    println!("Warp SMS webhook server listening on http://localhost:3000");
    println!("Send webhooks to: POST http://localhost:3000/webhooks/plivo");

    warp::serve(routes).run(([0, 0, 0, 0], 3000)).await;
}
