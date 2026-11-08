//! In-memory asynchronous flow runner with per-edge bounded queues.

use phlox_plugin_host::{LifecycleAction, registry::PluginRegistry};
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
    lifecycle_lock: Arc<Mutex<()>>,
}

impl RunState {
    pub fn start(&self, root: PathBuf, flow: FlowSpec) -> Result<(), String> {
        let _guard = self
            .lifecycle_lock
            .lock()
            .map_err(|_| "Lifecycle lock poisoned")?;
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
            let Some(plugin) = registry.plugin_for_type(&node.node_type) else {
                self.running.store(false, Ordering::SeqCst);
                return Err(format!("Unknown node type: {}", node.node_type));
            };
            let descriptor = plugin
                .info
                .nodes
                .iter()
                .find(|n| n.id == node.node_type)
                .unwrap();
            if let Err(error) = validate_role(&descriptor.schema) {
                self.running.store(false, Ordering::SeqCst);
                return Err(format!("{}: {error}", node.id));
            }
            if let Err(error) = validate_configuration(&descriptor.schema, &node.configuration) {
                self.running.store(false, Ordering::SeqCst);
                return Err(format!("{}: {error}", node.id));
            }
        }
        if flow.nodes.is_empty() || flow.edges.is_empty() {
            self.running.store(false, Ordering::SeqCst);
            return Err("Flow must contain nodes and connections".into());
        }
        // Reject duplicate instance IDs and cycles before creating queues.
        let mut ids = std::collections::HashSet::new();
        if flow.nodes.iter().any(|node| !ids.insert(&node.id)) {
            self.running.store(false, Ordering::SeqCst);
            return Err("Duplicate node instance ID".into());
        }
        if has_cycle(&flow) {
            self.running.store(false, Ordering::SeqCst);
            return Err("Flow contains a cycle".into());
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
                let source_role = registry
                    .plugin_for_type(&source.node_type)?
                    .info
                    .nodes
                    .iter()
                    .find(|n| n.id == source.node_type)?
                    .schema
                    .get("x-phlox-role")?
                    .as_str()?;
                let target_role = registry
                    .plugin_for_type(&target.node_type)?
                    .info
                    .nodes
                    .iter()
                    .find(|n| n.id == target.node_type)?
                    .schema
                    .get("x-phlox-role")?
                    .as_str()?;
                if source_role == "sink" || target_role == "source" {
                    return Some(false);
                }
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
                        // Initialize each node instance once before scheduling.
                        // Lifecycle callbacks are optional for older plugins.
                        let mut created = Vec::new();
                        let mut handles = std::collections::HashMap::new();
                        for node in &flow.nodes {
                            let plugin = registry.plugin_for_type(&node.node_type).unwrap();
                            match plugin.lifecycle(LifecycleAction::Create {
                                node_type: &node.node_type,
                                instance: &node.id,
                                configuration: &node.configuration,
                            }) {
                                Ok(Some(handle)) => {
                                    handles.insert(node.id.clone(), handle);
                                    created.push((node.node_type.clone(), handle, false));
                                    if let Err(error) =
                                        plugin.lifecycle(LifecycleAction::Start(handle))
                                    {
                                        push_log(
                                            &logs,
                                            format!("Node {} failed to start: {error:?}", node.id),
                                        );
                                        running.store(false, Ordering::SeqCst);
                                        break;
                                    }
                                    created.last_mut().unwrap().2 = true;
                                }
                                Ok(None) => (),
                                Err(error) => {
                                    push_log(
                                        &logs,
                                        format!("Node {} failed to create: {error:?}", node.id),
                                    );
                                    running.store(false, Ordering::SeqCst);
                                    break;
                                }
                            }
                        }
                        let handles = std::rc::Rc::new(handles);
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
                            let handle = handles.get(&target.id).copied();
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
                                        let result = if let Some(handle) = handle {
                                            plugin.instance_consume(handle, &target_port, &value)
                                        } else {
                                            plugin.consume(&target.node_type, &value)
                                        };
                                        match result {
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
                                        let result = if let Some(handle) = handle {
                                            plugin.instance_process(handle, &target_port, &value)
                                        } else {
                                            plugin.process(&target.node_type, &target_port, &value)
                                        };
                                        match result {
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
                                && registry
                                    .plugin_for_type(&node.node_type)
                                    .and_then(|plugin| {
                                        plugin.info.nodes.iter().find(|n| n.id == node.node_type)
                                    })
                                    .and_then(|desc| desc.schema.get("x-phlox-role"))
                                    .and_then(|role| role.as_str())
                                    == Some("source")
                        }) {
                            let source = source.clone();
                            let output_routes: Vec<_> = routes
                                .iter()
                                .filter(|((id, _), _)| id == &source.id)
                                .map(|((_, port), senders)| (port.clone(), senders.clone()))
                                .collect();
                            let registry = registry.clone();
                            let handle = handles.get(&source.id).copied();
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
                                    let plugin =
                                        registry.plugin_for_type(&source.node_type).unwrap();
                                    let result = if let Some(handle) = handle {
                                        plugin.instance_poll(handle)
                                    } else {
                                        plugin.poll(&source.node_type)
                                    };
                                    match result {
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
                        // Stop then destroy in reverse creation order. This also
                        // cleans up partially started flows.
                        for (node_type, handle, started) in created.into_iter().rev() {
                            if let Some(plugin) = registry.plugin_for_type(&node_type) {
                                if started {
                                    if let Err(error) =
                                        plugin.lifecycle(LifecycleAction::Stop(handle))
                                    {
                                        push_log(&logs, format!("Node stop error: {error:?}"));
                                    }
                                }
                                if let Err(error) =
                                    plugin.lifecycle(LifecycleAction::Destroy(handle))
                                {
                                    push_log(&logs, format!("Node destroy error: {error:?}"));
                                }
                            }
                        }
                        running.store(false, Ordering::SeqCst);
                    })
                    .await;
            });
        });
        *self.worker.lock().unwrap() = Some(worker);
        Ok(())
    }

    /// Signal shutdown and block until node stop/destroy callbacks finish.
    pub fn stop(&self) -> Result<(), String> {
        let _guard = self
            .lifecycle_lock
            .lock()
            .map_err(|_| "Lifecycle lock poisoned")?;
        self.running.store(false, Ordering::SeqCst);
        if let Some(worker) = self
            .worker
            .lock()
            .map_err(|_| "Worker lock poisoned")?
            .take()
        {
            worker
                .join()
                .map_err(|_| "Flow worker panicked".to_string())?;
        }
        Ok(())
    }
}

