#!/usr/bin/env bash
set -euo pipefail

# Dev Containers overrides a Dockerfile ENTRYPOINT unless overrideCommand is
# false. Run here before the VS Code server starts: Linux processes inherit
# their groups at launch, so usermod in postStartCommand cannot grant an
# already-running server access to the mounted socket.
# Podman's socket GID belongs to the host and may vary across machines. Access
# to that group grants control of host Podman; never make it world-writable.
socket=/var/run/docker.sock
host_socket=/var/run/docker-host.sock

if [[ -S "$host_socket" ]]; then
  # Docker Desktop presents a root:root socket. Forward it without changing
  # host permissions or granting vscode membership in the root group.
  sudo socat "UNIX-LISTEN:$socket,fork,mode=600,user=vscode" "UNIX-CONNECT:$host_socket" &
  proxy=$!
  for _ in {1..50}; do
    if [[ -S "$socket" && -w "$socket" ]]; then
      exec "$@"
    fi
    if ! kill -0 "$proxy" 2>/dev/null; then
      echo "Container engine socket proxy exited" >&2
      exit 1
    fi
    sleep 0.1
  done
  echo "Container engine socket proxy did not start" >&2
  exit 1
fi

if [[ ! -S "$socket" ]]; then
  echo "Container engine socket not found at $socket" >&2
  exit 1
fi

# Podman normally maps the host socket owner to vscode with keep-id.
if [[ ! -r "$socket" || ! -w "$socket" ]]; then
  gid=$(stat -c %g "$socket")
  if [[ "$gid" == 0 ]]; then
    echo "Refusing to add vscode to the root group for $socket" >&2
    exit 1
  fi
  group=$(getent group "$gid" | cut -d: -f1 || true)
  if [[ -z "$group" ]]; then
    group="container-engine-socket-$gid"
    sudo groupadd -g "$gid" "$group"
  fi
  sudo usermod -aG "$group" vscode
  # Existing processes keep their supplementary groups; start the command
  # with freshly initialized groups instead of assuming usermod updates them.
  exec sudo -E setpriv --reuid "$(id -u)" --regid "$(id -g)" --init-groups "$@"
fi

# Keep the image CMD running so the container stays alive for VS Code to attach.
exec "$@"
