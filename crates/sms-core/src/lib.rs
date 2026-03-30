//! # SMS Core
//!
//! Core traits and types for the smskit multi-provider SMS abstraction.
//!
//! This crate provides the fundamental building blocks for SMS operations:
//! - [`SmsClient`] trait for sending SMS messages
//! - [`InboundWebhook`] trait for processing incoming webhooks
//! - [`SmsRouter`] for dispatching sends to named providers
//! - [`FallbackClient`] for try-in-order provider chaining
//! - Common types for requests, responses, and errors
//!
//! ## Sending a message
//!
//! ```rust,ignore
//! use sms_core::{SendRequest, SmsClient};
//!
//! // Any SMS provider implements SmsClient
//! let response = client.send(SendRequest {
//!     to: "+1234567890",
//!     from: "+0987654321",
//!     text: "Hello world!"
//! }).await?;
//! ```
//!
//! ## Owned requests for async contexts
//!
//! When you need to hold a request across `.await` points, use [`OwnedSendRequest`]:
//!
//! ```rust,ignore
//! use sms_core::OwnedSendRequest;
//!
//! let req = OwnedSendRequest::new("+1234567890", "+0987654321", "Hello!");
//! // Can be moved across .await boundaries freely
//! let response = client.send(req.as_ref()).await?;
//! ```
//!
//! ## Routing to named providers
//!
//! ```rust,ignore
//! use sms_core::SmsRouter;
//!
//! let router = SmsRouter::new()
//!     .with("plivo", plivo_client)
//!     .with("aws-sns", sns_client);
//!
//! // Dispatch by name — callers don't need provider crate imports
//! let response = router.send_via("plivo", SendRequest { .. }).await?;
//! ```
//!
//! ## Fallback chaining
//!
//! ```rust,ignore
//! use sms_core::FallbackClient;
//!
//! let client = FallbackClient::new(vec![primary_client, backup_client]);
//! // Tries each provider in order; returns first success
//! let response = client.send(SendRequest { .. }).await?;
//! ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use std::collections::HashMap;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during SMS send operations.
///
/// Each variant maps to a distinct failure class so callers can decide whether
/// to retry, re-authenticate, fix their input, or escalate.
#[derive(Debug, thiserror::Error)]
pub enum SmsError {
    /// An HTTP / network-level transport error (timeouts, DNS failures, etc.).
    #[error("http error: {0}")]
    Http(String),

    /// The provider rejected the caller's credentials.
    #[error("authentication error: {0}")]
    Auth(String),

    /// The request itself was malformed (bad phone number, empty text, etc.).
    #[error("invalid request: {0}")]
    Invalid(String),

    /// The provider returned a business-logic error (insufficient balance,
    /// blocked destination, etc.).
    #[error("provider error: {0}")]
    Provider(String),

    /// Catch-all for errors that don't fit the categories above.
    #[error("unexpected: {0}")]
    Unexpected(String),
}

/// Errors specific to inbound webhook processing.
#[derive(Debug, thiserror::Error)]
pub enum WebhookError {
    /// The provider name in the URL did not match any registered provider.
    #[error("provider not found: {0}")]
    ProviderNotFound(String),

    /// Signature / HMAC verification on the incoming payload failed.
    #[error("signature verification failed: {0}")]
    VerificationFailed(String),

    /// The payload could not be deserialized into the expected format.
    #[error("parsing failed: {0}")]
    ParseError(String),

