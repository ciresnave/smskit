//! Example SMS webhook server using raw Hyper
use std::sync::Arc;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use sms_core::InboundRegistry;
use sms_plivo::PlivoClient;
use sms_web_hyper::{make_service, AppState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let plivo = PlivoClient::new("auth_id", "auth_token");
    let registry = InboundRegistry::new().with(Arc::new(plivo));
    let state = AppState { registry };

    let service = make_service(state);
    let addr = "0.0.0.0:3000";
    let listener = TcpListener::bind(addr).await?;

    println!("Hyper SMS webhook server listening on http://{}", addr);
    println!("Send webhooks to: POST http://{}/webhooks/plivo", addr);

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let service = service.clone();

        tokio::task::spawn(async move {
            let service_fn = hyper::service::service_fn(move |req| {
                let service = service.clone();
                async move { service(req).await }
            });

            if let Err(err) = http1::Builder::new()
                .serve_connection(io, service_fn)
                .await
            {
                eprintln!("Error serving connection: {:?}", err);
            }
        });
    }
}
