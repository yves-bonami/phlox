# Container development

Start Phlox:

```sh
docker compose up --build -d
```

Open http://127.0.0.1:3000. The server binds to all interfaces inside the
container, but Compose exposes the port only on localhost.

The image includes `phlox-example` v0.7.0, which is copied into the
persistent `plugin-data` volume at startup if that version is absent.
Existing installed packages are not overwritten.

Use the **Install plugin** form in the UI to upload a trusted `plugin.toml`
and matching Linux `.so`. New plugin IDs are discovered automatically;
an already-installed version cannot be replaced while running. Native
plugins execute with the server's privileges. Do not expose this unauthenticated
prototype outside localhost.

Source files are bind-mounted. Cargo registry and build artifacts use named
volumes. No file watcher or hot reload is enabled. After Rust or Askama
template changes, use `docker compose restart phlox` to rerun `cargo run`.
Static assets are served directly from the bind mount.

Use `docker compose down` to stop Phlox without deleting plugin storage.
Avoid `docker compose down -v` unless you intend to delete installed plugins.