    /// A lower-level [`SmsError`] surfaced during webhook handling.
    #[error("SMS processing error: {0}")]
    SmsError(#[from] SmsError),
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

/// Minimal HTTP status codes used by [`WebhookResponse`].
///
/// Only the codes that the webhook pipeline actually produces are listed here;
/// this is **not** a general-purpose HTTP status enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpStatus {
    /// 200 OK
    Ok = 200,
    /// 400 Bad Request
    BadRequest = 400,
    /// 401 Unauthorized
    Unauthorized = 401,
    /// 404 Not Found
    NotFound = 404,
    /// 500 Internal Server Error
    InternalServerError = 500,
}

impl HttpStatus {
    /// Return the numeric HTTP status code.
    pub fn as_u16(self) -> u16 {
        self as u16
    }
}

// ---------------------------------------------------------------------------
// Send request / response
// ---------------------------------------------------------------------------

/// A borrowing SMS send request.
///
/// This is the type accepted by [`SmsClient::send`].  It borrows its string
/// fields to avoid allocations on the hot path.  If you need an owned variant
/// that can live across `.await` points, see [`OwnedSendRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendRequest<'a> {
    /// E.164 destination phone number, e.g. `"+14155551234"`.
    pub to: &'a str,
    /// E.164 sender / originating number, or an alphanumeric sender ID.
    pub from: &'a str,
    /// The message body (plain text).
    pub text: &'a str,
}

/// An owned variant of [`SendRequest`] for use in async contexts.
///
/// Holding `&str` references across `.await` points requires the referent to
/// outlive the future, which creates lifetime friction when the strings come
/// from `String` values.  `OwnedSendRequest` sidesteps this by owning the
/// data and offering [`as_ref`](OwnedSendRequest::as_ref) to borrow a
/// `SendRequest<'_>` at the call site.
///
/// # Examples
///
/// ```
/// use sms_core::OwnedSendRequest;
///
/// let req = OwnedSendRequest::new("+14155551234", "+10005551234", "Hello!");
/// assert_eq!(req.to, "+14155551234");
///
/// // Borrow as a SendRequest<'_> for SmsClient::send()
/// let borrowed = req.as_ref();
/// assert_eq!(borrowed.to, req.to);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OwnedSendRequest {
    /// E.164 destination phone number.
    pub to: String,
    /// E.164 sender number or alphanumeric sender ID.
    pub from: String,
    /// The message body (plain text).
    pub text: String,
}

impl OwnedSendRequest {
    /// Create a new owned send request.
    ///
    /// All three parameters accept anything that converts to `String`,
    /// so both `&str` and `String` work without explicit `.to_string()` calls.
    pub fn new(
        to: impl Into<String>,
        from: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            to: to.into(),
            from: from.into(),
            text: text.into(),
        }
    }

    /// Borrow this owned request as a [`SendRequest`] suitable for
    /// [`SmsClient::send`].
    pub fn as_ref(&self) -> SendRequest<'_> {
        SendRequest {
            to: &self.to,
            from: &self.from,
            text: &self.text,
        }
    }
}

impl<'a> From<SendRequest<'a>> for OwnedSendRequest {
    fn from(req: SendRequest<'a>) -> Self {
        Self {
            to: req.to.to_owned(),
            from: req.from.to_owned(),
            text: req.text.to_owned(),
        }
    }
}

impl<'a> From<&'a OwnedSendRequest> for SendRequest<'a> {
    fn from(req: &'a OwnedSendRequest) -> Self {
        req.as_ref()
    }
}

/// The response returned after a successful SMS send.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendResponse {
    /// Provider-assigned message identifier.
    pub id: String,
    /// Name of the provider that handled the send, e.g. `"plivo"`.
    pub provider: &'static str,
    /// Raw JSON payload from the provider, useful for debugging / audit logs.
    pub raw: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Inbound message
// ---------------------------------------------------------------------------

/// A provider-normalized inbound SMS message (e.g. a reply or MO message).
///
/// Every provider adapter converts its native format into this common struct
/// so that downstream code never needs to know which provider delivered it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboundMessage {
    /// Provider-assigned message ID (if available).
    pub id: Option<String>,
    /// Sender phone number.
    pub from: String,
    /// Destination phone number / short code.
    pub to: String,
    /// The message body.
    pub text: String,
    /// When the message was sent/received (if the provider supplies it).
    pub timestamp: Option<OffsetDateTime>,
    /// Which provider delivered this message, e.g. `"plivo"`.
    pub provider: &'static str,
    /// Raw provider payload for debugging.
    pub raw: serde_json::Value,
}

