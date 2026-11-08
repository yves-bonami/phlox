//! Minimal executable, in-memory flow runner. A first synchronous runtime
//! slice; scheduling and backpressure will replace this loop later.

use phlox_plugin_host::registry::PluginRegistry;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

#[derive(Debug, Clone, serde::Deserialize)]
pub struct FlowSpec {
    pub nodes: Vec<FlowNode>,
    pub edges: Vec<FlowEdge>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct FlowNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct FlowEdge {
    pub from: String,
    #[serde(rename = "fromPort")]
    pub from_port: String,
    pub to: String,
    #[serde(rename = "toPort")]
    pub to_port: String,
}

#[derive(Clone, Default)]
pub struct RunState {
    pub running: Arc<AtomicBool>,
    pub logs: Arc<Mutex<Vec<String>>>,
    worker: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
}

impl RunState {
    pub fn start(&self, root: PathBuf, flow: FlowSpec) -> Result<(), String> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err("Flow already running".into());
        }
        // After Stop, join the previous thread before making Running true again.
        if let Some(worker) = self.worker.lock().unwrap().take() {
            let _ = worker.join();
        }
        let mut registry = PluginRegistry::new(root);
        let failures = registry.refresh();
        if !failures.is_empty() {
            self.running.store(false, Ordering::SeqCst);
            return Err(format!("Plugin errors: {failures:?}"));
        }
        for node in &flow.nodes {
            if registry.plugin_for_type(&node.node_type).is_none() {
                self.running.store(false, Ordering::SeqCst);
                return Err(format!("Unknown node type: {}", node.node_type));
            }
        }
        if flow.nodes.is_empty() || flow.edges.is_empty() {
            self.running.store(false, Ordering::SeqCst);
            return Err("Flow must contain nodes and connections".into());
        }
        for edge in &flow.edges {
            let source = flow
                .nodes
                .iter()
                .find(|n| n.id == edge.from)
                .ok_or_else(|| format!("Missing source node {}", edge.from));
            let target = flow
                .nodes
                .iter()
                .find(|n| n.id == edge.to)
                .ok_or_else(|| format!("Missing target node {}", edge.to));
            let ports_valid = (|| {
                let source = source.ok()?;
                let target = target.ok()?;
                let output = registry
                    .plugin_for_type(&source.node_type)?
                    .info
                    .nodes
                    .iter()
                    .find(|n| n.id == source.node_type)?
                    .schema
                    .get("x-phlox-ports")?
                    .get("outputs")?
                    .as_array()?
                    .iter()
                    .find(|port| {
                        port.get("name").and_then(|v| v.as_str()) == Some(edge.from_port.as_str())
                    })?;
                let input = registry
                    .plugin_for_type(&target.node_type)?
                    .info
                    .nodes
                    .iter()
                    .find(|n| n.id == target.node_type)?
                    .schema
                    .get("x-phlox-ports")?
                    .get("inputs")?
                    .as_array()?
                    .iter()
                    .find(|port| {
                        port.get("name").and_then(|v| v.as_str()) == Some(edge.to_port.as_str())
                    })?;
                let output_type = output.get("value_type").and_then(|v| v.as_str());
                let input_type = input.get("value_type").and_then(|v| v.as_str());
                Some(output_type.is_none() || input_type.is_none() || output_type == input_type)
            })()
            .unwrap_or(false);
            if !ports_valid {
                self.running.store(false, Ordering::SeqCst);
                return Err(format!(
                    "Invalid connection: {}.{} -> {}.{}",
                    edge.from, edge.from_port, edge.to, edge.to_port
                ));
            }
        }
        let logs = self.logs.clone();
        logs.lock().unwrap().clear();
        let running = self.running.clone();
        let worker = thread::spawn(move || {
            while running.load(Ordering::SeqCst) {
                for edge in &flow.edges {
                    if !running.load(Ordering::SeqCst) {
                        break;
                    }
                    let source = flow.nodes.iter().find(|n| n.id == edge.from);
                    let target = flow.nodes.iter().find(|n| n.id == edge.to);
                    let (Some(source), Some(target)) = (source, target) else {
                        continue;
                    };
                    let Some(source_plugin) = registry.plugin_for_type(&source.node_type) else {
                        continue;
                    };
                    let Some(target_plugin) = registry.plugin_for_type(&target.node_type) else {
                        continue;
                    };
                    match source_plugin.poll(&source.node_type) {
                        Ok(values) => {
                            for (port, value) in values {
                                if port != edge.from_port {
                                    continue;
                                }
                                let result = target_plugin.consume(&target.node_type, &value);
                                let message = match result {
                                    Ok(()) => format!(
                                        "{}.{port} → {}.{}: {value}",
                                        source.node_type, target.node_type, edge.to_port
                                    ),
                                    Err(error) => format!("Sink error: {error:?}"),
                                };
                                let mut lines = logs.lock().unwrap();
                                lines.push(message);
                                if lines.len() > 100 {
                                    lines.remove(0);
                                }
                            }
                        }
                        Err(error) => {
                            let mut lines = logs.lock().unwrap();
                            lines.push(format!("Source error: {error:?}"));
                            if lines.len() > 100 {
                                lines.remove(0);
                            }
                        }
                    }
                }
                thread::sleep(Duration::from_millis(500));
            }
        });
        *self.worker.lock().unwrap() = Some(worker);
        Ok(())
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}
