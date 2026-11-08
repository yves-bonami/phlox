//! In-memory asynchronous flow runner with per-edge bounded queues.

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
    #[serde(default)]
    pub configuration: serde_json::Value,
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
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .expect("Tokio runtime");
            runtime.block_on(async move {
                // Local tasks avoid requiring native libraries to be Send/Sync.
                let local = tokio::task::LocalSet::new();
                local
                    .run_until(async move {
                        let registry = std::rc::Rc::new(registry);
                        let mut routes: std::collections::HashMap<
                            (String, String),
                            Vec<tokio::sync::mpsc::Sender<serde_json::Value>>,
                        > = std::collections::HashMap::new();
                        let mut consumers = Vec::new();
                        for edge in &flow.edges {
                            let (sender, receiver) =
                                tokio::sync::mpsc::channel::<serde_json::Value>(64);
                            routes
                                .entry((edge.from.clone(), edge.from_port.clone()))
                                .or_default()
                                .push(sender);
                            consumers.push((edge.clone(), receiver));
                        }
                        // Wire all sender routes before starting consumers: processors
                        // must see their downstream queues, independent of edge ordering.
                        for (edge, mut receiver) in consumers {
                            let target =
                                flow.nodes.iter().find(|n| n.id == edge.to).unwrap().clone();
                            let target_port = edge.to_port.clone();
                            let forwards: Vec<_> = routes
                                .iter()
                                .filter(|((id, _), _)| id == &target.id)
                                .map(|((_, port), senders)| (port.clone(), senders.clone()))
                                .collect();
                            let registry = registry.clone();
                            let logs = logs.clone();
                            let running = running.clone();
                            tokio::task::spawn_local(async move {
                                while running.load(Ordering::SeqCst) {
                                    let Some(value) = receiver.recv().await else {
                                        break;
                                    };
                                    let plugin =
                                        registry.plugin_for_type(&target.node_type).unwrap();
                                    if forwards.is_empty() {
                                        match plugin.consume(&target.node_type, &value) {
                                            Ok(()) => push_log(
                                                &logs,
                                                format!(
                                                    "{}.{target_port}: {value}",
                                                    target.node_type
                                                ),
                                            ),
                                            Err(error) => {
                                                push_log(&logs, format!("Sink error: {error:?}"))
                                            }
                                        }
                                    } else {
                                        match plugin.process(
                                            &target.node_type,
                                            &target_port,
                                            &value,
                                        ) {
                                            Ok(values) => {
                                                for (port, transformed) in values {
                                                    for (_, senders) in
                                                        forwards.iter().filter(|(p, _)| *p == port)
                                                    {
                                                        for sender in senders {
                                                            if sender
                                                                .try_send(transformed.clone())
                                                                .is_err()
                                                            {
                                                                push_log(
                                                                    &logs,
                                                                    format!(
                                                                        "Queue full: {}.{port}",
                                                                        target.node_type
                                                                    ),
                                                                );
                                                            } else {
                                                                push_log(
                                                                    &logs,
                                                                    format!(
                                                                        "{}.{port}: {transformed}",
                                                                        target.node_type
                                                                    ),
                                                                );
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            Err(error) => push_log(
                                                &logs,
                                                format!("Processor error: {error:?}"),
                                            ),
                                        }
                                    }
                                }
                            });
                        }
                        for source in flow.nodes.iter().filter(|node| {
                            routes.keys().any(|(id, _)| id == &node.id)
                                && !flow.edges.iter().any(|edge| edge.to == node.id)
                        }) {
                            let source = source.clone();
                            let output_routes: Vec<_> = routes
                                .iter()
                                .filter(|((id, _), _)| id == &source.id)
                                .map(|((_, port), senders)| (port.clone(), senders.clone()))
                                .collect();
                            let registry = registry.clone();
                            let running = running.clone();
                            let logs = logs.clone();
                            tokio::task::spawn_local(async move {
                                let mut timer = tokio::time::interval(Duration::from_millis(
                                    source
                                        .configuration
                                        .get("interval_ms")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(500)
                                        .clamp(50, 60_000),
                                ));
                                timer.set_missed_tick_behavior(
                                    tokio::time::MissedTickBehavior::Skip,
                                );
                                while running.load(Ordering::SeqCst) {
                                    timer.tick().await;
                                    if !running.load(Ordering::SeqCst) {
                                        break;
                                    }
                                    match registry
                                        .plugin_for_type(&source.node_type)
                                        .unwrap()
                                        .poll(&source.node_type)
                                    {
                                        Ok(values) => {
                                            for (port, value) in values {
                                                for (_, senders) in output_routes
                                                    .iter()
                                                    .filter(|(name, _)| *name == port)
                                                {
                                                    for sender in senders {
                                                        if sender.try_send(value.clone()).is_err() {
                                                            push_log(
                                                                &logs,
                                                                format!(
                                                                    "Queue full: {}.{port}",
                                                                    source.node_type
                                                                ),
                                                            );
                                                        } else {
                                                            push_log(
                                                                &logs,
                                                                format!(
                                                                    "{}.{port}: {value}",
                                                                    source.node_type
                                                                ),
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        Err(error) => {
                                            push_log(&logs, format!("Source error: {error:?}"))
                                        }
                                    }
                                }
                            });
                        }
                        while running.load(Ordering::SeqCst) {
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                    })
                    .await;
            });
        });
        *self.worker.lock().unwrap() = Some(worker);
        Ok(())
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

fn push_log(logs: &Arc<Mutex<Vec<String>>>, message: String) {
    let mut lines = logs.lock().unwrap();
    lines.push(message);
    if lines.len() > 100 {
        lines.remove(0);
    }
}
