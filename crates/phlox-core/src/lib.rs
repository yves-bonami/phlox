//! Pure domain types for Phlox. No runtime, networking, or plugin dependencies.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub type FlowId = Uuid;
pub type NodeId = Uuid;
pub type MessageId = Uuid;

/// A portable, schema-versioned directed flow definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Flow {
    pub schema_version: u32,
    pub id: FlowId,
    pub name: String,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    /// Plugin-owned type identifier (e.g. "example/source").
    pub node_type: String,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    #[serde(default)]
    pub configuration: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Port {
    pub name: String,
    /// Optional declared value type; None accepts any type.
    pub value_type: Option<ValueType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub from_node: NodeId,
    pub from_port: String,
    pub to_node: NodeId,
    pub to_port: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueType {
    Null,
    Boolean,
    Integer,
    Float,
    String,
    Binary,
    Array,
    Object,
}

/// Typed payloads retain binary data without encoding it as JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum Value {
    Null,
    Boolean(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Binary(Vec<u8>),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl Value {
    pub fn value_type(&self) -> ValueType {
        match self {
            Self::Null => ValueType::Null,
            Self::Boolean(_) => ValueType::Boolean,
            Self::Integer(_) => ValueType::Integer,
            Self::Float(_) => ValueType::Float,
            Self::String(_) => ValueType::String,
            Self::Binary(_) => ValueType::Binary,
            Self::Array(_) => ValueType::Array,
            Self::Object(_) => ValueType::Object,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    Good,
    Uncertain,
    Bad,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub payload: Value,
    pub received_at: DateTime<Utc>,
    pub source_timestamp: Option<DateTime<Utc>>,
    pub quality: Option<Quality>,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}

impl Message {
    pub fn new(payload: Value) -> Self {
        Self {
            id: Uuid::new_v4(),
            payload,
            received_at: Utc::now(),
            source_timestamp: None,
            quality: None,
            metadata: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    UnsupportedSchemaVersion(u32),
    EmptyFlowName,
    DuplicateNode(NodeId),
    EmptyNodeType(NodeId),
    EmptyPortName(NodeId),
    DuplicateInputPort(NodeId, String),
    DuplicateOutputPort(NodeId, String),
    UnknownSource(NodeId),
    UnknownTarget(NodeId),
    UnknownOutput(NodeId, String),
    UnknownInput(NodeId, String),
    IncompatiblePorts(NodeId, String, NodeId, String),
}

impl Flow {
    pub const SCHEMA_VERSION: u32 = 1;

    /// Returns all errors without starting or modifying a flow.
    /// Cycles are intentionally allowed (runtime safeguards come later).
    pub fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();
        if self.schema_version != Self::SCHEMA_VERSION {
            errors.push(ValidationError::UnsupportedSchemaVersion(
                self.schema_version,
            ));
        }
        if self.name.trim().is_empty() {
            errors.push(ValidationError::EmptyFlowName);
        }
        let mut ids = BTreeSet::new();
        for node in &self.nodes {
            if !ids.insert(node.id) {
                errors.push(ValidationError::DuplicateNode(node.id));
            }
            if node.node_type.trim().is_empty() {
                errors.push(ValidationError::EmptyNodeType(node.id));
            }
            for (ports, input) in [(&node.inputs, true), (&node.outputs, false)] {
                let mut names = BTreeSet::new();
                for port in ports {
                    if port.name.trim().is_empty() {
                        errors.push(ValidationError::EmptyPortName(node.id));
                    }
                    if !names.insert(&port.name) {
                        errors.push(if input {
                            ValidationError::DuplicateInputPort(node.id, port.name.clone())
                        } else {
                            ValidationError::DuplicateOutputPort(node.id, port.name.clone())
                        });
                    }
                }
            }
        }
        // Duplicate IDs are already invalid; avoid ambiguously validating their edges.
        if errors
            .iter()
            .any(|e| matches!(e, ValidationError::DuplicateNode(_)))
        {
            return errors;
        }
        let nodes: BTreeMap<_, _> = self.nodes.iter().map(|n| (n.id, n)).collect();
        for edge in &self.edges {
            let source = nodes.get(&edge.from_node);
            let target = nodes.get(&edge.to_node);
            if source.is_none() {
                errors.push(ValidationError::UnknownSource(edge.from_node));
            }
            if target.is_none() {
                errors.push(ValidationError::UnknownTarget(edge.to_node));
            }
            let output = source.and_then(|n| n.outputs.iter().find(|p| p.name == edge.from_port));
            let input = target.and_then(|n| n.inputs.iter().find(|p| p.name == edge.to_port));
            if source.is_some() && output.is_none() {
                errors.push(ValidationError::UnknownOutput(
                    edge.from_node,
                    edge.from_port.clone(),
                ));
            }
            if target.is_some() && input.is_none() {
                errors.push(ValidationError::UnknownInput(
                    edge.to_node,
                    edge.to_port.clone(),
                ));
            }
            if let (Some(output), Some(input)) = (output, input) {
                if let (Some(a), Some(b)) = (output.value_type, input.value_type) {
                    if a != b {
                        errors.push(ValidationError::IncompatiblePorts(
                            edge.from_node,
                            edge.from_port.clone(),
                            edge.to_node,
                            edge.to_port.clone(),
                        ));
                    }
                }
            }
        }
        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: NodeId, inputs: Vec<Port>, outputs: Vec<Port>) -> Node {
        Node {
            id,
            node_type: "test/node".into(),
            inputs,
            outputs,
            configuration: serde_json::Value::Null,
        }
    }

    fn port(name: &str, value_type: Option<ValueType>) -> Port {
        Port {
            name: name.into(),
            value_type,
        }
    }

    #[test]
    fn valid_flow_with_named_ports() {
        let source = Uuid::new_v4();
        let sink = Uuid::new_v4();
        let flow = Flow {
            schema_version: Flow::SCHEMA_VERSION,
            id: Uuid::new_v4(),
            name: "test".into(),
            nodes: vec![
                node(source, vec![], vec![port("out", Some(ValueType::Integer))]),
                node(sink, vec![port("in", Some(ValueType::Integer))], vec![]),
            ],
            edges: vec![Edge {
                from_node: source,
                from_port: "out".into(),
                to_node: sink,
                to_port: "in".into(),
            }],
        };
        assert!(flow.validate().is_empty());
        assert_eq!(
            serde_json::from_str::<Flow>(&serde_json::to_string(&flow).unwrap()).unwrap(),
            flow
        );
    }

    #[test]
    fn detects_missing_nodes_ports_and_type_mismatch() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let missing = Uuid::new_v4();
        let flow = Flow {
            schema_version: 1,
            id: Uuid::new_v4(),
            name: "test".into(),
            nodes: vec![
                node(a, vec![], vec![port("out", Some(ValueType::String))]),
                node(b, vec![port("in", Some(ValueType::Integer))], vec![]),
            ],
            edges: vec![
                Edge {
                    from_node: a,
                    from_port: "out".into(),
                    to_node: b,
                    to_port: "in".into(),
                },
                Edge {
                    from_node: missing,
                    from_port: "out".into(),
                    to_node: b,
                    to_port: "wrong".into(),
                },
            ],
        };
        let errors = flow.validate();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ValidationError::IncompatiblePorts(..)))
        );
        assert!(errors.contains(&ValidationError::UnknownSource(missing)));
        assert!(errors.contains(&ValidationError::UnknownInput(b, "wrong".into())));
    }

    #[test]
    fn detects_duplicate_node_and_port_names() {
        let id = Uuid::new_v4();
        let n = node(id, vec![port("a", None), port("a", None)], vec![]);
        let flow = Flow {
            schema_version: 1,
            id: Uuid::new_v4(),
            name: "test".into(),
            nodes: vec![n.clone(), n],
            edges: vec![],
        };
        let errors = flow.validate();
        assert!(errors.contains(&ValidationError::DuplicateNode(id)));
        assert!(errors.contains(&ValidationError::DuplicateInputPort(id, "a".into())));
    }

    #[test]
    fn typed_message_round_trip() {
        let mut message = Message::new(Value::Binary(vec![0, 1, 255]));
        message.quality = Some(Quality::Good);
        message
            .metadata
            .insert("example:code".into(), Value::Integer(42));
        let decoded: Message =
            serde_json::from_str(&serde_json::to_string(&message).unwrap()).unwrap();
        assert_eq!(decoded, message);
        assert_eq!(message.payload.value_type(), ValueType::Binary);
    }
}
