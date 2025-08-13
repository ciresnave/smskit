//! Example SMS webhook server using Actix-web
use std::sync::Arc;
use actix_web::{web, App, HttpServer};
use sms_core::InboundRegistry;
use sms_plivo::PlivoClient;
use sms_web_actix::{configure_routes, AppData};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let plivo = PlivoClient::new("auth_id", "auth_token");
    let registry = InboundRegistry::new().with(Arc::new(plivo));
    let app_data = AppData { registry };

    println!("Actix-web SMS webhook server listening on http://localhost:3000");
    println!("Send webhooks to: POST http://localhost:3000/webhooks/plivo");

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(app_data.clone()))
            .configure(configure_routes)
    })
    .bind("0.0.0.0:3000")?
    .run()
    .await
}
