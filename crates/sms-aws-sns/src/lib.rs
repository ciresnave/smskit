//! # AWS SNS SMS Provider
//!
//! Amazon SNS SMS provider implementation for smskit.
//!
//! ## Sending messages
//!
//! ```rust,ignore
//! use sms_core::{SendRequest, SmsClient};
//! use sms_aws_sns::AwsSnsClient;
//!
//! let client = AwsSnsClient::new("us-east-1", "access_key", "secret_key");
//! let response = client.send(SendRequest {
//!     to: "+14155551234",
//!     from: "+10005551234",
//!     text: "Hello from AWS SNS!",
//! }).await?;
//! ```
//!
//! ## Creating from environment variables
//!
//! ```rust,ignore
//! let client = AwsSnsClient::from_env()?;
//! ```
//!
//! Reads `AWS_REGION` (or `AWS_DEFAULT_REGION`), `AWS_ACCESS_KEY_ID`, and
//! `AWS_SECRET_ACCESS_KEY` from the environment.  These are the same variable
//! names that the AWS CLI and SDKs use.
//!
//! ## Features
//!
//! - Send SMS messages via AWS SNS `Publish`
//! - Delivery status webhook parsing
//! - Subscription confirmation handling
//! - Standard AWS credential management

use async_trait::async_trait;
use aws_config::{BehaviorVersion, Region};
use aws_sdk_sns::{config::Credentials, Client as SnsClient, Config as SnsConfig};
use serde::{Deserialize, Serialize};
use sms_core::*;
use std::collections::HashMap;
use tracing::{debug, error, info, warn};

/// AWS SNS SMS client.
///
/// Wraps the AWS SDK's SNS client with the smskit [`SmsClient`] and
/// [`InboundWebhook`] traits.
///
/// # Construction
///
/// | Method | Description |
/// |--------|-------------|
/// | [`AwsSnsClient::new`] | Explicit region + credentials |
/// | [`AwsSnsClient::from_env`] | Read standard `AWS_*` env vars |
/// | [`AwsSnsClient::with_default_credentials`] | Use the default AWS credential chain (async) |
#[derive(Debug, Clone)]
pub struct AwsSnsClient {
    client: SnsClient,
    region: String,
}

/// An SNS notification envelope (used for both delivery reports and
/// subscription confirmations).
#[derive(Debug, Deserialize, Serialize)]
pub struct SnsDeliveryNotification {
    /// `"Notification"`, `"SubscriptionConfirmation"`, etc.
    #[serde(rename = "Type")]
    pub notification_type: String,
    /// SNS-assigned message ID.
    #[serde(rename = "MessageId")]
    pub message_id: String,
    /// The topic ARN this notification came from.
    #[serde(rename = "TopicArn")]
    pub topic_arn: String,
    /// The inner message body (may be JSON for delivery reports).
    #[serde(rename = "Message")]
    pub message: String,
    /// ISO 8601 / RFC 3339 timestamp.
    #[serde(rename = "Timestamp")]
    pub timestamp: String,
    /// Signature version (usually `"1"`).
    #[serde(rename = "SignatureVersion")]
    pub signature_version: String,
    /// Base64-encoded signature.
    #[serde(rename = "Signature")]
    pub signature: String,
    /// URL of the signing certificate.
    #[serde(rename = "SigningCertURL")]
    pub signing_cert_url: String,
}

/// The inner delivery-report payload nested inside
/// [`SnsDeliveryNotification::message`].
#[derive(Debug, Deserialize, Serialize)]
pub struct SmsDeliveryReport {
    /// Notification metadata.
    pub notification: SmsNotificationData,
    /// Delivery details.
    pub delivery: SmsDeliveryData,
    /// Overall status, e.g. `"SUCCESS"` or `"FAILURE"`.
    pub status: String,
    /// The original message ID.
    #[serde(rename = "messageId")]
    pub message_id: String,
    /// The destination phone number in E.164 format.
    #[serde(rename = "destinationPhoneNumber")]
    pub destination_phone_number: String,
}

/// Metadata within an SNS delivery report.
#[derive(Debug, Deserialize, Serialize)]
pub struct SmsNotificationData {
    /// The original message ID.
    #[serde(rename = "messageId")]
    pub message_id: String,
    /// Timestamp string.
    pub timestamp: String,
}

