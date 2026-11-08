# Phlox — Architecture & Plugin System Design

**Status:** Draft / RFC 0001  
**Version:** 0.1  
**Date:** 2026-11-08

## 1. Summary

Phlox is an extensible, Rust-based integration engine for connecting arbitrary systems through graphical or programmatically defined flows. It draws inspiration from Node-RED while prioritizing reusable libraries, protocol independence, and third-party extensibility.

The engine does not implement protocols directly. Independent plugins such as `phlox-mqtt`, `phlox-opcua`, or `phlox-modbus` contribute node types that can be instantiated and connected in flows. Non-protocol functionality (transformation, filtering, scheduling) uses the same node abstraction.

**Initial architecture decision:** Use a transport-independent, versioned plugin protocol with **out-of-process plugins as the primary third-party deployment model**. Built-in plugins can use an in-process adapter for efficiency. The initial design does not promise runtime portability of ordinary Rust trait objects across shared-library boundaries.

## 2. Goals and non-goals

### Goals

1. Build a reusable Rust **core library** independent of CLI, GUI, and protocol implementations.
2. Allow external teams to create, version, distribute, and execute plugins without rebuilding Phlox.
3. Compose sources, processors, and sinks into directed flows.
4. Support persistent connections and long-running subscriptions, as well as request/response and polling.
5. Support metadata-driven node discovery, configuration, and eventual GUI editors.
6. Provide lifecycle, supervision, bounded backpressure, observability, and versioned contracts.
7. Keep the design suitable for Linux edge deployments with an optional future distributed runtime.

### Non-goals for MVP

- A complete graphical editor or Node-RED feature parity.
- Exactly-once delivery across arbitrary protocols.
- Automatic discovery and installation from a public plugin marketplace.
- Transparent clustering or remote placement of nodes.
- Executing untrusted plugins with a formally proven sandbox.
- Designing every conceivable typed payload or industrial datatype up front.

## 3. Principles

- **Core is protocol-agnostic.** No MQTT, OPC UA, or Modbus types in the runtime API.
- **Plugins contribute node types.** Protocols are plugins; transformations are plugins too.
- **Separate contract from transport.** An in-process adapter and IPC adapter expose equivalent *logical* operations, not an identical Rust ABI.
- **Explicit boundaries.** Versioned schemas, typed messages, and named input/output ports.
- **Supervise independently.** Failure of a plugin should not automatically terminate the engine.
- **Backpressure is part of the contract.** Never assume infinite queues.
- **Secrets and credentials stay out of exported flow definitions.** Store references to secrets.
- **Start small.** Establish a minimal end-to-end flow before optimising for every use case.

## 4. System overview

```text
                  CLI              Future GUI / API
                   |                      |
                   +----------+-----------+
                              |
                        Phlox Runtime
               +--------------+---------------+
               |              |               |
          Flow Registry   Flow Scheduler   Plugin Manager
               |              |               |
               +--------- Message Router -----+
                              |
                    Node Instance API
                              |
         +--------------------+--------------------+
         |                    |                    |
   Built-in adapter       IPC adapter         Future adapters
         |                    |                    |
   Built-in nodes       phlox-mqtt          WASM / remote
                       phlox-opcua
                       phlox-modbus
```

### Major components

| Component | Responsibility |
|---|---|
| `phlox-core` | Flow definitions, node/port metadata, message types, validation, identifiers; no Tokio or protocol knowledge required unless justified. |
| `phlox-runtime` | Graph execution, routing, lifecycle, cancellation, queues, supervision. |
| `phlox-plugin-api` | Versioned plugin manifest and logical request/event contracts. |
| `phlox-plugin-host` | Discovery, compatibility checks, IPC, plugin processes, health/restarts. |
| `phlox-cli` | Validate, run, list plugins/nodes, inspect flow status. |
| `phlox-nodes-basic` | Built-in inject, log, map/filter nodes for MVP. |
| `phlox-mqtt` | First external plugin proving independent distribution. |

## 5. Plugin model

### 5.1 Definitions

- **Plugin:** A separately versioned provider of node types and optionally reusable resources.
- **Node type:** Metadata and behavior factory, e.g. `mqtt.subscribe` or `mqtt.publish`.
- **Node instance:** Configured runtime instance of a node type within a flow.
- **Port:** Named, directed connection point with an optional declared payload type/schema.
- **Resource:** Shared plugin-owned object, e.g. an MQTT broker connection.
- **Flow:** A versioned directed graph of node instances and edges.

