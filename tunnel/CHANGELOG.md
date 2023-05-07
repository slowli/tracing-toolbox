# Changelog

All notable changes to this project will be documented in this file.
The project adheres to [Semantic Versioning](http://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Expose `TracingEvent::normalize()` to transform a sequence of events so that
  it does not contain information that changes between program runs (e.g., metadata IDs)
  or due to minor refactoring (source code lines).

### Changed

- Bump minimum supported Rust version to 1.64.

## 0.1.0 - 2022-12-09

The initial release of `tracing-tunnel`.
