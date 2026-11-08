# Phlox — Accepted Architecture Decisions

Status: Accepted design intentions; not yet implemented.
Updated: 2026-11-08.

These decisions reflect the user's answers to questions 1–51 and supersede conflicting proposals in [design.md](design.md), especially its original IPC-first recommendation.

## Plugin platform

- **1** Runtime-installable Linux native .so libraries via stable C ABI and Rust SDK (not sidecars).
- **2** Activate new plugins immediately.
- **3** Local install first, GitHub Releases later.
- **4** JSON Schema configuration, optionally schemars-generated.
- **5** Multiple node types per plugin.
- **14** Stage updates until Phlox restart.
- **15** Versioned API with compatibility checks.
- **16** No cross-plugin dependencies.
- **17** Trusted native plugins run with full host privileges.
- **18** Linux/Docker first.
- **19** Persist plugins in /var/lib/phlox/plugins via volume.
- **20** Startup scan and directory watcher with atomic install visibility.
- **40** Keep native libraries loaded until restart.
- **41** SHA-256 checks, cryptographic signatures later.
- **42** Multiple versions installed, one active.
- **43** Manual rollback first, automatic later.
- **44** Safe uninstall by default; force stops dependents, preserves invalid flows.
- **45** Global plugin installation.

## Nodes and messages

- **7** Typed primitives/structured values/binary/metadata.
- **8** Per-edge bounded asynchronous Tokio channels.
- **9** Per-edge backpressure: block by default, drop-oldest or drop-newest; configurable capacity.
- **10** Per-node errors: stop, restart with backoff (default), or propagate to error output.
- **21** Node configuration changes restart affected node.
- **25** Sequential processing per node first; configurable concurrency later.
- **26** Per-instance in-memory state first; persistence later.
- **27** Multiple named inputs.
- **28** Static and config-derived outputs, resolved before execution.
- **29** Optional declared types and generic ports; conversions explicit.

## Connections

- **6** Named shared connection configurations, plugin-managed.
- **46** Global and per-flow configurations; local shadows global, explicit reference supported.
- **47** Lazy connect by default; optional keep-alive.
- **48** Plugins manage reconnection and inform dependent nodes.
- **49** Basic status first; richer metrics later.
- **50** Plugin-controlled configuration changes: reconnect by default, node restart fallback.
- **51** Minimal shared lifecycle/health interface; plugin-specific operations and schemas.

## Flows and runtime

- **11** YAML first, SQLite and import/export later.
- **12** Multiple independently controllable flows.
- **13** Cycles allowed with validation warnings and runtime safeguards.
- **22** Full flow restart on changes first; selective node restart later.
- **23** Scheduling through nodes (not a Node-RED copy).
- **24** Directed graph of nodes and named ports.
- **30** Full validation before start; incremental validation later.
- **31** Schema versioning first, revision history and rollback later.
- **32** Configurable deployment: automatic in development, explicit by default in production.
- **33** Standard metrics/errors and optional limited payload tracing.
- **34** Built-in logs/status/metrics first, OpenTelemetry later.

## Management and security

- **35** REST management API first, WebSocket later.
- **36** Configurable authentication; API keys initially, local opt-out, OIDC later.
- **37** Single administrator first, RBAC later.
- **38** TOML/environment overrides first, SQLite runtime configuration later.
- **39** Environment and mounted file secret references first; encrypted SQLite later.

## Implementation cautions

- A stable FFI contract needs explicit ownership, threading, error, and ABI version rules. Ordinary Rust trait objects are not stable across separately compiled dynamic libraries.
- Loaded native plugins are trusted; a crash can terminate Phlox even if node-level error recovery exists.
- SHA-256 without an authenticated expected digest is not an authenticity check.
- Updates must use immutable versioned files, not overwrite loaded shared libraries.
- Independent edge queues do not completely isolate upstream producers from backpressure.
- Cycles need safeguards beyond queue limits; deadlocks and infinite message loops remain risks.
- The initial implementation should prove a small end-to-end flow before adding every accepted feature.

## Still to specify

Exact FFI entrypoints, message memory ownership, install atomicity, graph-cycle safeguards, buffer semantics, schema dialect, API network defaults, storage schemas, and detailed test criteria.

