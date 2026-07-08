# Strategy: Scotia Daemon & Nova Scotia Notifications

## Goals

1. **Zero-friction global wrapping**: Typing `kimi`, `claude`, `codex`, `opencode`, etc. automatically records a Scotia run without the user prefixing commands or changing workflow.
2. **Persistent daemon presence**: A background `scotiad` owns notification state, recent-run history, shim registration, and cross-session configuration.
3. **Icy Nova Scotia theming**: All user-facing notifications, TUI chrome, and log summaries use a consistent maritime/arctic Nova Scotia motif (icebergs, fog, lighthouses, nor'easters, the Bluenose).

## Current state

Scotia is a per-invocation CLI:

```bash
scotia run --agent claude-code --task "refactor auth" -- claude
```

It spawns the agent as a child process, tees `stdout`/`stderr`/`stdin` through agent-specific interceptors, normalizes events, synthesizes a summary, and writes artifacts to `scotia-log/YYYY-MM-DD/`.

The wrapping logic is already modular (`wrapper::run_and_capture`, `interceptor::AgentInterceptor`, `storage::store_run`). The daemon strategy reuses those modules and adds a long-lived control plane.

## Architecture overview

```text
┌─────────────────────────────────────────────────────────────────────┐
│                          User shell / desktop                       │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                 │
│  │  kimi shim  │  │ claude shim │  │  codex shim │  ...             │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘                 │
│         │                │                │                         │
│         └────────────────┴────────────────┘                         │
│                          │                                          │
│                   Unix socket / IPC                                │
│                          │                                          │
├──────────────────────────┼──────────────────────────────────────────┤
│                          ▼                                          │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │                          scotiad                              │  │
│  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────────────┐  │  │
│  │  │ IPC server   │ │ Notification │ │   Session registry   │  │  │
│  │  │ (tokio UDS)  │ │   engine     │ │  (recent runs, TUI)  │  │  │
│  │  └──────┬───────┘ └──────┬───────┘ └──────────┬───────────┘  │  │
│  │         │                │                    │              │  │
│  │         ▼                ▼                    ▼              │  │
│  │  ┌────────────────────────────────────────────────────────┐  │  │
│  │  │              wrapper::run_and_capture                    │  │  │
│  │  │         (existing tee + interceptor logic)               │  │  │
│  │  └────────────────────────┬───────────────────────────────┘  │  │
│  │                           │                                   │  │
│  │                           ▼                                   │  │
│  │  ┌────────────────────────────────────────────────────────┐  │  │
│  │  │              storage::store_run                          │  │  │
│  │  └────────────────────────┬───────────────────────────────┘  │  │
│  └───────────────────────────┼───────────────────────────────────┘  │
│                              │                                       │
│                              ▼                                       │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │  Desktop notifications (notify-rust / macOS NotificationCtr)  │   │
│  └──────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
```

## 1. Daemon (`scotiad`)

### Responsibilities

- Accept wrap requests from shims over a Unix domain socket.
- Spawn the real agent with `wrapper::run_and_capture`.
- Stream captured stdio back to the shim via the same socket (or hand back a PTY fd).
- Dispatch Nova Scotia themed notifications at run start/finish/error.
- Maintain an in-memory session registry for the TUI (`scotia` with no args).
- Watch `~/.config/scotia/daemon.toml` for hot config reloads.
- (Optional) Expose a tiny HTTP endpoint for the `scotia.tech` dashboard.

### Lifecycle

Managed as a user service:

- **Linux**: systemd `--user` unit `scotiad.service`
- **macOS**: `launchd` plist `com.scotia.scotiad`
- **Manual**: `scotia daemon start|stop|status|logs`

### IPC protocol (initial)

Unix domain socket at `~/.local/share/scotia/scotiad.sock`.

```rust
// Request from shim
struct WrapRequest {
    agent: AgentKind,
    task: Option<String>,
    cwd: PathBuf,
    argv: Vec<String>,
    env: HashMap<String, String>,
}

// Response: either child PID + stdio handles, or error
```

For the first iteration the shim can simply `exec` into `scotia run` while the daemon handles notifications and session tracking. The second iteration moves stdio transport into the daemon so the daemon can emit notifications without the CLI blocking.

## 2. Global wrapping shims

### Approach A: PATH shims (recommended)

A directory `~/.local/share/scotia/shims` contains small executables named after supported agents:

```text
~/.local/share/scotia/shims/
├── kimi -> scotia-shim
├── kimi-code -> scotia-shim
├── claude -> scotia-shim
├── claude-code -> scotia-shim
├── codex -> scotia-shim
├── opencode -> scotia-shim
└── agy -> scotia-shim
```

`scotia-shim` is a tiny Rust binary (or shell script) that:

1. Reads `argv[0]` to determine `AgentKind`.
2. Resolves the real binary via a config map (e.g. `claude` -> `/usr/local/bin/claude`).
3. Either:
   - **Phase 1**: execs `scotia run --agent <agent> -- <real-binary> "$@"`
   - **Phase 2**: sends a `WrapRequest` to `scotiad` and bridges stdio.

Installation:

```bash
scotia install-shims
```

This prepends `~/.local/share/scotia/shims` to the user's shell PATH (`.bashrc`, `.zshrc`, fish config) or writes wrapper functions for shells that cache PATH lookups.

### Approach B: Desktop file overrides

For GUI launcher usage, Scotia can rewrite `.desktop` files in `~/.local/share/applications` to prefix the `Exec=` line with the shim. This is lower priority than shell shims.

### Fallback for unknown agents

If a binary name is not in the supported list, the shim passes through unchanged. Users can add custom mappings in `~/.config/scotia/agents.toml`:

```toml
[agents.myagent]
binary = "myagent"
kind = "unknown"
interceptor = "generic"
```

## 3. Icy Nova Scotia notification system

### Visual identity

- **Palette**: iceberg white `#E8F4F8`, fog grey `#B8C5D0`, Atlantic teal `#2E6B7A`, lighthouse red `#C94C4C`, storm navy `#1A2E3B`, pack ice blue `#A8D0E6`.
- **Icons**: snowflake, lighthouse, schooner, iceberg, compass rose.
- **Typography**: crisp sans-serif with occasional nautical terms in body copy.

### Notification severity map

| Level | Maritime term | When |
|-------|---------------|------|
| `info` | Light flurries | Background state updates (daemon started, config reloaded) |
| `success` | Harbour clear | Run finished cleanly |
| `warning` | Nor'easter warning | Run finished with errors or retries |
| `error` | Mayday / Shipwreck | Agent crashed or Scotia failed to wrap |
| `progress` | In the ice field | Long-running run still active (throttled) |

### Notification templates

- **Run started**: "*Casting off from `<cwd>` — expect light flurries.*"
- **Run finished (success)**: "*Returned to port. `<n>` actions logged, `<m>` models routed.*"
- **Run finished (with errors)**: "*Nor'easter off Cape Breton — `<k>` errors, `<r>` retries.*"
- **Crash**: "*Mayday. `<agent>` went down in heavy seas.*"
- **Long-running**: "*Still in the ice field... `<agent>` has been underway for `<duration>`.*"

### Backends

- **Linux**: `notify-rust` (D-Bus `org.freedesktop.Notifications`).
- **macOS**: `notify` crate using `UNUserNotificationCenter`.
- **Optional**: OSC 777 terminal hyperlinks + audible bell for headless environments.

### Sound (opt-in)

| Event | Sound cue |
|-------|-----------|
| Start | Iceberg calving (low crackle) |
| Success | Foghorn single blast |
| Warning | Wind gust through rigging |
| Error | Distress horn + wave crash |
| Long-running | Creaking ship hull (very sparse) |

## 4. Configuration

`~/.config/scotia/daemon.toml`:

```toml
[daemon]
socket_path = "~/.local/share/scotia/scotiad.sock"
log_root = "~/scotia-log"

[notifications]
enabled = true
sounds = false
throttle_seconds = 30
progress_after_seconds = 60

[notifications.theme]
name = "nova-scotia"
icon_set = "nautical"

[shims]
install_path = "~/.local/share/scotia/shims"
agents = ["kimi", "claude", "codex", "opencode", "agy", "cosine"]

[[shims.override]]
name = "claude-code"
real_binary = "/opt/homebrew/bin/claude"
```

## 5. CLI additions

```bash
scotia daemon start      # start scotiad
scotia daemon stop       # stop scotiad
scotia daemon status     # show PID, uptime, wrapped runs today
scotia daemon logs       # tail daemon logs
scotia install-shims     # create shims and update shell PATH
scotia uninstall-shims   # remove shims and PATH entries
scotia notify test       # send a test notification
```

## 6. Implementation phases

### Phase 1 — Shim + CLI wrapping (no daemon)

- Add `scotia-shim` crate.
- Implement `scotia install-shims` / `uninstall-shims`.
- Shims exec `scotia run --agent <agent> -- <real-binary>`. Notifications are emitted by the CLI process directly via `notify-rust`.
- Goal: zero-friction wrapping today; daemon can come later.

### Phase 2 — Daemon control plane

- Add `scotiad` binary with UDS IPC.
- Move notification dispatch and session registry into daemon.
- Shims talk to daemon; CLI can still work standalone when daemon is absent.
- Add systemd/launchd service files.

### Phase 3 — Rich notifications & TUI integration

- Implement themed notification templates and progress throttling.
- Wire the TUI (`scotia` with no args) to the daemon's session registry so it shows live runs.
- Add sound pack scaffolding.

### Phase 4 — Desktop / GUI integration

- `.desktop` file overrides.
- Optional tray icon: a tiny animated lighthouse that glows when a run is active.

## 7. Security & UX considerations

- **PATH order**: shims must appear before real binaries. Detect collisions and warn if a real binary is earlier in PATH.
- **Recursive wrapping**: shims must recognize when they are already inside a Scotia wrapper and pass through to avoid double-counting.
- **Shell aliases**: some users alias `kimi` to something else. `install-shims` should detect common aliases and ask whether to override.
- **Sensitive data**: notifications must not include prompt content by default; only action counts and agent name.
- **Daemon crash**: CLI fallback must always work so the user is never locked out of their agent.

## 8. Open questions

1. Should the daemon own the PTY allocation so interactive agents (Claude Code, Kimi) retain full terminal control?
2. Should notifications be batched into a single tray menu, or emitted per-run?
3. How should custom user interceptors be loaded by the daemon (dynamic library, WASM, or config-only)?
4. Do we ship the Nova Scotia sound pack, or keep it opt-in due to file size?
