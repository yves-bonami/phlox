#!/bin/sh
set -eu
destination=/var/lib/phlox/plugins/phlox-example/0.2.0
if [ ! -e "$destination" ]; then
    mkdir -p "$destination"
    cp /opt/phlox/example/plugin.toml /opt/phlox/example/libphlox_example_plugin.so "$destination/"
fi
exec "$@"
