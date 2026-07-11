# Scotia Security

Scotia is a semantic decision ledger that wraps and observes agent processes. A
shim or wrapper launches an agent (for example `claude`, `codex`, `kimi`),
intercepts its stdout/stderr, and records structured events; an optional local
daemon (`scotiad`) aggregates runs over a per-user control socket. This document
describes the security boundaries the code actually enforces, the platforms those
boundaries are validated on, and how to report a vulnerability. It is a statement
of current posture, not a guarantee of absence of defects.

## Security model

### Runtime layout (unix)

On unix targets the daemon's runtime directory, its control socket, and the
optional handshake token are created owner-only and re-asserted on reuse:

- The runtime directory is created mode `0700` (`rwx------`).
- The control socket node is `0600` (`rw-------`).
- The optional IPC handshake token is written `0600`.

These modes are produced by shared helpers in `src/runtime.rs`
(`ensure_private_dir` for the directory, `set_owner_only` for the socket node and
token) so the code that creates the layout and the code that verifies it cannot
drift apart. `ensure_private_dir` re-asserts the mode even when the directory
already exists, so a previously-loose directory cannot be reused to expose the
socket. The `scotia doctor` command (`src/cli/doctor.rs`) re-verifies all three
modes and reports a hard failure if the runtime directory is not `0700` or the
socket/token are not `0600`. On non-unix targets these helpers are explicit
no-ops; see Platform support.

### IPC authentication

The control socket is a unix socket whose node is owner-only (`0600`), so by
default only processes running as the same uid can connect. Because the socket
is already restricted to the owning user, token authentication is off by
default.

When the daemon is started with `--require-token`, a client must present the
handshake token (the `Auth` request) before any other request is served. The
token lives next to the socket at `<runtime-dir>/token` with mode `0600`, so it
is readable only by the same uid; presenting it therefore proves the peer shares
the daemon's uid. `try_connect_authed` (`src/ipc_transport.rs`) performs this
handshake transparently when a token file is present, and behaves like a plain
connect otherwise, so callers are unaffected when the daemon runs without
`--require-token`. The server side drops the connection if the first frame is
missing or carries a wrong token. Control-plane messages are also capped in size
(`MAX_IPC_MESSAGE_BYTES`) and per-read time so a stalled or hostile peer cannot
exhaust daemon memory.

### Bounded wrapper I/O

The wrapper tees an agent's stdout/stderr through Scotia without letting a
runaway stream grow memory without bound. Per-line reads are capped at
`MAX_LINE_BYTES` (1 MiB) in `src/wrapper/io.rs`. A hostile or runaway agent that
emits a multi-gigabyte newline-free stream cannot make the wrapper allocate an
unbounded buffer: lines beyond the cap are emitted in bounded `MAX_LINE_BYTES`
fragments (lossy-but-bounded), and a warning is logged the first time a stream
exceeds the cap.

### Input handling and injection resistance

Untrusted strings that originate from an agent (task names, working directories,
tool/target labels, notification text) are escaped or rejected at every boundary
where they are re-serialised into another format:

- GraphViz DOT output (synthesizer) escapes labels so a crafted `target`/`tool`
  string cannot inject extra DOT attributes; newlines are flattened.
- systemd `ExecStart` generation (installer/service) quotes and escapes paths
  and rejects embedded newlines/carriage returns outright, so a crafted path
  cannot inject additional unit-file directives.
- macOS launch-agent XML plists (installer) are run through an XML escaper
  before being written into `<string>` elements.
- Desktop-notification markup is escaped so freedesktop notification text cannot
  render as HTML or a clickable link.
- Terminal-bound text has control characters (including the ANSI `ESC`
  introducer) stripped, while newlines and tabs are preserved.

In addition, agent binaries resolved from `PATH` are vetted with
`is_safe_executable` (`src/shim.rs`) before use: a candidate must be a regular
file, be executable, and not be writable by group or others, so a world-writable
impostor earlier on `PATH` is not run.

### Dependency posture

CI runs `cargo audit` and `cargo deny check advisories sources`
(`.github/workflows/ci.yml`) on every push and pull request, so known
vulnerabilities and disallowed dependency sources are caught in the gate.
`serde`, `tokio`, and `uuid` are treated as security-critical dependencies: do
not downgrade or replace them without an explicit security review.

## Platform support

| Platform | Daemon (`scotiad`) | CLI (`scotia`) | Validation |
| --- | --- | --- | --- |
| Linux | Supported | Supported | Full CI (`check` job on `ubuntu-latest`) |
| macOS | Supported (unix permission model, unix sockets) | Supported | CI job `check-macos` runs the runtime permission tests (`cargo test --lib runtime`) on `macos-latest`, so the `0700`/`0600` guarantees are asserted on macOS and not on Linux alone |
| Windows | Not supported | May compile, not validated in CI | No Windows CI job |

Windows is **not** a supported target for the daemon. `scotiad` is built on
`tokio::net::UnixListener` / `UnixStream` and on `tokio::signal::unix`, all of
which are unix-only, so the daemon does not build or run on Windows. The
runtime-permission helpers in `src/runtime.rs` and the `is_safe_executable` mode
checks are explicit no-ops on non-unix targets, and the symlink-based shims use
`std::os::unix::fs::symlink`. The `scotia` CLI's `run`/`replay`/`list` commands
may compile on Windows, but daemon IPC, the `0700`/`0600` socket model, and the
symlink shims are unix-only and none of it is exercised by Windows CI. Do not
assume the Windows build is secure or supported. Native Windows daemon support
is future work and would require a named-pipe transport and Windows ACLs to
re-create the owner-only guarantees that unix permission bits provide today.

## Reporting a vulnerability

Please report suspected vulnerabilities privately through GitHub's private
security advisory workflow for this repository:

<https://github.com/elci-group/scotia/security/advisories/new>

Do not open a public issue for a report you believe has security impact. If you
are unsure whether something is in scope, the private advisory channel is still
the right place to start; maintainers can re-route it from there.