/// Delivery-specific data within an SNS delivery report.
#[derive(Debug, Deserialize, Serialize)]
pub struct SmsDeliveryData {
    /// Destination phone number.
    pub destination: String,
    /// Cost in USD (may be absent for some message types).
    #[serde(rename = "priceInUSD")]
    pub price_in_usd: Option<f64>,
    /// `"Transactional"` or `"Promotional"`.
    #[serde(rename = "smsType")]
    pub sms_type: String,
    /// Time the message spent in SNS (milliseconds).
    #[serde(rename = "dwellTimeMs")]
    pub dwell_time_ms: Option<u64>,
    /// Time until the device acknowledged (milliseconds).
    #[serde(rename = "dwellTimeMsUntilDeviceAck")]
    pub dwell_time_ms_until_device_ack: Option<u64>,
}

impl AwsSnsClient {
    /// Create a new client with explicit credentials.
    ///
    /// # Arguments
    ///
    /// * `region`            - AWS region name, e.g. `"us-east-1"`.
    /// * `access_key_id`     - IAM access key ID.
    /// * `secret_access_key` - IAM secret access key.
    pub fn new(
        region: impl Into<String>,
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
    ) -> Self {
        let region_str = region.into();
        let region_copy = region_str.clone();
        let aws_region = Region::from_static(Box::leak(region_copy.into_boxed_str()));

        let credentials = Credentials::new(
            access_key_id,
            secret_access_key,
            None,
            None,
            "smskit",
        );

        let config = SnsConfig::builder()
            .region(aws_region)
            .credentials_provider(credentials)
            .behavior_version(BehaviorVersion::latest())
            .build();

        let client = SnsClient::from_conf(config);

        Self {
            client,
            region: region_str,
        }
    }

    /// Create a client from standard AWS environment variables.
    ///
    /// | Variable                 | Required | Notes |
    /// |--------------------------|----------|-------|
    /// | `AWS_REGION`             | Yes*     | Falls back to `AWS_DEFAULT_REGION` |
    /// | `AWS_ACCESS_KEY_ID`      | Yes      | |
    /// | `AWS_SECRET_ACCESS_KEY`  | Yes      | |
    ///
    /// Returns [`SmsError::Auth`] if any required variable is missing.
    pub fn from_env() -> Result<Self, SmsError> {
        let region = std::env::var("AWS_REGION")
            .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
            .map_err(|_| SmsError::Auth("AWS_REGION (or AWS_DEFAULT_REGION) not set".into()))?;
        let access_key_id = std::env::var("AWS_ACCESS_KEY_ID")
            .map_err(|_| SmsError::Auth("AWS_ACCESS_KEY_ID not set".into()))?;
        let secret_access_key = std::env::var("AWS_SECRET_ACCESS_KEY")
            .map_err(|_| SmsError::Auth("AWS_SECRET_ACCESS_KEY not set".into()))?;
        Ok(Self::new(region, access_key_id, secret_access_key))
    }

    /// Create a client using the default AWS credential chain (profile files,
    /// instance metadata, ECS task role, etc.).
    ///
    /// This is an async constructor because the default credential chain may
    /// need to make HTTP calls (e.g. to the EC2 metadata service).
    pub async fn with_default_credentials(region: impl Into<String>) -> Self {
        let region_str = region.into();
        let aws_region = Region::new(region_str.clone());
        let config = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_region)
            .load()
            .await;

        let client = SnsClient::new(&config);

        Self {
            client,
            region: region_str,
        }
    }
}

