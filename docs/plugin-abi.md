# Native plugin ABI — draft v1 discovery contract

The first ABI slice supports **discovery and node-type registration only**.
Node creation, execution, host-owned messages, async callbacks and teardown
are deliberately **not yet defined**. No claim of stable public compatibility
is made until an end-to-end host/plugin prototype is validated.

## Export

Linux plugins export `phlox_plugin_entry` with the exact `PluginEntry` C ABI
signature in `phlox-plugin-api`. The host resolves the symbol using
`libloading` (later step) and supplies ABI version 1. The plugin sets the
out-pointer to a plugin-owned, static, immutable `PluginApi` table, or
returns a non-success status. The host validates null pointers, ABI version,
and `struct_size` **before reading table fields**.

## Registration

The host provides `HostApi` (version 1), with a context pointer and
`register_node_type` callback. Calling `PluginApi.register` allows a plugin
to advertise UTF-8 type IDs and JSON Schema bytes. The host copies both
byte spans during the callback. Duplicate identifiers and invalid schemas
are rejected by the host; plugin registration must propagate failures.

## Ownership and threading

- `ByteSpan` never transfers ownership. The producer keeps memory valid
  until the callback returns; plugin ID/version spans live until unload.
- Host owns its callback context; it stays valid for the whole registration
  call. Plugins must never retain it or call registration asynchronously.
- Both sides must keep their own allocations and never free each other's.
- Callbacks and entrypoints must not unwind across the C ABI boundary.
  Rust SDK wrappers must catch panics where supported and translate to a
  status; panic=abort and native faults remain process-fatal.
- Only trusted native libraries should be loaded: these APIs do not provide
  sandboxing or crash isolation.

## Versioning

`abi_version` is a **major** ABI number. A mismatch rejects loading;
future append-only additions can use `struct_size` checks. Do not change
existing field order or semantics in a published major version. The host
must reject missing mandatory callback/function pointers. Layout and calling
conventions must be tested across separately compiled host and plugin.

## Limits of this slice

No guarantees yet for multi-threaded callback use, plugin update/unload,
message ownership, instance lifetimes, or runtime execution. Those require
a separate contract and implementation tests before adding functions.

## Prototype verification

Build the example plugin separately:

```sh
cargo build --manifest-path examples/native-plugin/Cargo.toml
cargo run -p phlox-plugin-host --example inspect -- examples/native-plugin/target/debug/libphlox_example_plugin.so
```

This demonstrates synchronous discovery. It is **not** yet runtime
installation, manifest validation, directory watching, or node execution.

## Local versioned installation (prototype)

`PluginRegistry` installs a trusted local package containing `plugin.toml`
and its named `.so`. It copies files to a staging directory and publishes
`<plugin-root>/<id>/<version>/` with a rename; existing versions are never
overwritten. `refresh()` loads new plugin IDs without restarting the host;
already active plugin versions stay loaded until restart. `watch()` runs an
optional blocking filesystem watcher and notifies the application when new
IDs activate. The management API and the web interface are not yet wired up.

Example on Linux:

```sh
cargo build --manifest-path examples/native-plugin/Cargo.toml
mkdir -p /tmp/phlox-example-package
cp examples/native-plugin/plugin.toml /tmp/phlox-example-package/
cp examples/native-plugin/target/debug/libphlox_example_plugin.so /tmp/phlox-example-package/
cargo run -p phlox-plugin-host --example install -- /tmp/phlox-plugins /tmp/phlox-example-package
```

Native code is **trusted** and executes in the host process. Secure
signatures, cross-filesystem staging, installation concurrency controls,
plugin source authentication, and runtime flow integration remain future work.
