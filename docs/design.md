# Phlox — Technical Architecture (RFC 0001)

Status: Draft architecture aligned with [80 accepted decisions](decisions.md).  
Updated: 2026-11-08.

## Purpose

Phlox is a Rust-based, protocol-agnostic integration engine. Independently developed runtime-installable plugins (for example `phlox-mqtt`) contribute nodes that participate in directed flows. It is *not* intended to reproduce Node-RED one-to-one. The library/runtime is the product; CLI, REST API, and future GUI are interfaces.

**Authority:** [decisions.md](decisions.md) is the canonical decision register. [mvp.md](mvp.md) defines the narrower first implementation target.

## High-level architecture

```text
CLI / REST (WebSocket later)       YAML (SQLite later)
            |                           |
            +-------------+-------------+
                          |
                     Phlox runtime
           flow manager / graph / routing
                          |
             plugin registry and host ABI
                   /        |         \
             basic nodes  mqtt.so   opcua.so
                          |
                 plugin-owned connections
```

- **phlox-core:** Domain model for UUID-identified flows and nodes, typed message payloads, named ports, metadata, quality and timestamps, configuration validation.
- **phlox-runtime:** Tokio-based graph execution, bounded independent edge queues, configurable backpressure, two-phase startup, shutdown, supervision, in-memory flow variables and cross-flow channels.
- **phlox-plugin-api:** Versioned **C ABI contract** for discovery and execution, opaque host-owned message handles, host callbacks, structured errors, explicit ownership and lifetime rules.
- **phlox-plugin-sdk:** Safe ergonomic Rust traits/wrappers for third-party plugin authors. It translates between native Rust APIs and the C ABI; it must not expose unstable Rust ABI across the dynamic library boundary.
- **phlox-plugin-host:** Manifest discovery, compatibility checks, secure installation staging, `libloading` integration, registration, library lifetime management, active version selection.
- **phlox-cli / phlox-api:** CLI and management REST API; future WebSocket events and graphical editor.

## Plugin mechanics

A plugin is a Linux shared library (`.so`) in an immutable version directory, accompanied by a TOML manifest (ID, version, host ABI constraints and library filename). It can expose several immutable node type definitions, whose configuration and available ports are discoverable via API. JSON Schema describes configuration; output ports may depend on configuration but are resolved before execution.

Phlox scans the plugin directory on startup and watches for atomically completed installs. A **new** compatible plugin can be loaded and activated without restarting. Existing loaded versions are not unloaded or overwritten; choosing another installed version, updating, and rolling back take effect after host restart. Only one version of a given plugin is active globally. Plugins have no plugin-to-plugin dependencies, but can use external Rust crates.

The plugin developer contract must contain a C-compatible entrypoint and a versioned function table or equivalent stable API. Specify allocation/freeing responsibilities, callback lifetime, panic containment boundaries, thread synchronization, error codes, and ABI feature negotiation *before implementation*. Host-owned opaque message handles avoid requiring plugins to understand Rust allocation layouts. Plugins manage their own async execution runtimes initially. Host callbacks perform quick enqueue attempts and return status; optionally an async wait method provides backpressure without tying up protocol callback threads.

Native plugins run with Phlox's permissions and can crash the host. Only trusted plugins should be installed. SHA-256 verifies downloaded content *only if* the expected digest comes from a trusted source; signing and sandboxing are future work.

## Node and connection model

Each plugin may provide multiple node types (source, processor, sink), with multiple named inputs, static/configuration-dependent outputs, and optional type constraints. Phlox validates known port types and allows generic ports. Basic transformations are built-in; a future language-independent WASM execution plugin can host scripts compiled from multiple languages.

Nodes are instantiated per flow start and destroyed per flow stop. In the MVP, a node processes messages sequentially and can keep ephemeral internal state. Nodes may define a stop/restart/error-output failure policy; failed messages are dropped by default, with optional error/dead-letter routing. No automatic delivery retry is promised.

Connection configuration can be globally reusable or flow-scoped. Flow-local names may shadow global names; explicit scoped references should disambiguate them. Protocol plugins manage pooling, lazy connect by default, optional keep-alive, reconnection/backoff, and config changes. Phlox standardizes only lifecycle and basic health; protocol-specific capabilities remain in the plugin.

## Typed message model

Messages have an ID, typed payload (primitive, structured, or binary), optional source timestamp, required UTC receipt timestamp, optional quality (Good/Uncertain/Bad), and typed extensible metadata with namespaced protocol keys. Original quality codes and protocol details remain available in metadata. Reference-counting may be used within the host, but must not leak Rust layouts into the C ABI.

Messages move through bounded asynchronous edge queues, preserving ordering within an edge. Edges can block (default), drop-oldest, or drop-newest. A slow edge can still propagate backpressure to its producer; independent queues are not full failure isolation. MVP delivery is **at-most-once**; replay, durable message storage, and at-least-once delivery are future capabilities.

## Flow semantics

A flow is a directed graph with stable UUID node IDs and named ports. Cycles are allowed with validation warnings and runtime safeguards; bounded buffers alone are insufficient to prevent infinite loops or deadlock. Multiple flows execute independently and may communicate using dedicated internal flow-input/output nodes. Flow-scoped variables are in-memory, typed, and reset on restart.

Starting a flow follows a two-phase lifecycle: prepare instances, resources, and queues, then activate processing and sources. Graceful shutdown stops producers and drains within a deadline; immediate shutdown is configurable. On deployment of changed configuration, the first implementation restarts the *whole affected flow*; selective node restarts may follow.

Flow definitions are YAML first, explicitly schema-versioned. Deployment is configurable (explicit by default in production, automatic in development); edits to drafts do not change the deployed flow until activation. Later SQLite stores managed flow revisions and runtime configuration, without replacing import/export. Templates are parameterized, versioned, and create independent instances, with updates explicitly approved. Folders organize flows but do not isolate their execution.

## Operations and external integration

CLI and REST manage flow/plugin lifecycle and validation. WebSocket events and a GUI come later. API-key authentication is planned for MVP (disabled only when explicitly selected for local development); OIDC and RBAC are future scope. Application configuration uses TOML and environment overrides; secrets use named references to environment variables or mounted files initially, encrypted storage later.

The engine exposes status, structured logs, error counts, message counters, and queue diagnostics. Optional bounded per-flow message tracing is useful for debugging but should redact sensitive data. OpenTelemetry exporters are future work. Importing flows with missing dependencies is allowed in an inactive/invalid state until references are fixed, with interactive mapping later.

## Implementation boundaries and validation

The full decision set describes intended eventual behavior. **Do not implement every feature as part of the first milestone.** See [mvp.md](mvp.md) for staged acceptance criteria.

Important unresolved technical specifications: binary ABI layout, ABI change/version policy, message handle ownership, callback wake/backpressure semantics, transactionality of installs, cycle deadlock mitigation, structured error representation, metadata/value layout, and secure external installation provenance. These require implementation RFCs or narrow prototypes rather than more broad feature-choice questions.