#[async_trait]
impl SmsClient for AwsSnsClient {
    async fn send(&self, req: SendRequest<'_>) -> Result<SendResponse, SmsError> {
        info!("Sending SMS via AWS SNS to {}", req.to);

        let mut message_attributes = HashMap::new();

        message_attributes.insert(
            "AWS.SNS.SMS.SMSType".to_string(),
            aws_sdk_sns::types::MessageAttributeValue::builder()
                .data_type("String")
                .string_value("Transactional")
                .build()
                .map_err(|e| {
                    SmsError::Provider(format!("Failed to build SMS type attribute: {}", e))
                })?,
        );

        if !req.from.is_empty() && !req.from.starts_with('+') {
            message_attributes.insert(
                "AWS.SNS.SMS.SenderID".to_string(),
                aws_sdk_sns::types::MessageAttributeValue::builder()
                    .data_type("String")
                    .string_value(req.from)
                    .build()
                    .map_err(|e| {
                        SmsError::Provider(format!("Failed to build sender ID attribute: {}", e))
                    })?,
            );
        }

        debug!(
            "Sending SNS message with attributes: {:?}",
            message_attributes
        );

        let result = self
            .client
            .publish()
            .phone_number(req.to)
            .message(req.text)
            .set_message_attributes(Some(message_attributes))
            .send()
            .await
            .map_err(|e| {
                error!("AWS SNS publish failed: {}", e);
                match e.into_service_error() {
                    aws_sdk_sns::operation::publish::PublishError::AuthorizationErrorException(_) => {
                        SmsError::Auth("AWS authorization failed".to_string())
                    }
                    aws_sdk_sns::operation::publish::PublishError::InvalidParameterException(e) => {
                        SmsError::Invalid(e.message().unwrap_or("Invalid parameter").to_string())
                    }
                    aws_sdk_sns::operation::publish::PublishError::InvalidParameterValueException(e) => {
                        SmsError::Invalid(e.message().unwrap_or("Invalid parameter value").to_string())
                    }
                    e => SmsError::Provider(format!("AWS SNS error: {}", e)),
                }
            })?;

        let message_id = result.message_id().unwrap_or_default().to_string();

        info!(
            "SMS sent successfully via AWS SNS with MessageId: {}",
            message_id
        );

        let raw_json = serde_json::json!({
            "MessageId": message_id,
            "Region": self.region,
            "ResponseMetadata": {
                "HTTPStatusCode": 200
            }
        });

        Ok(SendResponse {
            id: message_id,
            provider: "aws-sns",
            raw: raw_json,
        })
    }
}

