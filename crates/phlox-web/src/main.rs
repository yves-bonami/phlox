use askama::Template;
use axum::{Router, http::StatusCode, response::Html, routing::get};
use std::net::SocketAddr;
use tower_http::services::ServeDir;

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    title: &'static str,
}

async fn render_index() -> Result<Html<String>, StatusCode> {
    let template = IndexTemplate { title: "Phlox" };
    template
        .render()
        .map(Html)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn plugin_directory() -> std::path::PathBuf {
    if let Some(configured) = std::env::var_os("PHLOX_PLUGIN_DIR") {
        return configured.into();
    }
    if let Some(data) = std::env::var_os("XDG_DATA_HOME").filter(|s| !s.is_empty()) {
        return std::path::PathBuf::from(data).join("phlox/plugins");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return std::path::PathBuf::from(home).join(".local/share/phlox/plugins");
    }
    std::path::PathBuf::from("/var/lib/phlox/plugins")
}

fn app() -> Router {
    Router::new()
        .merge(phlox_api::router(phlox_api::PluginState {
            snapshot: phlox_api::start_plugin_discovery(plugin_directory()),
            root: plugin_directory(),
        }))
        .route("/", get(render_index))
        .route("/health", get(|| async { "ok" }))
        .nest_service(
            "/static",
            ServeDir::new(concat!(env!("CARGO_MANIFEST_DIR"), "/static")),
        )
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let address: SocketAddr = std::env::var("PHLOX_WEB_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:3000".into())
        .parse()
        .expect("PHLOX_WEB_ADDR must be a valid socket address");
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("Phlox web listening at http://{address}");
    axum::serve(listener, app()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_page() {
        let response = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(render_index())
            .unwrap();
        assert!(response.0.contains("Phlox"));
        assert!(response.0.contains("flow-canvas"));
    }
}
