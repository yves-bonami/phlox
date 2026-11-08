# Phlox web

Run the development server with:

```sh
cargo run -p phlox-web
```

Open http://127.0.0.1:3000. The flow editor is currently in-memory only.

## Plugin discovery

Set `PHLOX_PLUGIN_DIR` to the local versioned plugin root (default `$XDG_DATA_HOME/phlox/plugins`, or `~/.local/share/phlox/plugins` when XDG_DATA_HOME is unset. Containers can explicitly use `/var/lib/phlox/plugins`). The server exposes `GET /api/plugins` with currently discovered plugin and node descriptors. New trusted plugin IDs are picked up by the watcher without server restart. This read-only API is a prototype and has no authentication; keep the server bound to localhost.