#[async_trait]
impl InboundWebhook for AwsSnsClient {
    fn provider(&self) -> &'static str {
        "aws-sns"
    }

    fn parse_inbound(&self, headers: &Headers, body: &[u8]) -> Result<InboundMessage, SmsError> {
        debug!("Parsing AWS SNS webhook");

        let payload_str = String::from_utf8(body.to_vec()).map_err(|e| {
            error!("Invalid UTF-8 in AWS SNS webhook: {}", e);
            SmsError::Provider(format!("Invalid UTF-8: {}", e))
        })?;

        if let Some(signature) = headers.iter().find_map(|(k, v)| {
            if k.eq_ignore_ascii_case("x-amz-sns-message-type") {
                Some(v.as_str())
            } else {
                None
            }
        }) {
            debug!("SNS message type: {}", signature);
        }

        let notification: SnsDeliveryNotification =
            serde_json::from_str(&payload_str).map_err(|e| {
                error!("Failed to parse SNS notification: {}", e);
                SmsError::Provider(format!("Invalid notification format: {}", e))
            })?;

        if notification.notification_type == "Notification" {
            if let Ok(delivery_report) =
                serde_json::from_str::<SmsDeliveryReport>(&notification.message)
            {
                info!(
                    "Received SMS delivery report for message: {}",
                    delivery_report.message_id
                );

                let timestamp = time::OffsetDateTime::parse(
                    &notification.timestamp,
                    &time::format_description::well_known::Rfc3339,
                )
                .ok();

                let raw_json = serde_json::to_value(&notification)
                    .map_err(|e| SmsError::Provider(format!("JSON serialization error: {}", e)))?;

                return Ok(InboundMessage {
                    id: Some(delivery_report.message_id),
                    from: "AWS-SNS".to_string(),
                    to: delivery_report.destination_phone_number,
                    text: format!("Delivery Status: {}", delivery_report.status),
                    timestamp,
                    provider: "aws-sns",
                    raw: raw_json,
                });
            }
        }

        if notification.notification_type == "SubscriptionConfirmation" {
            warn!("Received SNS subscription confirmation, manual confirmation may be required");

            let raw_json = serde_json::to_value(&notification)
                .map_err(|e| SmsError::Provider(format!("JSON serialization error: {}", e)))?;

            let timestamp = time::OffsetDateTime::parse(
                &notification.timestamp,
                &time::format_description::well_known::Rfc3339,
            )
            .ok();

            return Ok(InboundMessage {
                id: Some(notification.message_id),
                from: "AWS-SNS".to_string(),
                to: "SYSTEM".to_string(),
                text: "Subscription confirmation required".to_string(),
                timestamp,
                provider: "aws-sns",
                raw: raw_json,
            });
        }

        error!(
            "Unknown SNS notification type: {}",
            notification.notification_type
        );
        Err(SmsError::Provider(format!(
            "Unsupported notification type: {}",
            notification.notification_type
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Construction --

    #[test]
    fn client_creation() {
        let client = AwsSnsClient::new("us-east-1", "test_key", "test_secret");
        assert_eq!(client.region, "us-east-1");
    }

    #[test]
    fn client_creation_different_region() {
        let client = AwsSnsClient::new("eu-west-1", "key", "secret");
        assert_eq!(client.region, "eu-west-1");
    }

    // All from_env tests are combined into one test because env vars are
    // process-global state and parallel tests would race on them.
    // SAFETY: env var mutations are unsafe in edition 2024 because they are
    // process-global. These tests run serially within this single test
    // function, so there is no concurrent access.
    #[test]
    fn from_env_scenarios() {
        // --- missing region ---
        unsafe {
            std::env::remove_var("AWS_REGION");
            std::env::remove_var("AWS_DEFAULT_REGION");
            std::env::remove_var("AWS_ACCESS_KEY_ID");
            std::env::remove_var("AWS_SECRET_ACCESS_KEY");
        }
        let err = AwsSnsClient::from_env().unwrap_err();
        assert!(err.to_string().contains("AWS_REGION"));

        // --- missing access key ---
        unsafe { std::env::set_var("AWS_REGION", "us-east-1"); }
        let err = AwsSnsClient::from_env().unwrap_err();
        assert!(err.to_string().contains("AWS_ACCESS_KEY_ID"));

        // --- missing secret key ---
        unsafe { std::env::set_var("AWS_ACCESS_KEY_ID", "test-key"); }
        let err = AwsSnsClient::from_env().unwrap_err();
        assert!(err.to_string().contains("AWS_SECRET_ACCESS_KEY"));

        // --- success ---
        unsafe { std::env::set_var("AWS_SECRET_ACCESS_KEY", "test-secret"); }
        let client = AwsSnsClient::from_env().unwrap();
        assert_eq!(client.region, "us-east-1");

        // --- fallback to AWS_DEFAULT_REGION ---
        unsafe {
            std::env::remove_var("AWS_REGION");
            std::env::set_var("AWS_DEFAULT_REGION", "ap-southeast-1");
        }
        let client = AwsSnsClient::from_env().unwrap();
        assert_eq!(client.region, "ap-southeast-1");

        // cleanup
        unsafe {
            std::env::remove_var("AWS_REGION");
            std::env::remove_var("AWS_DEFAULT_REGION");
            std::env::remove_var("AWS_ACCESS_KEY_ID");
            std::env::remove_var("AWS_SECRET_ACCESS_KEY");
        }
    }

    // -- Provider trait --

    #[test]
    fn provider_name() {
        let client = AwsSnsClient::new("us-east-1", "test_key", "test_secret");
        assert_eq!(client.provider(), "aws-sns");
    }

    // -- Webhook parsing: delivery report --

    fn delivery_report_json() -> String {
        r#"{
            "Type": "Notification",
            "MessageId": "test-message-id",
            "TopicArn": "arn:aws:sns:us-east-1:123456789012:test-topic",
            "Message": "{\"notification\":{\"messageId\":\"msg-123\",\"timestamp\":\"2023-01-01T00:00:00.000Z\"},\"delivery\":{\"destination\":\"+1234567890\",\"priceInUSD\":0.00645,\"smsType\":\"Transactional\"},\"status\":\"SUCCESS\",\"messageId\":\"msg-123\",\"destinationPhoneNumber\":\"+1234567890\"}",
            "Timestamp": "2023-01-01T00:00:00.000Z",
            "SignatureVersion": "1",
            "Signature": "test-signature",
            "SigningCertURL": "https://sns.us-east-1.amazonaws.com/test.pem"
        }"#.to_string()
    }

    #[test]
    fn webhook_parsing_delivery_report() {
        let client = AwsSnsClient::new("us-east-1", "test_key", "test_secret");
        let json = delivery_report_json();
        let headers = vec![];
        let result = client.parse_inbound(&headers, json.as_bytes());

        assert!(result.is_ok());
        let message = result.unwrap();
        assert_eq!(message.id, Some("msg-123".to_string()));
        assert_eq!(message.to, "+1234567890");
        assert_eq!(message.provider, "aws-sns");
        assert!(message.text.contains("SUCCESS"));
        assert!(message.timestamp.is_some());
    }

    #[test]
    fn webhook_delivery_report_from_field() {
        let client = AwsSnsClient::new("us-east-1", "k", "s");
        let json = delivery_report_json();
        let msg = client.parse_inbound(&vec![], json.as_bytes()).unwrap();
        assert_eq!(msg.from, "AWS-SNS");
    }

    #[test]
    fn webhook_delivery_report_raw_contains_notification() {
        let client = AwsSnsClient::new("us-east-1", "k", "s");
        let json = delivery_report_json();
        let msg = client.parse_inbound(&vec![], json.as_bytes()).unwrap();
        assert!(msg.raw.get("TopicArn").is_some());
    }

    // -- Webhook parsing: subscription confirmation --

    fn subscription_confirmation_json() -> String {
        r#"{
            "Type": "SubscriptionConfirmation",
            "MessageId": "subscription-message-id",
            "TopicArn": "arn:aws:sns:us-east-1:123456789012:test-topic",
            "Message": "You have chosen to subscribe to the topic...",
            "Timestamp": "2023-01-01T00:00:00.000Z",
            "SignatureVersion": "1",
            "Signature": "test-signature",
            "SigningCertURL": "https://sns.us-east-1.amazonaws.com/test.pem"
        }"#.to_string()
    }

    #[test]
    fn webhook_parsing_subscription_confirmation() {
        let client = AwsSnsClient::new("us-east-1", "test_key", "test_secret");
        let json = subscription_confirmation_json();
        let result = client.parse_inbound(&vec![], json.as_bytes());

        assert!(result.is_ok());
        let message = result.unwrap();
        assert_eq!(message.id, Some("subscription-message-id".to_string()));
        assert_eq!(message.text, "Subscription confirmation required");
        assert_eq!(message.to, "SYSTEM");
        assert_eq!(message.provider, "aws-sns");
    }

    // -- Webhook parsing: unknown type --

    #[test]
    fn webhook_parsing_unknown_type_errors() {
        let client = AwsSnsClient::new("us-east-1", "k", "s");
        let json = r#"{
            "Type": "SomethingNew",
            "MessageId": "id",
            "TopicArn": "arn",
            "Message": "...",
            "Timestamp": "2023-01-01T00:00:00.000Z",
            "SignatureVersion": "1",
            "Signature": "sig",
            "SigningCertURL": "https://example.com/cert.pem"
        }"#;
        let result = client.parse_inbound(&vec![], json.as_bytes());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsupported notification type"));
    }

    // -- Webhook parsing: invalid JSON --

    #[test]
    fn webhook_parsing_invalid_json() {
        let client = AwsSnsClient::new("us-east-1", "k", "s");
        let result = client.parse_inbound(&vec![], b"not json");
        assert!(result.is_err());
    }

    // -- Webhook parsing: invalid UTF-8 --

    #[test]
    fn webhook_parsing_invalid_utf8() {
        let client = AwsSnsClient::new("us-east-1", "k", "s");
        let result = client.parse_inbound(&vec![], &[0xFF, 0xFE]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("UTF-8"));
    }

    // -- Webhook parsing: message type header --

    #[test]
    fn webhook_with_message_type_header() {
        let client = AwsSnsClient::new("us-east-1", "k", "s");
        let json = subscription_confirmation_json();
        let headers = vec![(
            "x-amz-sns-message-type".to_string(),
            "SubscriptionConfirmation".to_string(),
        )];
        let result = client.parse_inbound(&headers, json.as_bytes());
        assert!(result.is_ok());
    }

    // -- Notification payload parsing: delivery report with failure --

    #[test]
    fn webhook_delivery_report_failure_status() {
        let client = AwsSnsClient::new("us-east-1", "k", "s");
        let json = r#"{
            "Type": "Notification",
            "MessageId": "test-id",
            "TopicArn": "arn:aws:sns:us-east-1:123:topic",
            "Message": "{\"notification\":{\"messageId\":\"msg-fail\",\"timestamp\":\"2023-06-15T10:00:00.000Z\"},\"delivery\":{\"destination\":\"+19875551234\",\"smsType\":\"Transactional\"},\"status\":\"FAILURE\",\"messageId\":\"msg-fail\",\"destinationPhoneNumber\":\"+19875551234\"}",
            "Timestamp": "2023-06-15T10:00:00.000Z",
            "SignatureVersion": "1",
            "Signature": "sig",
            "SigningCertURL": "https://sns.us-east-1.amazonaws.com/cert.pem"
        }"#;
        let msg = client.parse_inbound(&vec![], json.as_bytes()).unwrap();
        assert!(msg.text.contains("FAILURE"));
        assert_eq!(msg.id, Some("msg-fail".into()));
    }

    // -- Notification where inner message is NOT a delivery report --

    #[test]
    fn webhook_notification_with_non_delivery_message() {
        let client = AwsSnsClient::new("us-east-1", "k", "s");
        // The inner Message is not a valid SmsDeliveryReport JSON
        let json = r#"{
            "Type": "Notification",
            "MessageId": "notif-id",
            "TopicArn": "arn:aws:sns:us-east-1:123:topic",
            "Message": "This is a plain text notification, not a delivery report",
            "Timestamp": "2023-01-01T00:00:00.000Z",
            "SignatureVersion": "1",
            "Signature": "sig",
            "SigningCertURL": "https://sns.us-east-1.amazonaws.com/cert.pem"
        }"#;
        // This should fall through to the "Unsupported notification type" error
        // because the notification type IS "Notification" but the inner message
        // is not a delivery report, AND it's not "SubscriptionConfirmation"
        // Wait — looking at the code, if inner parse fails it falls through past
        // the SubscriptionConfirmation check to the final error.
        // But the type IS "Notification", so it won't match SubscriptionConfirmation.
        // It should hit the final error branch.
        let result = client.parse_inbound(&vec![], json.as_bytes());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsupported notification type"));
    }

    // -- SnsDeliveryNotification serde --

    #[test]
    fn sns_notification_serde_roundtrip() {
        let json = delivery_report_json();
        let notif: SnsDeliveryNotification = serde_json::from_str(&json).unwrap();
        assert_eq!(notif.notification_type, "Notification");
        assert_eq!(notif.message_id, "test-message-id");

        let reserialized = serde_json::to_string(&notif).unwrap();
        let notif2: SnsDeliveryNotification = serde_json::from_str(&reserialized).unwrap();
        assert_eq!(notif2.message_id, notif.message_id);
    }

    // -- SmsDeliveryReport serde --

    #[test]
    fn delivery_report_serde() {
        let inner = r#"{"notification":{"messageId":"m1","timestamp":"2023-01-01T00:00:00Z"},"delivery":{"destination":"+1","priceInUSD":0.005,"smsType":"Transactional","dwellTimeMs":100,"dwellTimeMsUntilDeviceAck":200},"status":"SUCCESS","messageId":"m1","destinationPhoneNumber":"+1"}"#;
        let report: SmsDeliveryReport = serde_json::from_str(inner).unwrap();
        assert_eq!(report.status, "SUCCESS");
        assert_eq!(report.message_id, "m1");
        assert_eq!(report.delivery.price_in_usd, Some(0.005));
        assert_eq!(report.delivery.dwell_time_ms, Some(100));
    }

    // -- OwnedSendRequest integration --

    #[test]
    fn owned_request_can_be_borrowed_for_send() {
        let owned = sms_core::OwnedSendRequest::new("+14155551234", "MySenderID", "Hello SNS!");
        let borrowed = owned.as_ref();
        assert_eq!(borrowed.to, "+14155551234");
        assert_eq!(borrowed.from, "MySenderID");
        assert!(!borrowed.from.starts_with('+'));
    }
}