## Further accepted decisions (52–80)

These decisions continue the same conversation. **MVP** means accepted direction for the initial implementation, not a promise that all capabilities ship in the first vertical slice.

- **52 — Plugin developer interface:** Documented stable C ABI plus official ergonomic Rust SDK; other languages may implement ABI.
- **53 — Message ownership:** Host-owned buffers with opaque handles, explicit lifetimes; internal reference counting allowed.
- **54 — Threading:** Plugin-managed async runtime for MVP, shared by its node instances; host-managed execution API later.
- **55 — Plugin-to-host events:** Thread-safe host callbacks; callbacks enqueue messages, not execute flow logic inline.
- **56 — Callback backpressure:** Nonblocking status by default; optional async waiting for queue capacity, respecting edge policy.
- **57 — Errors:** Structured error code, category, message and optional plugin-specific details.
- **58 — Plugin initialization:** Lazy by default, configurable eager initialization; distinguish activation from initialization.
- **59 — Metadata:** Static TOML manifest for discovery and ABI constraints, dynamic descriptions/schema from loaded plugin.
- **60 — Node registration:** Immutable set of node types after registration; configured instances may derive output ports.
- **61 — Instance lifetime:** Create on flow start, destroy on flow stop; shared connections can outlive individual flows.
- **62 — Node startup:** Two-phase prepare/activate, with queues/resources ready before sources emit.
- **63 — Shutdown:** Graceful drain by default with deadline, optional immediate shutdown.
- **64 — Delivery semantics:** At-most-once within runtime for MVP; no automatic retries; delivery guarantees may expand later.
- **65 — Processing failures:** Drop and report by default; optional error/dead-letter output without automatic retry.
- **66 — Timestamps:** Optional source timestamp, always-populated received timestamp, UTC; additional timestamps in metadata.
- **67 — Quality:** Optional Good/Uncertain/Bad with original protocol status retained in metadata; absent is unspecified.
- **68 — Metadata:** Flexible typed key/value metadata, standardized common keys and namespaced protocol-specific keys.
- **69 — Transformations:** Common built-in transformation nodes first; scriptable transformations via external plugin later.
- **70 — Scripting:** Future language-agnostic WebAssembly engine for multiple compile-to-WASM languages; runtime choice deferred.
- **71 — Variables:** In-memory flow-scoped typed variables for MVP; global variables later; node-local state separately.
- **72 — Variable persistence:** In-memory only for MVP; reset on Phlox restart; persistence deferred.
- **73 — Flow failures:** Isolate recoverable failures to affected flow; native crashes can still bring down process.
- **74 — Inter-flow messages:** Dedicated internal input/output nodes, no network broker needed; subflows later.
- **75 — Flow startup dependencies:** Independent startup for MVP; optional explicit dependency-aware startup later, reject dependency cycles.
- **76 — Templates:** Parameterized reusable flow templates generate independent flow instances.
- **77 — Template changes:** Do not alter existing instances automatically; track template version and opt-in validated updates.
- **78 — Organization:** Hierarchical folders are organizational only, not execution/security boundaries.
- **79 — Identity:** Stable UUIDs for flows and nodes; names and paths may change.
- **80 — Import/export:** Allow import with unresolved dependencies but prevent deployment; interactive mapping later.

## Precedence

This register is the canonical record of accepted decisions (1–83). Architecture and MVP documents should follow it. Implementation-level details not explicitly decided remain open; don't interpret a proposed mechanism as an accepted requirement.

## Frontend architecture (subsequent decision)

- **81 — Frontend stack:** Build the MVP frontend with Axum, Askama, HTMX, and _hyperscript. Avoid React/Angular as primary frameworks.
- **82 — Visual editor:** Use an SVG graph editor; small, focused vanilla JavaScript is explicitly allowed for dragging, zooming, panning, and wiring when _hyperscript alone is impractical. Server remains authoritative for graph validation.
- **83 — Frontend timing:** Include editor scaffolding in Milestone 0 and a usable visual editor in the initial MVP, rather than deferring all UI work until after protocol support.

The frontend uses the same graph and node metadata contracts as the runtime. Fancy canvas features and advanced live monitoring may follow later.
