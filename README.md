# Phlox

An extensible, Rust-first IIoT client library with a CLI frontend and a possible future GUI.

## Workspace

- `phlox-sdk`: public contracts for independent, third-party protocol implementations.
- `phlox-core`: protocol-independent registry and client lifecycle (not implemented yet).
- `phlox`: library facade, currently re-exporting core and SDK.
- `phlox-cli`: minimal command-line entry point.

No protocols, registry, session abstractions, or plugin loader are implemented yet. External protocol crates will depend on `phlox-sdk`, with explicit registration as the initial integration model. Runtime plugin loading is a future design decision.

## Development

```sh
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p phlox-cli
```
