use sms_core::*;
use sms_web_generic::WebhookProcessor;

#[tokio::test]
async fn test_webhook_processor_unknown_provider() {
    // Create a test registry without any providers
    let registry = InboundRegistry::new();
    let processor = WebhookProcessor::new(registry);

    let headers: Headers = vec![];
    let response = processor.process_webhook("unknown-provider", headers, b"test payload");

    // Should return 404 for unknown provider
    assert_eq!(response.status.as_u16(), 404);
    assert_eq!(response.content_type, "application/json");
    assert!(response.body.contains("unknown provider"));
}

#[tokio::test]
async fn test_webhook_processor_empty_payload() {
    let registry = InboundRegistry::new();
    let processor = WebhookProcessor::new(registry);

    let headers: Headers = vec![("content-type".to_string(), "application/json".to_string())];
    let response = processor.process_webhook("test-provider", headers, b"");

    // Should return 404 for unknown provider
    assert_eq!(response.status.as_u16(), 404);
    assert!(response.body.contains("unknown provider"));
}

#[tokio::test]
async fn test_webhook_processor_large_payload() {
    let registry = InboundRegistry::new();
    let processor = WebhookProcessor::new(registry);

    // Create a large payload
    let large_payload = "x".repeat(10000);
    let headers: Headers = vec![("content-type".to_string(), "application/json".to_string())];

    let response = processor.process_webhook("test-provider", headers, large_payload.as_bytes());

    // Should handle gracefully (return 404 since provider not registered)
    assert_eq!(response.status.as_u16(), 404);
    assert!(response.body.contains("unknown provider"));
}

#[tokio::test]
async fn test_webhook_processor_concurrent_requests() {
    use futures::future;

    let registry = InboundRegistry::new();
    let processor = WebhookProcessor::new(registry);

    // Create multiple concurrent webhook requests
    let futures = (0..10).map(|i| {
        let processor_clone = processor.clone();
        let payload = format!("test payload {}", i);
        let headers: Headers = vec![("content-type".to_string(), "application/json".to_string())];

        async move { processor_clone.process_webhook("test-provider", headers, payload.as_bytes()) }
    });

    let responses = future::join_all(futures).await;

    // All requests should complete successfully (with 404 for unknown provider)
    assert_eq!(responses.len(), 10);
    for response in responses {
        assert_eq!(response.status.as_u16(), 404);
        assert!(response.body.contains("unknown provider"));
    }
}

#[tokio::test]
async fn test_webhook_processor_with_headers() {
    let registry = InboundRegistry::new();
    let processor = WebhookProcessor::new(registry);

    // Test with various header combinations
    let headers_with_auth: Headers = vec![
        ("content-type".to_string(), "application/json".to_string()),
        ("authorization".to_string(), "Bearer test-token".to_string()),
        ("x-custom-header".to_string(), "custom-value".to_string()),
    ];

    let response = processor.process_webhook("test", headers_with_auth, b"{}");
    assert_eq!(response.status.as_u16(), 404);
}

#[tokio::test]
async fn test_webhook_processor_edge_cases() {
    let registry = InboundRegistry::new();
    let processor = WebhookProcessor::new(registry);

    // Empty provider name
    let response1 = processor.process_webhook("", vec![], b"test");
    assert_eq!(response1.status.as_u16(), 404);

    // Very long provider name
    let long_provider = "a".repeat(1000);
    let response2 = processor.process_webhook(&long_provider, vec![], b"test");
    assert_eq!(response2.status.as_u16(), 404);

    // Null bytes in payload
    let null_payload = b"test\x00payload\x00with\x00nulls";
    let response3 = processor.process_webhook("test", vec![], null_payload);
    assert_eq!(response3.status.as_u16(), 404);

    // Unicode in headers
    let unicode_headers: Headers = vec![
        ("x-unicode-header".to_string(), "测试数据".to_string()),
        (
            "content-type".to_string(),
            "application/json; charset=utf-8".to_string(),
        ),
    ];
    let response4 = processor.process_webhook("test", unicode_headers, "测试".as_bytes());
    assert_eq!(response4.status.as_u16(), 404);
}

#[tokio::test]
async fn test_high_throughput_webhook_processing() {
    use futures::future;

    let registry = InboundRegistry::new();
    let processor = WebhookProcessor::new(registry);

    // Simulate 50 concurrent requests (reduced for CI stability)
    let futures = (0..50).map(|i| {
        let processor_clone = processor.clone();
        let provider = if i % 3 == 0 {
            "plivo"
        } else if i % 3 == 1 {
            "twilio"
        } else {
            "test"
        };
        let payload = format!(r#"{{ "test": "payload", "index": {} }}"#, i);
        let headers: Headers = vec![
            ("content-type".to_string(), "application/json".to_string()),
            ("x-request-id".to_string(), format!("req-{}", i)),
        ];

        async move {
            let start = std::time::Instant::now();
            let response = processor_clone.process_webhook(provider, headers, payload.as_bytes());
            let duration = start.elapsed();
            (response, duration)
        }
    });

    let results = future::join_all(futures).await;

    // Verify all requests completed
    assert_eq!(results.len(), 50);

    // All should return 404 (unknown provider) but complete successfully
    for (response, duration) in results {
        assert_eq!(response.status.as_u16(), 404);
        // Performance check - each request should complete quickly (relaxed for CI)
        assert!(
            duration.as_millis() < 500,
            "Request took too long: {:?}",
            duration
        );
    }
}
