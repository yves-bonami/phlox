# Container development

Run from the repository root:

```sh
LOCAL_UID=$(id -u) LOCAL_GID=$(id -g) docker compose up --build
```

Open http://127.0.0.1:3000. The web server binds to all interfaces **inside**
the container, but Compose publishes only to host localhost.

The source directory is bind-mounted. Rust dependencies and compilation outputs
use named Docker volumes, and installed plugins persist in `.dev/plugins`,
which is ignored by Git. To rebuild after modifying Rust or Askama templates,
restart the container with `docker compose down && docker compose up` (not
`docker compose restart`, since a restarted binary command must recompile).
Static CSS/JS files are read from the bind mount on each request.

There is no file watcher or hot-reload service.

`PHLOX_PLUGIN_DIR` is `/var/lib/phlox/plugins` in the container; native
plugin libraries must be compiled for the container's Linux architecture.
Podman Compose can use the same file when available.
