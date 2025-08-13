//! Example SMS webhook server using Tide
use std::sync::Arc;
use sms_core::InboundRegistry;
use sms_plivo::PlivoClient;
use sms_web_tide::{configure_routes, AppState};

#[async_std::main]
async fn main() -> tide::Result<()> {
    let plivo = PlivoClient::new("auth_id", "auth_token");
    let registry = InboundRegistry::new().with(Arc::new(plivo));
    let state = AppState { registry };

    let mut app = tide::with_state(state);
    configure_routes(&mut app);

    println!("Tide SMS webhook server listening on http://localhost:3000");
    println!("Send webhooks to: POST http://localhost:3000/webhooks/plivo");

    app.listen("0.0.0.0:3000").await?;
    Ok(())
}