fn push_log(logs: &Arc<Mutex<Vec<String>>>, message: String) {
    let mut lines = logs.lock().unwrap();
    lines.push(message);
    if lines.len() > 100 {
        lines.remove(0);
    }
}

fn validate_configuration(
    schema: &serde_json::Value,
    configuration: &serde_json::Value,
) -> Result<(), String> {
    if configuration.is_null() {
        return Ok(());
    }
    let object = configuration
        .as_object()
        .ok_or("Configuration must be an object")?;
    let properties = schema.get("properties").and_then(|value| value.as_object());
    for (key, value) in object {
        let prop = properties
            .and_then(|props| props.get(key))
            .ok_or_else(|| format!("Unknown configuration field: {key}"))?;
        match prop.get("type").and_then(|v| v.as_str()) {
            Some("integer") => {
                let n = value
                    .as_i64()
                    .ok_or_else(|| format!("{key} must be an integer"))?;
                if let Some(min) = prop.get("minimum").and_then(|v| v.as_i64()) {
                    if n < min {
                        return Err(format!("{key} must be >= {min}"));
                    }
                }
                if let Some(max) = prop.get("maximum").and_then(|v| v.as_i64()) {
                    if n > max {
                        return Err(format!("{key} must be <= {max}"));
                    }
                }
            }
            Some("string") if !value.is_string() => return Err(format!("{key} must be a string")),
            Some("boolean") if !value.is_boolean() => {
                return Err(format!("{key} must be a boolean"));
            }
            _ => {}
        }
    }
    Ok(())
}

fn has_cycle(flow: &FlowSpec) -> bool {
    let mut graph: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
    for edge in &flow.edges {
        graph.entry(&edge.from).or_default().push(&edge.to);
    }
    fn visit<'a>(
        id: &'a str,
        graph: &std::collections::HashMap<&'a str, Vec<&'a str>>,
        marks: &mut std::collections::HashMap<&'a str, u8>,
    ) -> bool {
        match marks.get(id) {
            Some(1) => return true,
            Some(2) => return false,
            _ => {}
        }
        marks.insert(id, 1);
        if graph
            .get(id)
            .is_some_and(|next| next.iter().any(|n| visit(n, graph, marks)))
        {
            return true;
        }
        marks.insert(id, 2);
        false
    }
    let mut marks = std::collections::HashMap::new();
    flow.nodes
        .iter()
        .any(|node| visit(&node.id, &graph, &mut marks))
}

#[cfg(test)]
mod validation_tests {
    use super::*;
    #[test]
    fn validates_node_settings() {
        let schema = serde_json::json!({"properties": {"interval_ms": {"type":"integer","minimum":50,"maximum":60000}}});
        assert!(validate_configuration(&schema, &serde_json::json!({"interval_ms":150})).is_ok());
        assert!(validate_configuration(&schema, &serde_json::json!({"interval_ms":10})).is_err());
        assert!(
            validate_configuration(&schema, &serde_json::json!({"interval_ms":"fast"})).is_err()
        );
        assert!(validate_configuration(&schema, &serde_json::json!({"unknown":1})).is_err());
    }
    #[test]
    fn rejects_cycles() {
        let nodes = ["a", "b"]
            .iter()
            .map(|id| FlowNode {
                id: (*id).into(),
                node_type: "example/source".into(),
                configuration: serde_json::Value::Null,
            })
            .collect();
        let mut flow = FlowSpec {
            nodes,
            edges: vec![FlowEdge {
                from: "a".into(),
                from_port: "out".into(),
                to: "b".into(),
                to_port: "in".into(),
            }],
        };
        assert!(!has_cycle(&flow));
        flow.edges.push(FlowEdge {
            from: "b".into(),
            from_port: "out".into(),
            to: "a".into(),
            to_port: "in".into(),
        });
        assert!(has_cycle(&flow));
    }
}

fn validate_role(schema: &serde_json::Value) -> Result<(), String> {
    match schema.get("x-phlox-role").and_then(|role| role.as_str()) {
        Some("source" | "processor" | "sink") => Ok(()),
        _ => Err("Node type must declare x-phlox-role (source, processor, or sink)".into()),
    }
}