Node type IDs should be namespaced to avoid collisions, e.g. `phlox-mqtt/subscribe`, `phlox-mqtt/publish`.

### 5.2 Required plugin capabilities

A plugin must support:

- `hello`: handshake including protocol version, plugin ID/version and capabilities.
- `describe`: enumerate node types, port definitions, configuration JSON Schemas, and optional UI metadata.
- `create_instance`: validate configuration and allocate a node instance.
- `start_instance` and `stop_instance`: manage background work and cleanup.
- `deliver`: pass an incoming message to a named input port.
- `emit`: send an outgoing message on a named output port to the host.
- `destroy_instance`: release resources and remove the instance.
- `health`: report plugin and optionally node-instance health.

The host owns graph routing. Plugins cannot directly invoke downstream node implementations.

### 5.3 Example manifest

```toml
[plugin]
id = "phlox-mqtt"
version = "0.1.0"
protocol_version = "1"
entrypoint = "phlox-mqtt"

[[nodes]]
id = "phlox-mqtt/subscribe"
display_name = "MQTT Subscribe"
category = "input"

[[nodes]]
id = "phlox-mqtt/publish"
display_name = "MQTT Publish"
category = "output"
```

**Important:** The manifest is an index and discovery hint, not authoritative executable behavior. `describe` supplies the actual node definitions. The host validates consistency between the two.

### 5.4 Configuration and secrets

Each node type publishes a JSON Schema. UI annotations (labels, ordering, widgets) are optional and must not alter runtime semantics. Values such as usernames and passwords should reference a host-managed secret ID; configurations should not embed secrets. The plugin receives permitted secrets through a host-mediated mechanism. MVP may use a constrained environment/IPC handoff, documented as a security trade-off.

### 5.5 Shared resources

An MQTT plugin can expose `connection_ref` in node configuration to let multiple subscribers/publishers share one broker connection. The plugin owns connection pooling, subscription multiplexing, and reconnect handling. Resources must be scoped to plugin instances and released when no longer referenced. The host does not need to understand MQTT connection semantics.

## 6. Node and flow contracts

### 6.1 Nodes

Nodes can be sources (no inputs), processors (inputs and outputs), sinks (no outputs), or a mixture. Each node is asynchronous and may emit zero or more outputs per input, plus unsolicited events from a subscription or timer.

Conceptual in-process Rust API (not an ABI-stable plugin interface):

```rust
#[async_trait::async_trait]
pub trait Node: Send {
    async fn start(&mut self, context: NodeContext) -> anyhow::Result<()>;
    async fn on_message(
        &mut self,
        input_port: &str,
        message: Message,
        context: &NodeContext,
    ) -> anyhow::Result<()>;
    async fn stop(&mut self) -> anyhow::Result<()>;
}
```

An IPC adapter maps the same logical operations onto a versioned wire protocol. In-process and out-of-process implementations may differ internally in concurrency and error handling.

### 6.2 Messages

MVP envelope:

```json
{
  "id": "01J...",
  "timestamp": "2026-11-08T10:00:00Z",
  "payload": { "kind": "json", "value": { "temperature": 21.4 } },
  "metadata": { "source": "sensor-1" },
  "correlation_id": null
}
```

The payload is a tagged union: `json`, `bytes`, `text` initially. Later extensions might include numeric arrays and structured industrial values. IPC frames must preserve raw bytes without JSON/base64 expansion where possible. The envelope includes tracing/correlation identifiers; transport-specific details belong in metadata or typed extensions, not core fields.

### 6.3 Ports and type compatibility

A node definition declares input and output port IDs. Nodes can optionally expose a type/schema, such as `application/json` plus JSON Schema. The editor/validator flags incompatible connections where known. MVP supports dynamic payloads and validates *port existence*; deeper runtime schema checks are optional to avoid excessive overhead.

### 6.4 Flow example

```yaml
schema_version: 1
id: example-bridge
nodes:
  - id: telemetry
    type: phlox-mqtt/subscribe
    config:
      connection_ref: broker-main
      topic: sensors/#
  - id: log
    type: phlox-basic/log
    config: {}
edges:
  - from: { node: telemetry, port: message }
    to:   { node: log, port: input }
```