/// Result of webhook processing, containing both the message and response info.
#[derive(Debug, Clone)]
pub struct WebhookResult {
    /// The parsed inbound message.
    pub message: InboundMessage,
    /// HTTP status to return to the provider's webhook caller.
    pub status: u16,
}

/// A framework-agnostic webhook HTTP response.
///
/// Framework adapters convert this into their native response type using the
/// `ResponseConverter` trait defined in `sms-web-generic`.
#[derive(Debug, Clone)]
pub struct WebhookResponse {
    /// HTTP status code to return.
    pub status: HttpStatus,
    /// Response body (JSON).
    pub body: String,
    /// The `Content-Type` header value.
    pub content_type: String,
}

impl WebhookResponse {
    /// Build a 200 OK response containing the serialized [`InboundMessage`].
    pub fn success(message: InboundMessage) -> Self {
        Self {
            status: HttpStatus::Ok,
            body: serde_json::to_string(&message).unwrap_or_else(|_| "{}".to_string()),
            content_type: "application/json".to_string(),
        }
    }

    /// Build an error response with the given status and human-readable message.
    pub fn error(status: HttpStatus, message: &str) -> Self {
        Self {
            status,
            body: format!(r#"{{"error": "{}"}}"#, message.replace('"', r#"\""#)),
            content_type: "application/json".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Core trait: SmsClient
// ---------------------------------------------------------------------------

/// The primary trait for sending SMS messages.
///
/// Every provider crate (`sms-plivo`, `sms-aws-sns`, `sms-twilio`) implements
/// this trait.  Because the trait is **object-safe** (`Send + Sync`, no
/// associated types), you can use `Box<dyn SmsClient>` or
/// `Arc<dyn SmsClient>` for dynamic dispatch — which is exactly what
/// [`SmsRouter`] and [`FallbackClient`] do under the hood.
///
/// # Example
///
/// ```rust,ignore
/// use sms_core::{SmsClient, SendRequest};
///
/// async fn send_otp(client: &dyn SmsClient) -> Result<String, sms_core::SmsError> {
///     let resp = client.send(SendRequest {
///         to: "+14155551234",
///         from: "+10005551234",
///         text: "Your code is 123456",
///     }).await?;
///     Ok(resp.id)
/// }
/// ```
#[async_trait]
pub trait SmsClient: Send + Sync {
    /// Send a single text SMS and return the provider's response.
    async fn send(&self, req: SendRequest<'_>) -> Result<SendResponse, SmsError>;
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

/// Generate a random UUID v4 string, useful as a fallback message ID when the
/// provider does not return one.
pub fn fallback_id() -> String {
    Uuid::new_v4().to_string()
}

/// Lightweight header representation (`Vec<(name, value)>`) that avoids
/// coupling the core crate to any particular HTTP framework.
pub type Headers = Vec<(String, String)>;

// ---------------------------------------------------------------------------
// Inbound webhook trait
// ---------------------------------------------------------------------------

/// Provider-agnostic interface for processing inbound SMS webhooks.
///
/// Each provider crate implements this trait on its client type, enabling the
/// unified [`InboundRegistry`] and `WebhookProcessor` to handle any provider
/// without compile-time knowledge of which ones are in use.
#[async_trait]
pub trait InboundWebhook: Send + Sync {
    /// A stable, lowercase identifier for this provider (e.g. `"plivo"`,
    /// `"twilio"`, `"aws-sns"`).  Used as the lookup key in
    /// [`InboundRegistry`].
    fn provider(&self) -> &'static str;

    /// Parse the raw HTTP payload (headers + body) into a normalized
    /// [`InboundMessage`].
    fn parse_inbound(&self, headers: &Headers, body: &[u8]) -> Result<InboundMessage, SmsError>;

    /// Verify the cryptographic signature on the incoming request.
    ///
    /// The default implementation is a no-op (always succeeds).  Providers
    /// that support webhook signatures should override this.
    fn verify(&self, _headers: &Headers, _body: &[u8]) -> Result<(), SmsError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// InboundRegistry
// ---------------------------------------------------------------------------

/// A runtime registry that maps provider names to [`InboundWebhook`]
/// implementations.
///
/// Used by the generic webhook processor to look up the right handler at
/// request time without compile-time knowledge of which providers are
/// registered.
///
/// # Example
///
/// ```rust,ignore
/// use sms_core::InboundRegistry;
/// use std::sync::Arc;
///
/// let registry = InboundRegistry::new()
///     .with(Arc::new(plivo_client))
///     .with(Arc::new(sns_client));
///
/// // Later, in a request handler:
/// if let Some(hook) = registry.get("plivo") {
///     let msg = hook.parse_inbound(&headers, &body)?;
/// }
/// ```
#[derive(Default, Clone)]
pub struct InboundRegistry {
    map: Arc<HashMap<&'static str, Arc<dyn InboundWebhook>>>,
}

impl InboundRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            map: Arc::new(HashMap::new()),
        }
    }

    /// Register a provider.  The provider's [`InboundWebhook::provider()`]
    /// return value is used as the lookup key.
    pub fn with(mut self, hook: Arc<dyn InboundWebhook>) -> Self {
        let mut m = (*self.map).clone();
        m.insert(hook.provider(), hook);
        self.map = Arc::new(m);
        self
    }

    /// Look up a registered provider by name.
    pub fn get(&self, provider: &str) -> Option<Arc<dyn InboundWebhook>> {
        self.map.get(provider).cloned()
    }
}

// ---------------------------------------------------------------------------
// SmsRouter — unified dispatch by provider name
// ---------------------------------------------------------------------------

/// Routes SMS sends to a named provider without requiring the caller to know
/// about individual provider crate types.
///
/// This is the unified dispatch client that eliminates boilerplate in
/// consumer code.  Instead of matching on a provider enum and constructing
/// the right client, register each provider once and then call
/// [`send_via`](SmsRouter::send_via) with a name.
///
/// `SmsRouter` also implements [`SmsClient`] itself, forwarding to a
/// configured default provider.
///
/// # Example
///
/// ```rust,ignore
/// use sms_core::{SmsRouter, SendRequest};
///
/// let router = SmsRouter::new()
///     .with("plivo", plivo_client)
///     .with("aws-sns", sns_client)
///     .default_provider("plivo");
///
/// // Explicit dispatch:
/// router.send_via("aws-sns", SendRequest { .. }).await?;
///
/// // Or use the SmsClient impl (goes to the default):
/// router.send(SendRequest { .. }).await?;
/// ```
#[derive(Clone)]
pub struct SmsRouter {
    providers: Arc<HashMap<String, Arc<dyn SmsClient>>>,
    default: Option<String>,
}

impl SmsRouter {
    /// Create an empty router with no providers registered.
    pub fn new() -> Self {
        Self {
            providers: Arc::new(HashMap::new()),
            default: None,
        }
    }

    /// Register a provider under the given name.
    ///
    /// If this is the first provider added it automatically becomes the
    /// default (override with [`default_provider`](SmsRouter::default_provider)).
    pub fn with(mut self, name: impl Into<String>, client: impl SmsClient + 'static) -> Self {
        let name = name.into();
        let mut m = (*self.providers).clone();
        let first = m.is_empty();
        m.insert(name.clone(), Arc::new(client));
        self.providers = Arc::new(m);
        if first {
            self.default = Some(name);
        }
        self
    }

    /// Register a provider that is already behind an `Arc`.
    pub fn with_arc(mut self, name: impl Into<String>, client: Arc<dyn SmsClient>) -> Self {
        let name = name.into();
        let mut m = (*self.providers).clone();
        let first = m.is_empty();
        m.insert(name.clone(), client);
        self.providers = Arc::new(m);
        if first {
            self.default = Some(name);
        }
        self
    }

    /// Set which provider name is used when calling the [`SmsClient`] trait
    /// impl directly (i.e. `router.send(..)`).
    pub fn default_provider(mut self, name: impl Into<String>) -> Self {
        self.default = Some(name.into());
        self
    }

    /// Send a message through a specific named provider.
    pub async fn send_via(
        &self,
        provider: &str,
        req: SendRequest<'_>,
    ) -> Result<SendResponse, SmsError> {
        let client = self
            .providers
            .get(provider)
            .ok_or_else(|| SmsError::Invalid(format!("unknown provider: {}", provider)))?;
        client.send(req).await
    }

    /// Returns `true` if a provider with the given name is registered.
    pub fn has_provider(&self, name: &str) -> bool {
        self.providers.contains_key(name)
    }

    /// Returns the name of the current default provider, if any.
    pub fn default_provider_name(&self) -> Option<&str> {
        self.default.as_deref()
    }
}

impl Default for SmsRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SmsClient for SmsRouter {
    /// Send through the default provider.
    ///
    /// Returns [`SmsError::Invalid`] if no default has been set.
    async fn send(&self, req: SendRequest<'_>) -> Result<SendResponse, SmsError> {
        let name = self
            .default
            .as_deref()
            .ok_or_else(|| SmsError::Invalid("no default provider configured".into()))?;
        self.send_via(name, req).await
    }
}

// ---------------------------------------------------------------------------
// FallbackClient — try providers in order
// ---------------------------------------------------------------------------

/// An [`SmsClient`] that tries a list of providers in order, returning the
/// first successful response.
///
/// This is the pattern every consumer re-invents for primary / backup
/// failover.  `FallbackClient` encapsulates it once so you don't have to.
///
/// All errors from intermediate providers are collected; if every provider
/// fails, the **last** error is returned (with a summary of all failures in
/// the message).
///
/// # Example
///
/// ```rust,ignore
/// use sms_core::FallbackClient;
///
/// let client = FallbackClient::new(vec![
///     Arc::new(primary_client),
///     Arc::new(backup_client),
/// ]);
///
/// // Tries primary first; on failure, tries backup.
/// let response = client.send(SendRequest { .. }).await?;
/// ```
pub struct FallbackClient {
    providers: Vec<Arc<dyn SmsClient>>,
}

impl FallbackClient {
    /// Create a new fallback chain.
    ///
    /// Providers are tried in the order given.  The list must contain at
    /// least one provider.
    pub fn new(providers: Vec<Arc<dyn SmsClient>>) -> Self {
        assert!(!providers.is_empty(), "FallbackClient requires at least one provider");
        Self { providers }
    }

    /// Convenience builder that wraps each client in an `Arc` for you.
    pub fn from_clients(clients: Vec<Box<dyn SmsClient>>) -> Self {
        let providers = clients.into_iter().map(Arc::from).collect();
        Self { providers }
    }

    /// Returns how many providers are in the chain.
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    /// Returns `true` if the chain is empty (should never happen after `new`).
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

#[async_trait]
impl SmsClient for FallbackClient {
    /// Try each provider in order.  Returns the first success or, if all
    /// fail, an error summarizing every failure.
    async fn send(&self, req: SendRequest<'_>) -> Result<SendResponse, SmsError> {
        let mut errors: Vec<String> = Vec::new();

        for provider in &self.providers {
            match provider.send(req.clone()).await {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    errors.push(e.to_string());
                }
            }
        }

        // All providers failed — return a summary.
        Err(SmsError::Provider(format!(
            "all {} providers failed: [{}]",
            self.providers.len(),
            errors.join("; ")
        )))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- OwnedSendRequest tests --

    #[test]
    fn owned_send_request_new() {
        let req = OwnedSendRequest::new("+14155551234", "+10005551234", "Hello");
        assert_eq!(req.to, "+14155551234");
        assert_eq!(req.from, "+10005551234");
        assert_eq!(req.text, "Hello");
    }

    #[test]
    fn owned_send_request_from_string_values() {
        let to = String::from("+14155551234");
        let from = String::from("+10005551234");
        let text = String::from("Hello");
        let req = OwnedSendRequest::new(to, from, text);
        assert_eq!(req.to, "+14155551234");
    }

    #[test]
    fn owned_send_request_as_ref_roundtrip() {
        let owned = OwnedSendRequest::new("+1", "+2", "hi");
        let borrowed = owned.as_ref();
        assert_eq!(borrowed.to, "+1");
        assert_eq!(borrowed.from, "+2");
        assert_eq!(borrowed.text, "hi");
    }

    #[test]
    fn owned_send_request_from_send_request() {
        let borrowed = SendRequest {
            to: "+1",
            from: "+2",
            text: "msg",
        };
        let owned: OwnedSendRequest = borrowed.into();
        assert_eq!(owned.to, "+1");
        assert_eq!(owned.text, "msg");
    }

    #[test]
    fn send_request_from_owned_ref() {
        let owned = OwnedSendRequest::new("+1", "+2", "hi");
        let borrowed: SendRequest<'_> = (&owned).into();
        assert_eq!(borrowed.to, "+1");
    }

    #[test]
    fn owned_send_request_serde_roundtrip() {
        let req = OwnedSendRequest::new("+1", "+2", "test");
        let json = serde_json::to_string(&req).unwrap();
        let deser: OwnedSendRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, deser);
    }

    // -- HttpStatus tests --

    #[test]
    fn http_status_values() {
        assert_eq!(HttpStatus::Ok.as_u16(), 200);
        assert_eq!(HttpStatus::BadRequest.as_u16(), 400);
        assert_eq!(HttpStatus::Unauthorized.as_u16(), 401);
        assert_eq!(HttpStatus::NotFound.as_u16(), 404);
        assert_eq!(HttpStatus::InternalServerError.as_u16(), 500);
    }

    // -- WebhookResponse tests --

    #[test]
    fn webhook_response_success_serializes_message() {
        let msg = InboundMessage {
            id: Some("msg-1".into()),
            from: "+1111".into(),
            to: "+2222".into(),
            text: "hi".into(),
            timestamp: None,
            provider: "test",
            raw: serde_json::json!({}),
        };
        let resp = WebhookResponse::success(msg);
        assert_eq!(resp.status, HttpStatus::Ok);
        assert!(resp.body.contains("msg-1"));
        assert_eq!(resp.content_type, "application/json");
    }

    #[test]
    fn webhook_response_error_escapes_quotes() {
        let resp = WebhookResponse::error(HttpStatus::BadRequest, r#"bad "input""#);
        assert!(resp.body.contains(r#"bad \"input\""#));
    }

    // -- InboundRegistry tests --

    #[test]
    fn inbound_registry_get_returns_none_for_unknown() {
        let reg = InboundRegistry::new();
        assert!(reg.get("nonexistent").is_none());
    }

    // -- SmsError display --

    #[test]
    fn sms_error_display() {
        let e = SmsError::Http("timeout".into());
        assert_eq!(e.to_string(), "http error: timeout");

        let e = SmsError::Auth("bad token".into());
        assert_eq!(e.to_string(), "authentication error: bad token");
    }

    // -- WebhookError from SmsError --

    #[test]
    fn webhook_error_from_sms_error() {
        let sms_err = SmsError::Provider("oops".into());
        let wh_err: WebhookError = sms_err.into();
        assert!(wh_err.to_string().contains("oops"));
    }

    // -- fallback_id --

    #[test]
    fn fallback_id_is_valid_uuid() {
        let id = fallback_id();
        assert!(uuid::Uuid::parse_str(&id).is_ok());
    }

    // -- SmsRouter tests --

    /// A mock client that always succeeds.
    struct MockClient {
        provider_name: &'static str,
    }

    #[async_trait]
    impl SmsClient for MockClient {
        async fn send(&self, _req: SendRequest<'_>) -> Result<SendResponse, SmsError> {
            Ok(SendResponse {
                id: "mock-id".into(),
                provider: self.provider_name,
                raw: serde_json::json!({"mock": true}),
            })
        }
    }

    /// A mock client that always fails.
    struct FailingClient {
        message: String,
    }

    #[async_trait]
    impl SmsClient for FailingClient {
        async fn send(&self, _req: SendRequest<'_>) -> Result<SendResponse, SmsError> {
            Err(SmsError::Provider(self.message.clone()))
        }
    }

    fn test_request() -> SendRequest<'static> {
        SendRequest {
            to: "+14155551234",
            from: "+10005551234",
            text: "test",
        }
    }

    #[tokio::test]
    async fn router_send_via_dispatches_correctly() {
        let router = SmsRouter::new()
            .with("alpha", MockClient { provider_name: "alpha" })
            .with("beta", MockClient { provider_name: "beta" });

        let resp = router.send_via("beta", test_request()).await.unwrap();
        assert_eq!(resp.provider, "beta");
    }

    #[tokio::test]
    async fn router_send_via_unknown_provider_errors() {
        let router = SmsRouter::new()
            .with("alpha", MockClient { provider_name: "alpha" });

        let err = router.send_via("nope", test_request()).await.unwrap_err();
        assert!(err.to_string().contains("unknown provider"));
    }

    #[tokio::test]
    async fn router_default_is_first_registered() {
        let router = SmsRouter::new()
            .with("first", MockClient { provider_name: "first" })
            .with("second", MockClient { provider_name: "second" });

        assert_eq!(router.default_provider_name(), Some("first"));
        let resp = router.send(test_request()).await.unwrap();
        assert_eq!(resp.provider, "first");
    }

    #[tokio::test]
    async fn router_explicit_default_override() {
        let router = SmsRouter::new()
            .with("first", MockClient { provider_name: "first" })
            .with("second", MockClient { provider_name: "second" })
            .default_provider("second");

        let resp = router.send(test_request()).await.unwrap();
        assert_eq!(resp.provider, "second");
    }

    #[tokio::test]
    async fn router_no_default_errors() {
        let router = SmsRouter::new();
        let err = router.send(test_request()).await.unwrap_err();
        assert!(err.to_string().contains("no default provider"));
    }

    #[test]
    fn router_has_provider() {
        let router = SmsRouter::new()
            .with("plivo", MockClient { provider_name: "plivo" });
        assert!(router.has_provider("plivo"));
        assert!(!router.has_provider("twilio"));
    }

    // -- FallbackClient tests --

    #[tokio::test]
    async fn fallback_returns_first_success() {
        let client = FallbackClient::new(vec![
            Arc::new(MockClient { provider_name: "primary" }),
            Arc::new(MockClient { provider_name: "backup" }),
        ]);
        let resp = client.send(test_request()).await.unwrap();
        assert_eq!(resp.provider, "primary");
    }

    #[tokio::test]
    async fn fallback_skips_failing_provider() {
        let client = FallbackClient::new(vec![
            Arc::new(FailingClient { message: "down".into() }),
            Arc::new(MockClient { provider_name: "backup" }),
        ]);
        let resp = client.send(test_request()).await.unwrap();
        assert_eq!(resp.provider, "backup");
    }

    #[tokio::test]
    async fn fallback_all_fail_returns_summary() {
        let client = FallbackClient::new(vec![
            Arc::new(FailingClient { message: "err-a".into() }),
            Arc::new(FailingClient { message: "err-b".into() }),
        ]);
        let err = client.send(test_request()).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("all 2 providers failed"));
        assert!(msg.contains("err-a"));
        assert!(msg.contains("err-b"));
    }

    #[test]
    fn fallback_len() {
        let client = FallbackClient::new(vec![
            Arc::new(MockClient { provider_name: "a" }),
            Arc::new(MockClient { provider_name: "b" }),
        ]);
        assert_eq!(client.len(), 2);
        assert!(!client.is_empty());
    }

    #[test]
    #[should_panic(expected = "at least one provider")]
    fn fallback_empty_panics() {
        FallbackClient::new(vec![]);
    }
}
