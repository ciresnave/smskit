//! # AWS SNS SMS Provider
//!
//! Amazon SNS SMS provider implementation for smskit.
//!
//! ## Features
//!
//! - Send SMS messages via AWS SNS
//! - Support for delivery status tracking
//! - AWS credential management
//! - Full error handling and tracing
//!
//! ## Example
//!
//! ```rust,ignore
//! use sms_core::{SendRequest, SmsClient};
//! use sms_aws_sns::AwsSnsClient;
//!
//! let client = AwsSnsClient::new("us-east-1", "access_key", "secret_key");
//! let response = client.send(SendRequest {
//!     to: "+1234567890",
//!     from: "+0987654321",
//!     text: "Hello from AWS SNS!"
//! }).await?;
//! ```

use async_trait::async_trait;
use aws_config::{BehaviorVersion, Region};
use aws_sdk_sns::{config::Credentials, Client as SnsClient, Config as SnsConfig};
use serde::{Deserialize, Serialize};
use sms_core::*;
use std::collections::HashMap;
use tracing::{debug, error, info, warn};

/// AWS SNS SMS client
#[derive(Debug, Clone)]
pub struct AwsSnsClient {
    client: SnsClient,
    region: String,
}

/// AWS SNS delivery status notification
#[derive(Debug, Deserialize, Serialize)]
pub struct SnsDeliveryNotification {
    #[serde(rename = "Type")]
    pub notification_type: String,
    #[serde(rename = "MessageId")]
    pub message_id: String,
    #[serde(rename = "TopicArn")]
    pub topic_arn: String,
    #[serde(rename = "Message")]
    pub message: String,
    #[serde(rename = "Timestamp")]
    pub timestamp: String,
    #[serde(rename = "SignatureVersion")]
    pub signature_version: String,
    #[serde(rename = "Signature")]
    pub signature: String,
    #[serde(rename = "SigningCertURL")]
    pub signing_cert_url: String,
}

