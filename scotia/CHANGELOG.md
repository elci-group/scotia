# Changelog

All notable changes to Scotia are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Cross-platform GUI installers: Calamares module for Linux, NSIS installer for Windows, and pkg/DMG installer for macOS, each with per-user or system-wide autostart options.
- `scotia installer apply` command for scripted installs from GUI installers.
- Windows service and HKCU Run registration in the shared installer module.
- Calamares QML view module (`scotia_ui`) plus Python job module (`scotia`) for Linux.
- `scotiad` daemon for session registry and themed notification dispatch.
- Unix-domain-socket IPC between CLI and daemon.
- `scotia-shim` binary and `scotia install-shims` / `uninstall-shims` commands for zero-friction agent wrapping.
- Nova Scotia themed notification system with desktop and terminal backends.
- `scotia notify test` command.
- `scotia daemon start|stop|status|logs` commands.
- systemd and launchd service files under `deploy/`.
- `LICENSE`, `CONTRIBUTING.md`, and this `CHANGELOG.md`.
- GitHub Actions CI workflow for build, test, clippy, and formatting.

### Changed

- `WrapperConfig` now accepts an optional `run_id` so the CLI and daemon share the same run identifier.
- `ScotiaRun::new` accepts an optional `run_id`.

## [0.1.1] - 2026-07-07

### Added

- Initial Rust CLI for wrapping agent processes.
- Adapters for Kimi Code, agy, cosine, Codex, Claude Code, and opencode.
- Event normalization, synthesis, and Graphviz action graphs.
- Filesystem storage under `scotia-log/`.
- Optional Git commits of run artifacts.
- TUI harness selector.
