//! Example SMS webhook server using Rocket
use std::sync::Arc;
use rocket::State;
use sms_core::InboundRegistry;
use sms_plivo::PlivoClient;
use sms_web_rocket::{unified_webhook, AppState};

#[rocket::launch]
fn rocket() -> _ {
    let plivo = PlivoClient::new("auth_id", "auth_token");
    let registry = InboundRegistry::new().with(Arc::new(plivo));
    let state = AppState { registry };

    println!("Rocket SMS webhook server will start on http://localhost:8000");
    println!("Send webhooks to: POST http://localhost:8000/webhooks/plivo");

    rocket::build()
        .manage(state)
        .mount("/", rocket::routes![unified_webhook])
}
