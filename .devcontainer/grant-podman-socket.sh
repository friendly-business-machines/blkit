#!/usr/bin/env bash
set -euo pipefail

# Dev Containers overrides a Dockerfile ENTRYPOINT unless overrideCommand is
# false. Run here (before the VS Code server starts), not in postStartCommand:
# Linux processes inherit their groups at launch, so usermod in a later hook
# cannot grant the already-running server access to the mounted host socket.
# The socket GID belongs to the host and may differ across machines. Access to
# this group grants control of host Podman; never make the socket world-writable.
socket=/var/run/docker.sock
if [[ ! -S "$socket" ]]; then
  echo "Podman socket not found at $socket" >&2
  exit 1
fi

gid=$(stat -c %g "$socket")
if [[ "$gid" == 0 ]]; then
  echo "Refusing to add vscode to the root group for $socket" >&2
  exit 1
fi
group=$(getent group "$gid" | cut -d: -f1 || true)
if [[ -z "$group" ]]; then
  group="podman-socket-$gid"
  sudo groupadd -g "$gid" "$group"
fi
sudo usermod -aG "$group" vscode

# Keep the image CMD running so the container stays alive for VS Code to attach.
exec "$@"
