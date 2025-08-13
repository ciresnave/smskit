# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.2.x   | :white_check_mark: |
| 0.1.x   | :x:                |

## Reporting a Vulnerability

If you discover a security vulnerability within smskit, please send an email to <security@yourproject.com>. All security vulnerabilities will be promptly addressed.

Please do not report security vulnerabilities through public GitHub issues.

## Security Considerations

### Webhook Security

- Always validate webhook signatures from SMS providers
- Use HTTPS endpoints for webhook URLs
- Implement rate limiting for webhook endpoints
- Validate and sanitize all incoming webhook data

### API Security

- Store SMS provider credentials securely (use environment variables)
- Use TLS for all API communications
- Implement proper error handling to avoid information leakage
- Consider implementing API rate limiting

## Known Security Issues

Current security warnings from dependencies:

- Several unmaintained transitive dependencies via Tide framework
- These do not affect core SMS functionality but should be monitored

For the latest security status, run:

```bash
cargo audit
```