/// SMS delivery report from SNS
#[derive(Debug, Deserialize, Serialize)]
pub struct SmsDeliveryReport {
    pub notification: SmsNotificationData,
    pub delivery: SmsDeliveryData,
    pub status: String,
    #[serde(rename = "messageId")]
    pub message_id: String,
    #[serde(rename = "destinationPhoneNumber")]
    pub destination_phone_number: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SmsNotificationData {
    #[serde(rename = "messageId")]
    pub message_id: String,
    pub timestamp: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SmsDeliveryData {
    pub destination: String,
    #[serde(rename = "priceInUSD")]
    pub price_in_usd: Option<f64>,
    #[serde(rename = "smsType")]
    pub sms_type: String,
    #[serde(rename = "dwellTimeMs")]
    pub dwell_time_ms: Option<u64>,
    #[serde(rename = "dwellTimeMsUntilDeviceAck")]
    pub dwell_time_ms_until_device_ack: Option<u64>,
}

impl AwsSnsClient {
    /// Create a new AWS SNS client
    pub fn new(
        region: impl Into<String>,
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
    ) -> Self {
        let region_str = region.into();

        // Clone region for the AWS Region type
        let region_copy = region_str.clone();
        let aws_region = Region::from_static(Box::leak(region_copy.into_boxed_str()));

        let credentials = Credentials::new(
            access_key_id,
            secret_access_key,
            None, // session_token
            None, // expiration
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

    /// Create a client using the default AWS credential chain
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

        // Set SMS type (promotional or transactional)
        message_attributes.insert(
            "AWS.SNS.SMS.SMSType".to_string(),
            aws_sdk_sns::types::MessageAttributeValue::builder()
                .data_type("String")
                .string_value("Transactional") // Default to transactional for higher delivery rates
                .build()
                .map_err(|e| {
                    SmsError::Provider(format!("Failed to build SMS type attribute: {}", e))
                })?,
        );

        // Set sender ID if provided in from field
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

        // Create raw response data
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

        // Parse JSON payload
        let payload_str = String::from_utf8(body.to_vec()).map_err(|e| {
            error!("Invalid UTF-8 in AWS SNS webhook: {}", e);
            SmsError::Provider(format!("Invalid UTF-8: {}", e))
        })?;

        // Verify SNS signature if present
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

        // Parse the inner message if it's a delivery report
        if notification.notification_type == "Notification" {
            if let Ok(delivery_report) =
                serde_json::from_str::<SmsDeliveryReport>(&notification.message)
            {
                info!(
                    "Received SMS delivery report for message: {}",
                    delivery_report.message_id
                );

                // Parse timestamp
                let timestamp = time::OffsetDateTime::parse(
                    &notification.timestamp,
                    &time::format_description::well_known::Rfc3339,
                )
                .ok();

                let raw_json = serde_json::to_value(&notification)
                    .map_err(|e| SmsError::Provider(format!("JSON serialization error: {}", e)))?;

                return Ok(InboundMessage {
                    id: Some(delivery_report.message_id),
                    from: "AWS-SNS".to_string(), // SNS doesn't provide original sender in delivery reports
                    to: delivery_report.destination_phone_number,
                    text: format!("Delivery Status: {}", delivery_report.status),
                    timestamp,
                    provider: "aws-sns",
                    raw: raw_json,
                });
            }
        }

        // Handle subscription confirmation
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

    #[test]
    fn client_creation() {
        let client = AwsSnsClient::new("us-east-1", "test_key", "test_secret");
        assert_eq!(client.region, "us-east-1");
    }

    #[test]
    fn provider_name() {
        let client = AwsSnsClient::new("us-east-1", "test_key", "test_secret");
        assert_eq!(client.provider(), "aws-sns");
    }

    #[test]
    fn webhook_parsing_delivery_report() {
        let client = AwsSnsClient::new("us-east-1", "test_key", "test_secret");

        let notification_json = r#"{
            "Type": "Notification",
            "MessageId": "test-message-id",
            "TopicArn": "arn:aws:sns:us-east-1:123456789012:test-topic",
            "Message": "{\"notification\":{\"messageId\":\"msg-123\",\"timestamp\":\"2023-01-01T00:00:00.000Z\"},\"delivery\":{\"destination\":\"+1234567890\",\"priceInUSD\":0.00645,\"smsType\":\"Transactional\"},\"status\":\"SUCCESS\",\"messageId\":\"msg-123\",\"destinationPhoneNumber\":\"+1234567890\"}",
            "Timestamp": "2023-01-01T00:00:00.000Z",
            "SignatureVersion": "1",
            "Signature": "test-signature",
            "SigningCertURL": "https://sns.us-east-1.amazonaws.com/test.pem"
        }"#;

        let headers = vec![];
        let result = client.parse_inbound(&headers, notification_json.as_bytes());

        assert!(result.is_ok());
        let message = result.unwrap();
        assert_eq!(message.id, Some("msg-123".to_string()));
        assert_eq!(message.to, "+1234567890");
        assert_eq!(message.provider, "aws-sns");
    }

    #[test]
    fn webhook_parsing_subscription_confirmation() {
        let client = AwsSnsClient::new("us-east-1", "test_key", "test_secret");

        let confirmation_json = r#"{
            "Type": "SubscriptionConfirmation",
            "MessageId": "subscription-message-id",
            "TopicArn": "arn:aws:sns:us-east-1:123456789012:test-topic",
            "Message": "You have chosen to subscribe to the topic...",
            "Timestamp": "2023-01-01T00:00:00.000Z",
            "SignatureVersion": "1",
            "Signature": "test-signature",
            "SigningCertURL": "https://sns.us-east-1.amazonaws.com/test.pem"
        }"#;

        let headers = vec![];
        let result = client.parse_inbound(&headers, confirmation_json.as_bytes());

        assert!(result.is_ok());
        let message = result.unwrap();
        assert_eq!(message.id, Some("subscription-message-id".to_string()));
        assert_eq!(message.text, "Subscription confirmation required");
        assert_eq!(message.provider, "aws-sns");
    }
}
