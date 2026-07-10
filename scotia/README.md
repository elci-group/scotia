# Scotia

A Rust-based **Semantic Decision Ledger (SDL)** for agentic systems.

Scotia does **not** intercept hidden chain-of-thought. Instead, it observes the
structured telemetry that agents already emit—prompts, tool calls, model
routing decisions, responses, errors, and state changes—and stores them as a
Git-native audit log with post-hoc synthesis.

## Supported agents

- Kimi Code
- agy
- cosine
- Codex
- Claude Code
- opencode

## Installation

### From source

```bash
cargo build --release
```

The build produces three binaries in `target/release/`:
- `scotia` — CLI and TUI
- `scotiad` — daemon
- `scotia-shim` — agent wrapper shim

### GUI installers

Pre-built GUI installers are provided under `installer/`:

- **Linux** — Calamares module in `installer/linux/calamares/`. Copy or package the
  `calamares` directory, build the `scotia_ui` QML view module (see
  `installer/linux/calamares/modules/scotia_ui/CMakeLists.txt`), and run
  `calamares -d /path/to/calamares`. The installer lets you pick per-user or
  system-wide scope, autostart, and PATH shims.
- **Windows** — NSIS script in `installer/windows/scotia.nsi`. Copy the release
  binaries to `installer/windows/bin/` and run `makensis scotia.nsi` to produce
  `Scotia-Setup.exe`.
- **macOS** — `installer/macos/build-pkg.sh` builds a signed/unsigned `.pkg` and
  wraps it in a `.dmg`. Run it from the repository root after building the
  release binaries.

### Scripted install

GUI installers call the same underlying command:

```bash
scotia installer apply --scope user|system --autostart --install-shims --bin-dir /path/to/bin
```

## Usage

### Wrap an agent

```bash
scotia run --agent claude-code --task "refactor auth" -- claude
```

Scotia intercepts stdout/stderr, parses tool calls and routing hints, and
writes three artifacts under `scotia-log/YYYY-MM-DD/`:

- `run_<id>.json` — canonical event stream
- `run_<id>.summary.md` — human-readable synthesis
- `run_<id>.dot` — action graph for Graphviz

### List stored runs

```bash
scotia list
```

### Replay a run

```bash
scotia replay scotia-log/2026-07-03/run_<id>.json
```

### Print a summary

```bash
scotia summary scotia-log/2026-07-03/run_<id>.json
```

### Commit artifacts to Git

```bash
scotia --git-commit run --agent claude-code -- claude
```

### Zero-friction shims

Install shims so typing `kimi`, `claude`, `codex`, etc. automatically records a Scotia run:

```bash
scotia install-shims
# restart your shell or `source ~/.bashrc` / `source ~/.zshrc`
```

This creates symlinks in `~/.local/share/scotia/shims` and prepends that directory to your shell PATH.

To remove:

```bash
scotia uninstall-shims
```

### Nova Scotia notifications

Scotia surfaces desktop notifications with an icy Nova Scotia theme:

- **Light flurries** — background updates, run started
- **Harbour clear** — run finished cleanly
- **Nor'easter warning** — errors or retries detected
- **Mayday** — agent crashed

Disable with `--no-notify`:

```bash
scotia --no-notify run --agent claude-code -- claude
```

Test all notification levels:

```bash
scotia notify test
```

## Architecture

- `adapter` — trait and registry for agent-specific parsers
- `adapters` — parsers for Kimi, agy, cosine, codex, Claude Code, opencode
- `wrapper` — stdio process wrapper that tees agent streams
- `normalizer` — sorts, deduplicates, and coalesces raw events
- `synthesizer` — generates post-hoc rationales, trade-offs, and DOT graphs
- `storage` — filesystem persistence
- `git` — optional deterministic commits
- `cli` — `clap`-based command line interface
- `notify` — Nova Scotia themed desktop/terminal notifications
- `shim` — PATH shim installation and shell PATH management

## Event algebra

The canonical event schema is intentionally small:

- `run_started`
- `prompt_submitted`
- `action_invoked`
- `action_result`
- `model_routed`
- `response_chunk`
- `error_or_retry`
- `state_delta`
- `run_finished`

These events reconstruct agent behaviour as state transitions rather than
private reasoning.

## Development

```bash
cargo test
```
