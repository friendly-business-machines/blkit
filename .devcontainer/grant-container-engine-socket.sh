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
  start_proxy() {
    sudo socat "UNIX-LISTEN:$socket,unlink-early,fork,mode=600,user=vscode" "UNIX-CONNECT:$host_socket" &
    proxy=$!
  }
  start_proxy
  for _ in {1..50}; do
    if curl -fsS --noproxy '*' --max-time 1 --unix-socket "$socket" http://localhost/_ping >/dev/null 2>&1; then
      break
    fi
    if ! kill -0 "$proxy" 2>/dev/null; then
      echo "Container engine socket proxy exited during startup" >&2
      exit 1
    fi
    sleep 0.1
  done
  if ! curl -fsS --noproxy '*' --max-time 1 --unix-socket "$socket" http://localhost/_ping >/dev/null 2>&1; then
    echo "Container engine socket proxy did not connect" >&2
    exit 1
  fi

  "$@" &
  command=$!
  trap 'kill "$command" "$proxy" 2>/dev/null || true; wait "$command" "$proxy" 2>/dev/null || true' EXIT
  trap 'exit 143' TERM
  trap 'exit 130' INT
  while true; do
    completed=
    if wait -n -p completed "$command" "$proxy"; then status=0; else status=$?; fi
    if [[ "$completed" == "$command" ]]; then exit "$status"; fi
    if [[ "$completed" != "$proxy" ]]; then exit "$status"; fi
    echo "Container engine socket proxy exited; restarting" >&2
    start_proxy
  done
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