The flow validator checks unique node IDs, known node types, required configuration, valid ports, and graph structure. Cycles can be supported, but the engine must prevent unbounded synchronous recursion and provide clear rules for buffering and loop diagnostics.

## 7. Execution semantics

### 7.1 Lifecycle

Host states: `created -> starting -> running -> stopping -> stopped`, with `failed` and `restarting` for error paths. Startup order should ensure downstream consumers are ready before sources produce messages. Stop cancels source work, drains within a configurable deadline, and closes resources.

### 7.2 Concurrency and message routing

- Tokio executes the runtime and IPC tasks.
- Edges use bounded async channels or an equivalent bounded queue abstraction.
- `emit` is asynchronous and may wait when downstream queues are full.
- Ordering is guaranteed **per edge**, assuming one producer sequence; there is no total ordering across parallel branches.
- Fan-out creates logically independent deliveries, with payload sharing/copying determined by representation.
- Default semantics are **at-most-once inside the engine**: failed delivery is reported, not silently retried. Explicit retries and protocol acknowledgements belong to node/flow policy.

**Open choice:** When one of multiple downstream consumers is slow, should fan-out block all outputs, apply per-edge policies, or offer node-configured drop strategies? MVP: block with bounded queues and emit queue-depth metrics; add policies only after measurement.

### 7.3 Failures and supervision

- A node failure produces a structured error and transitions that instance to `failed`.
- A plugin process crash marks all of its node instances unavailable; the host may restart the process with exponential backoff and recreate instances from configuration.
- Restart can duplicate external operations. Plugins must document their delivery semantics; Phlox does not claim exactly-once behavior.
- Support per-node `stop`, `restart`, and eventual `disabled` control.
- A failed node does not necessarily stop the entire flow; configurable failure policies can be added later.

### 7.4 Persistence

MVP persists flow definitions and secret references; it does **not** persist every in-flight message. On engine restart, flows are reconstructed and subscriptions re-established. Durable queues, checkpoints, and replay require a separate design and should not be implicitly promised.

## 8. Plugin transport and isolation

### Preferred MVP: child-process IPC

The plugin host starts an executable and exchanges framed, versioned messages over local IPC. Prefer a Unix domain socket on Linux; an abstract transport layer permits other platforms later. Pick **one** encoding for MVP (e.g. length-delimited protobuf); avoid implementing several transports prematurely.

Required characteristics:

- Mandatory handshake and compatibility negotiation before accepting node definitions.
- Request/response IDs for commands, plus asynchronous events for `emit` and health.
- Bounded outbound queues and cancellation/deadline handling.
- Explicit maximum frame/payload sizes.
- Detect EOF, timeout, process crash, and malformed frames.
- Do not confuse process separation with a complete sandbox: plugins may still inherit host permissions unless OS restrictions are applied.

### Built-in and future adapters

Built-in nodes can implement a native Rust trait and avoid IPC. A future WASM adapter may use a Component Model/WIT contract, and a future native shared-library adapter could use an ABI-stable interface. Neither is required for MVP. The plugin protocol is the compatibility boundary; Rust crate APIs may evolve more rapidly.

## 9. Security considerations

Native plugin executables are trusted by default; there is no safe execution of arbitrary downloaded binaries. Restrict filesystem/network access when feasible, avoid inheriting unnecessary environment variables, never log secrets, and require explicit user consent for installation. Add provenance/signatures and fine-grained permission declarations before introducing a marketplace. Treat data received from plugins as untrusted input for validation and resource-limit purposes even when plugin installation is trusted.

## 10. Observability

Use `tracing` spans across host/plugin boundaries, propagating correlation and trace IDs. Expose counters for received/emitted/dropped/failed messages, queue depth, processing latency, plugin restarts, node lifecycle transitions, and reconnects. Logs should include flow ID, node instance ID, plugin ID/version, and error class. Do not put high-cardinality payload content into metric labels.

## 11. Repository organization

```text
phlox/
  Cargo.toml
  crates/
    phlox-core/
    phlox-plugin-api/
    phlox-plugin-host/
    phlox-runtime/
    phlox-nodes-basic/
    phlox-cli/
  examples/
    flows/
  docs/
    design.md
    plugin-protocol.md

# Separate repository and release lifecycle:
phlox-mqtt/
  Cargo.toml
  src/
  phlox-plugin.toml
```

