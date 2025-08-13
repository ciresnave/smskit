# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2024-12-30

### Added

- **Framework-Agnostic Architecture**: Complete redesign with universal web framework support
- **Multi-Framework Support**: Added adapters for 7+ web frameworks:
  - Axum (`sms-web-axum`)
  - Warp (`sms-web-warp`)
  - Actix-web (`sms-web-actix`)
  - Rocket (`sms-web-rocket`)
  - Tide (`sms-web-tide`)
  - Hyper (`sms-web-hyper`)
  - Poem (`sms-web-poem`)
  - Generic integration (`sms-web-generic`) for any framework
- **Enhanced Core Types**: Added `WebhookError`, `HttpStatus`, `WebhookResponse`
- **Comprehensive Examples**: Framework-specific examples for all supported frameworks
- **Production Documentation**: Enhanced README with architecture diagrams and usage examples

### Changed

- **Breaking**: Redesigned architecture with trait-based framework adapters
- **Breaking**: Enhanced webhook processing with structured error handling
- **Breaking**: Updated core types and interfaces for better ergonomics

### Fixed

- Resolved all clippy warnings across codebase
- Fixed move semantics issues in Plivo provider
- Improved error handling and type safety

## [0.1.0] - 2024-12-29

### Added

- Initial release with basic SMS functionality
- Plivo provider implementation
- Core traits and types
- Basic Axum integration
- Send SMS functionality
- Webhook processing foundation

[0.2.0]: https://github.com/yourusername/smskit/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/yourusername/smskit/releases/tag/v0.1.0
