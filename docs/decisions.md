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