`phlox-plugin-api` may eventually be published on crates.io for developer convenience, but **wire-protocol compatibility**, not shared Rust compiler dependencies, is what allows independent releases.

## 12. MVP implementation plan

### Milestone 1 — Core model

Implement flow/node/port/message models, JSON/YAML serialization, graph validation, and an `Inject -> Log` flow using built-in nodes. Deliver unit tests for invalid ports, duplicate IDs, and fan-out.

### Milestone 2 — Runtime

Add node lifecycle, bounded routing, cancellation, startup/shutdown ordering, basic error supervision, and CLI commands: `phlox validate`, `phlox run`, `phlox nodes list`.

### Milestone 3 — External plugin protocol

Implement manifest discovery, version handshake, node-description exchange, create/start/deliver/emit/stop/destroy, health checks, process monitoring, and an **external example plugin**. Include integration tests that kill/restart the plugin process.

### Milestone 4 — phlox-mqtt

Release a separate `phlox-mqtt` repository with subscribe and publish nodes, connection reuse, reconnect handling, secret references, and a sample flow (`MQTT Subscribe -> Log` and `Inject -> MQTT Publish`). Test broker restart, malformed messages, backpressure, and stop/restart.

### Milestone 5 — Editor-ready API

Add stable node metadata and schema endpoints needed by a future GUI, without implementing the GUI itself. Document plugin authoring and semver/compatibility guarantees.

## 13. Acceptance criteria for first useful release

- An independently built `phlox-mqtt` executable is discovered without recompiling the Phlox runtime.
- Node definitions and JSON Schemas are accessible through CLI/API.
- One running flow can subscribe to an MQTT topic and route received data to a built-in log node.
- Another flow can publish to MQTT via a plugin node.
- Multiple MQTT node instances share a broker connection when configured to do so.
- Plugin crash and broker disconnection are observable and do not crash the runtime.
- Stopping a flow frees resources and terminates/reuses plugin processes according to policy.
- Queues are bounded and behavior under congestion is tested.
- Plugin protocol version mismatch produces a readable failure, not undefined behavior.

## 14. Open decisions

| Topic | Initial proposal | Decision needed |
|---|---|---|
| Wire protocol | Length-delimited protobuf over Unix domain sockets | Confirm platform targets and encoding |
| Configuration | JSON Schema and JSON values | Decide schema dialect and secret resolution |
| Flow syntax | YAML with schema version | Consider JSON as canonical wire format |
| Connection reuse | Plugin-managed named resources | Decide resource ownership/scoping |
| Fan-out policy | Bounded queues, block on full | Define per-edge overflow policy later |
| Delivery semantics | At-most-once within runtime | Decide retries/acknowledgements per adapter |
| Plugin packaging | Manifest + executable directory | Decide install paths, update and signing strategy |
| Process sharing | One process per plugin package, multiple node instances | Assess failure isolation requirements |
| Node configuration updates | Restart node instance | Define hot-reconfiguration later |
| Cycles | Allowed with bounded async queues | Add loop detection/warnings |
| Distributed deployment | Out of scope | Reserve identifiers/correlation fields |

## 15. Main risks

1. **Overengineering the plugin contract:** Keep the first protocol minimal and prove it with MQTT before adding more interfaces.
2. **Performance of IPC for high-frequency telemetry:** Benchmark binary payloads, batch delivery, and queue contention; retain native built-ins where useful.
3. **Lifecycle complexity:** MQTT subscriptions, shared connections, reconnects, node restarts, and flow restarts need integration tests.
4. **False delivery guarantees:** Protocol QoS does not automatically imply end-to-end exactly-once semantics.
5. **Schema evolution:** Version plugin protocol and flow schema separately; test compatibility between independently released components.

## 16. First implementation slice

Build the smallest complete vertical slice:

```text
phlox run examples/mqtt-to-log.yaml
  -> load manifest
  -> launch phlox-mqtt
  -> handshake + describe
  -> create/start MQTT subscribe node
  -> receive plugin emit event
  -> route through bounded channel
  -> built-in Log node prints event
  -> Ctrl-C gracefully stops and cleans up
```

This tests the most important architectural claim: **a separately developed protocol plugin can contribute a working node to the Phlox graph without Phlox knowing the protocol.**
