# Contributing to Scotia

Thanks for helping make Scotia better. This document covers how to get started and what we expect from contributions.

## Getting started

1. Clone the repository.
2. Install the Rust toolchain (stable channel).
3. Build the project:

   ```bash
   cargo build --release
   ```

4. Run the test suite:

   ```bash
   cargo test
   ```

## Project structure

- `src/bin/scotia.rs` — main CLI entrypoint.
- `src/bin/scotia-shim.rs` — PATH shim that forwards agent invocations to the CLI.
- `src/bin/scotiad.rs` — background daemon (session registry + notifications).
- `src/wrapper.rs` — spawns an agent and tees its stdio through interceptors.
- `src/interceptor.rs` + `src/interceptors/` — agent-specific telemetry parsers.
- `src/storage.rs`, `src/normalizer.rs`, `src/synthesizer.rs` — run persistence and summary generation.
- `src/notify.rs`, `src/shim.rs`, `src/daemon.rs`, `src/ipc.rs` — zero-friction wrapping and notification infrastructure.
- `docs/strategy/` — design documents.
- `deploy/` — systemd and launchd service files.

## Code quality

All changes must pass:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The CI pipeline enforces these checks.

## Adding a new agent interceptor

1. Add the agent variant to `src/event.rs` (`AgentKind`).
2. Create `src/interceptors/<agent>.rs` implementing `AgentInterceptor`.
3. Register it in `src/interceptor.rs`.
4. Add a binary-name mapping in `src/shim.rs` (`agent_kind_for_name`) and `src/bin/scotia-shim.rs`.
5. Add tests in `tests/interceptor_<agent>_test.rs`.

## Submitting changes

1. Open a pull request against `master`.
2. Describe the change and the motivation.
3. Ensure CI is green.
4. Request review from a maintainer.

## Code of conduct

Be respectful, constructive, and inclusive. Harassment or hostile behavior will not be tolerated.
