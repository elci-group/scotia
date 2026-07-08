use axum::{Router, extract::Request};
use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::{Level, info};

/// Static file server for scotia.tech.
#[derive(Parser, Debug)]
#[command(name = "scotia-tech-server")]
#[command(about = "Serve the scotia.tech static website")]
struct Args {
    /// Port to listen on.
    #[arg(short, long, default_value = "8080")]
    port: u16,

    /// Host to bind to.
    #[arg(short = 'b', long, default_value = "127.0.0.1")]
    host: String,

    /// Directory containing the static site.
    #[arg(long, default_value = "..")]
    root: PathBuf,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .init();

    let args = Args::parse();
    let root = args
        .root
        .canonicalize()
        .expect("failed to canonicalize root directory");

    let index = root.join("index.html");
    let serve_dir = ServeDir::new(&root)
        .fallback(ServeFile::new(&index));

    let app = Router::new()
        .nest_service("/", serve_dir)
        .layer(TraceLayer::new_for_http().make_span_with(|request: &Request<_>| {
            tracing::info_span!(
                "http_request",
                method = %request.method(),
                uri = %request.uri(),
            )
        }));

    let addr: SocketAddr = format!("{}:{}", args.host, args.port)
        .parse()
        .expect("invalid socket address");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind");

    info!("scotia.tech server listening on http://{}", addr);
    info!("serving files from {}", root.display());

    axum::serve(listener, app)
        .await
        .expect("server failed");
}
