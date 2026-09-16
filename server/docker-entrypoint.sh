#!/bin/sh
set -e

# `make docker-run` bind-mounts the crate config here.
if [ -z "${EINK_CONFIG:-}" ] && [ -f /app/config.toml ]; then
  EINK_CONFIG=/app/config.toml
fi

# Compose persists config + Meross/weather caches under /data.
if [ -z "${EINK_CONFIG:-}" ]; then
  EINK_CONFIG=/data/config.toml
fi

mkdir -p /data /data/pictures "$(dirname "$EINK_CONFIG")"

if [ -d "$EINK_CONFIG" ]; then
  echo "eink-frame: $EINK_CONFIG is a directory." >&2
  echo "Docker creates one when the host path is missing. Remove it, copy config.example.toml to that path, and retry." >&2
  exit 1
fi

if [ ! -f "$EINK_CONFIG" ]; then
  echo "eink-frame: no config at $EINK_CONFIG — seeding demo defaults from config.example.toml"
  echo "eink-frame: edit the file on the host and restart to use live calendar / to-dos / weather"
  cp /app/config.example.toml "$EINK_CONFIG"
fi

export EINK_CONFIG

if [ "${1:-}" = "eink-frame" ]; then
  shift
fi

exec eink-frame "$@"
