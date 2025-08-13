use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use sms_core::*;
use sms_web_generic::WebhookProcessor;
use smskit::rate_limiter::{RateLimitConfig, RateLimiter};
use std::collections::HashMap;
use tokio::runtime::Runtime;

fn benchmark_webhook_processing(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let registry = InboundRegistry::new();
    let processor = WebhookProcessor::new(registry);

    let payload_sizes = vec![100, 1000, 10000];
    let mut group = c.benchmark_group("webhook_processing");

    for size in payload_sizes {
        let payload = "x".repeat(size);
        let headers: Headers = vec![("content-type".to_string(), "application/json".to_string())];

        group.bench_with_input(
            BenchmarkId::new("process_webhook", size),
            &size,
            |b, &_size| {
                b.to_async(&rt).iter(|| async {
                    black_box(processor.process_webhook(
                        "test-provider",
                        headers.clone(),
                        payload.as_bytes(),
                    ))
                })
            },
        );
    }
    group.finish();
}

fn benchmark_rate_limiting(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let config = RateLimitConfig {
        max_requests: 1000,
        window_seconds: 60,
        enabled: true,
        per_provider: HashMap::new(),
    };
    let limiter = RateLimiter::new(config);

    let mut group = c.benchmark_group("rate_limiting");

    group.bench_function("single_key_check", |b| {
        b.to_async(&rt)
            .iter(|| async { black_box(limiter.check_rate_limit("test-key").await) })
    });

    group.bench_function("multiple_keys_check", |b| {
        b.to_async(&rt).iter(|| async {
            for i in 0..10 {
                black_box(limiter.check_rate_limit(&format!("test-key-{}", i)).await);
            }
        })
    });

    group.finish();
}

fn benchmark_memory_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_usage");

    group.bench_function("registry_creation", |b| {
        b.iter(|| black_box(InboundRegistry::new()))
    });

    group.bench_function("processor_creation", |b| {
        b.iter(|| {
            let registry = InboundRegistry::new();
            black_box(WebhookProcessor::new(registry))
        })
    });

    group.bench_function("rate_limiter_creation", |b| {
        b.iter(|| {
            let config = RateLimitConfig::default();
            black_box(RateLimiter::new(config))
        })
    });

    group.finish();
}

fn benchmark_configuration_loading(c: &mut Criterion) {
    use smskit::config::AppConfig;

    let mut group = c.benchmark_group("configuration");

    group.bench_function("create_default", |b| {
        b.iter(|| black_box(AppConfig::default()))
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_webhook_processing,
    benchmark_rate_limiting,
    benchmark_memory_usage,
    benchmark_configuration_loading
);

criterion_main!(benches);
