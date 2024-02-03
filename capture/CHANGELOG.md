# Changelog

All notable changes to this project will be documented in this file.
The project adheres to [Semantic Versioning](http://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Update `predicates` dependency.
- Bump minimum supported Rust version to 1.64.

### Fixed

- Fix `CapturedSpan::deep_scan_events()`. Previously, the scanner returned by this method
  did not take events directly tied to the targeted span into account.

## 0.1.0 - 2022-12-09

The initial release of `tracing-capture`.
