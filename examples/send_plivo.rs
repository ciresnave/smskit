//! Send an SMS using the Plivo backend.
use sms_core::{SendRequest, SmsClient};
use sms_plivo::PlivoClient;

use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let auth_id = arg_or_env("--auth-id", "PLIVO_AUTH_ID");
    let auth_token = arg_or_env("--auth-token", "PLIVO_AUTH_TOKEN");
    let from = arg_or_env("--from", "SMS_FROM");
    let to = arg_or_env("--to", "SMS_TO");
    let text = arg_or_env("--text", "SMS_TEXT");

    let client = PlivoClient::new(auth_id, auth_token);
    let res = client
        .send(SendRequest {
            to: &to,
            from: &from,
            text: &text,
        })
        .await?;
    println!(
        "Sent via {} with id {}\nRaw: {}",
        res.provider,
        res.id,
        serde_json::to_string_pretty(&res.raw)?
    );
    Ok(())
}

fn arg_or_env(flag: &str, env_key: &str) -> String {
    let args: Vec<String> = std::env::args().collect();
    if let Some(idx) = args.iter().position(|a| a == flag) {
        if idx + 1 < args.len() {
            return args[idx + 1].clone();
        }
    }
    env::var(env_key)
        .unwrap_or_else(|_| panic!("missing {} (arg {} or env {})", flag, flag, env_key))
}
