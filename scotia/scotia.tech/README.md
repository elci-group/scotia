:warning: **Rust-based server policy** — This website is served by a Rust static file server. Do not add a Python, Node, or other non-Rust server unless the user explicitly requires a feature that Rust cannot provide.

# scotia.tech

Static website for the Scotia project, served by a Rust-based static file server.

## Files

- `index.html` — landing page
- `styles.css` — responsive theme-aware stylesheet
- `script.js` — theme toggle, copy buttons, scroll animations
- `docs/index.html` — documentation page
- `server/` — Rust static file server (Axum + tower-http)

## Run the Rust server

```bash
cd server
cargo run
```

The site is available at `http://127.0.0.1:8080`.

Options:

```bash
cargo run -- --port 3000 --host 0.0.0.0 --root ../
```

## Build release

```bash
cd server
cargo build --release
./target/release/scotia-tech-server
```

## Development

You can open `index.html` directly in a browser during UI work, but always verify against the Rust server before finishing. The Rust server is the canonical runtime for this site.

## Theme

The site supports dark and light modes. The user's preference is persisted in `localStorage` and respects `prefers-color-scheme`.

## Deployment

The static files (`index.html`, `styles.css`, `script.js`, `docs/`) can be deployed to any static host (GitHub Pages, Cloudflare Pages, Netlify, Vercel, S3, etc.). The Rust server can also be containerised and deployed behind a reverse proxy.
