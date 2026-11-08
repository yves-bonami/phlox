//! REST management endpoints for Phlox.

use axum::{Json, Router, routing::get};
use phlox_plugin_host::{PluginInfo, registry::PluginRegistry};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

#[derive(Debug, Clone, Serialize)]
pub struct NodeDescriptor {
    pub id: String,
    pub schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginDescriptor {
    pub id: String,
    pub version: String,
    pub nodes: Vec<NodeDescriptor>,
}

impl From<&PluginInfo> for PluginDescriptor {
    fn from(value: &PluginInfo) -> Self {
        Self {
            id: value.id.clone(),
            version: value.version.clone(),
            nodes: value
                .nodes
                .iter()
                .map(|node| NodeDescriptor {
                    id: node.id.clone(),
                    schema: node.schema.clone(),
                })
                .collect(),
        }
    }
}

pub type PluginSnapshot = Arc<RwLock<Vec<PluginDescriptor>>>;

/// Start the local registry watcher and return a snapshot shared with HTTP.
pub fn start_plugin_discovery(root: PathBuf) -> PluginSnapshot {
    let snapshot: PluginSnapshot = Arc::new(RwLock::new(Vec::new()));
    let shared = Arc::clone(&snapshot);
    std::thread::spawn(move || {
        let registry = PluginRegistry::new(root);
        if let Err(error) = registry.watch(move |registry| {
            let updated = registry.active().map(PluginDescriptor::from).collect();
            *shared.write().expect("plugin snapshot lock poisoned") = updated;
        }) {
            eprintln!("Plugin discovery stopped: {error:?}");
        }
    });
    snapshot
}

pub fn router(snapshot: PluginSnapshot) -> Router {
    Router::new()
        .route("/api/plugins", get(list_plugins))
        .with_state(snapshot)
}

async fn list_plugins(
    axum::extract::State(snapshot): axum::extract::State<PluginSnapshot>,
) -> Json<Vec<PluginDescriptor>> {
    Json(
        snapshot
            .read()
            .expect("plugin snapshot lock poisoned")
            .clone(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use tower::ServiceExt;

    #[tokio::test]
    async fn plugins_endpoint_returns_snapshot() {
        let snapshot = Arc::new(RwLock::new(vec![PluginDescriptor {
            id: "example".into(),
            version: "0.1.0".into(),
            nodes: vec![NodeDescriptor {
                id: "example/source".into(),
                schema: serde_json::json!({"type":"object"}),
            }],
        }]));
        let response = router(snapshot)
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/plugins")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json[0]["nodes"][0]["id"], "example/source");
    }
}
