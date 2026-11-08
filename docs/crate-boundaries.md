# Crate boundaries

These are the intended compile-time dependencies. They provide a starting structure, not a mandate to add abstractions before they are useful.

```text
phlox-core                  phlox-plugin-api
    ^                             ^
    |                             |
    |                      phlox-plugin-sdk  (for external authors)
    |                             |
    +----- phlox-plugin-host <-----+ (ABI only, not SDK)
                 ^
                 |
          phlox-runtime -----> phlox-core
                 ^
                 |
             phlox-api ------> phlox-core / phlox-plugin-host
                 ^
                 |
             phlox-web

phlox-cli ---> phlox-runtime / phlox-plugin-host
```

- **core:** Pure domain types and validation; no I/O, runtime, or plugin dependencies.
- **plugin-api:** C-compatible ABI declarations only; **never** depend on core or Rust-specific types across the ABI.
- **plugin-sdk:** Convenience layer for separately compiled plugin authors; depends only on plugin-api, not on the host/runtime.
- **plugin-host:** Native plugin discovery, loading, and lifecycle; translates the ABI into host-owned domain representations. Depends on core and plugin-api, not SDK.
- **runtime:** Flow execution and orchestration; depends on core and plugin-host. Does not depend on any UI or transport.
- **api:** Management HTTP API layer; depends on runtime, core, and plugin-host. No UI templates.
- **web:** Browser-facing server-rendered interface; uses the API crate and contains HTML/templates/static assets. No direct access to runtime internals.
- **cli:** Executable entrypoint; initially orchestrates runtime/plugin-host directly. An optional remote management client can be added later.

The graph must remain acyclic. Dependencies on external crates (Tokio, Axum, Askama, libloading, etc.) will be introduced only when the corresponding functionality is implemented.

**Important:** Workspace dependency entries do not by themselves make a crate depend on another. Each crate opts in explicitly through its own Cargo.toml.

See [design.md](design.md) for architectural intent and [mvp.md](mvp.md) for implementation order.
