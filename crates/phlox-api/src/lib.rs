//! REST management endpoints for Phlox.

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, State},
    http::StatusCode,
    routing::{get, post},
};
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

#[derive(Clone)]
pub struct PluginState {
    pub snapshot: PluginSnapshot,
    pub root: PathBuf,
}

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

pub fn router(state: PluginState) -> Router {
    Router::new()
        .route("/api/plugins", get(list_plugins))
        .route("/api/plugins/install", post(install_plugin))
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
        .with_state(state)
}

async fn list_plugins(State(state): State<PluginState>) -> Json<Vec<PluginDescriptor>> {
    Json(
        state
            .snapshot
            .read()
            .expect("plugin snapshot lock poisoned")
            .clone(),
    )
}

/// Install a trusted native plugin from two uploaded files.
//// Security: native plugins run with the privileges of the server process.
/// This endpoint is intended for loopback-only development until authentication exists.
async fn install_plugin(
    State(state): State<PluginState>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
    let error = |message: String| (StatusCode::BAD_REQUEST, message);
    let mut manifest = None;
    let mut library = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| error(e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_owned();
        let file_name = field.file_name().unwrap_or("").to_owned();
        if name != "manifest" && name != "library" {
            return Err(error(
                "Only manifest and library fields are accepted".into(),
            ));
        }
        let data = field.bytes().await.map_err(|e| error(e.to_string()))?;
        if name == "manifest" {
            if manifest.replace(data).is_some() {
                return Err(error("Duplicate manifest".into()));
            }
        } else {
            if file_name.is_empty()
                || !file_name.ends_with(".so")
                || !file_name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            {
                return Err(error(
                    "Library filename must be a simple .so filename".into(),
                ));
            }
            if library.replace((file_name, data)).is_some() {
                return Err(error("Duplicate library".into()));
            }
        }
    }
    let manifest = manifest.ok_or_else(|| error("Missing plugin.toml".into()))?;
    let (file_name, bytes) = library.ok_or_else(|| error("Missing .so library".into()))?;
    let stage = state.root.join(format!(".upload-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&state.root)
        .await
        .map_err(|e| error(e.to_string()))?;
    tokio::fs::create_dir(&stage)
        .await
        .map_err(|e| error(e.to_string()))?;
    let result = async {
        tokio::fs::write(stage.join("plugin.toml"), manifest).await?;
        tokio::fs::write(stage.join(file_name), bytes).await?;
        Ok::<_, std::io::Error>(())
    }
    .await;
    if let Err(e) = result {
        let _ = tokio::fs::remove_dir_all(&stage).await;
        return Err(error(e.to_string()));
    }
    // install validates manifest, package identity, ABI, and immutable destination.
    let installed = PluginRegistry::new(&state.root).install(&stage);
    let _ = tokio::fs::remove_dir_all(&stage).await;
    let installed = installed.map_err(|e| error(format!("{e:?}")))?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"installed": installed.display().to_string()})),
    ))
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
        let response = router(PluginState {
            snapshot,
            root: std::env::temp_dir().join("phlox-api-test"),
        })
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
