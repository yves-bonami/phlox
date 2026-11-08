# Phlox — MVP Implementation Plan

Status: Proposed implementation sequence based on [accepted decisions](decisions.md).  
Scope: small vertical slice first, with subsequent MVP increments. No plugin code has been implemented by this document.

## Core principle

The first technical proof must show: **Phlox can load a separately compiled Linux native plugin at runtime, discover a node, and route a typed message through a flow without recompiling the host.** In parallel, the MVP must establish a minimal usable **visual editor** from the outset.

Avoid a polished marketplace, SQLite persistence, full transformation catalog, or broad protocol support initially. The editor itself is not deferred.

## Milestone 0 — Repository and test scaffolding

- Rust workspace with `phlox-core`, `phlox-runtime`, `phlox-plugin-api`, `phlox-plugin-sdk`, `phlox-plugin-host`, and `phlox-cli`.
- CI for `cargo fmt --check`, `cargo clippy`, and tests.
- Conventional Commits.
- Minimal `README.md` and example configuration.
- Scaffold `phlox-web` using Axum + Askama + HTMX + _hyperscript; serve one web page and static assets from Rust.
- Create an early SVG-based editor prototype (two nodes, draggable positioning, link creation) using minimal vanilla JavaScript where _hyperscript becomes unwieldy.
- Unit tests for IDs, graph validation, and basic typed values.

**Exit:** A clean workspace builds and tests on Linux; a browser displays an editable two-node canvas (in-memory only).

## Milestone 1 — Native plugin vertical slice

- Define a *minimal* versioned C ABI and written ownership contract.
- Provide safe SDK wrappers for plugin registration, node construction and event emission.
- Load a locally installed `.so` using `libloading`.
- Read a manifest; validate ID and ABI version; register immutable node definitions.
- Host-allocated/owned opaque message handles (initial implementation may copy payloads through a safe host function).
- Implement a test plugin that emits one typed value; built-in log sink receives it through an in-memory queue.
- Start and stop a simple flow through CLI.

**Exit:** Build a plugin separately, install it while Phlox is running, activate it, and verify a message reaches the log sink **without rebuilding or restarting Phlox**. The frontend can show the discovered node type. Add negative tests for incompatible ABI and malformed manifest.

## Milestone 2 — Reliable flow runtime foundations

- Multiple independent flows, two-phase prepare/activate startup, graceful/immediate shutdown.
- Configurable bounded edge queues with block/drop-oldest/drop-newest and metrics.
- Sequential per-node processing; named multiple inputs, static and config-derived outputs.
- Source/received UTC timestamps, quality and typed metadata.
- At-most-once error behavior, optional error output, configurable node restart policy.
- Flow graph/YAML validation, schema versioning, and explicit deployment defaults.
- Hook the SVG editor to the same graph definitions, enable configuration forms from schema, save/validate/deploy flows using HTMX responses.
- Test fan-out, FIFO per edge, congestion, stop/restart, cycles (including behavior under full buffers), and failed plugin calls.

**Exit:** Graph behavior and lifecycle have deterministic automated tests; there is no promise of native crash isolation.

## Milestone 3 — First real third-party protocol

- Create **independent** `phlox-mqtt` repository using only the documented ABI/SDK.
- MQTT subscribe and publish nodes and JSON Schema metadata.
- Plugin-owned shared named connections; lazy connect/reconnection and basic health.
- Phlox install/activate from local binary directory; versioned immutable packages.
- Example flows: MQTT -> Log, Inject -> MQTT.
- Container deployment with mounted persistent plugin directory.

**Exit:** Two node instances can reuse an MQTT connection, recover from broker restart, and transfer typed data end-to-end. Updating installed plugin versions requires a Phlox restart.

## Milestone 4 — Small operator-friendly release

- REST management endpoints (with safe networking defaults/API-key auth), plugin/flow status.
- Complete the first useful server-rendered UI pages: plugin install/status, connections, flow list/editor, start/stop/deploy, basic node health and message activity.
- Keep SVG graph interactions in isolated vanilla JS; use HTMX + _hyperscript for the rest. Test browser interactions and server-side validation.
- Global and flow-local connection references; TOML/env configuration and file/env secret references.
- Built-in basic mapping/filter/type conversion, timer/interval, and internal flow channels.
- In-memory flow variables, basic debug tracing with caps, structured logs and queue metrics.
- Manual plugin-version selection/rollback, safe plugin removal, unresolved import dependency reporting.
- Basic parameterized flow templates and folder organization, if needed for initial user scenarios.

**Exit:** Install, configure, start, monitor, and stop MQTT integration flows in a Docker container without rebuilding Phlox.

## Deferred deliberately

GitHub Releases installation, SQLite managed configuration/history, selective node restarts, persistent state/variables, advanced real-time WebSocket/SSE monitoring, complex visual editor features (multi-select, undo/redo, subflow editing), WASM scripting engine, OIDC/RBAC, full OTEL export, robust plugin signing, multi-platform builds, external plugin sandboxing, durable delivery, and complex scheduling/dependency management.

## Prioritized technical questions to answer through code

1. Which exact C ABI entrypoint and function-table signatures offer safe compatibility?
2. How do host-owned message handles and callbacks enforce lifetime and thread safety?
3. Can `phlox-mqtt` use a plugin-owned Tokio runtime with safe shutdown?
4. How can a loaded version remain immutable while new packages install atomically?
5. What are the tested semantics of a cyclic flow with bounded queues and block policy?

## Minimum proof-of-concept flow

```text
Separately compiled phlox-example.so
       |
       | emit typed message (host callback)
       v
      Phlox edge queue
       |
       v
    Built-in log node
```

A successful run should report plugin version, loaded ABI version, flow and node UUIDs, message value, and clean shutdown, with CI covering invalid plugin versions and callback misuse where feasible.

## Async execution prototype

The editor now starts an in-memory Tokio local-task runner. Each source polls once per configurable interval (50–60000 ms), and each connection uses a bounded 64-message channel with drop-and-log behavior on overflow. Stop terminates the run, and the next Start joins the previous worker. This is a single-threaded proof; arbitrary processor nodes, production lifecycle handling, and delivery guarantees remain future milestones.
