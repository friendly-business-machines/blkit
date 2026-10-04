#!/usr/bin/env bash
set -euo pipefail

# Run against an image built from .devcontainer/Dockerfile on Docker Desktop.
image=${1:?Usage: scripts/test-devcontainer-engine-socket.sh IMAGE}
root=$(cd "$(dirname "$0")/.." && pwd -P)
if command -v cygpath >/dev/null; then
  root=$(cygpath -w "$root")
fi
script="$root/.devcontainer/grant-container-engine-socket.sh"
test -f "$script"
MSYS_NO_PATHCONV=1 docker run --rm \
  --mount type=bind,source=/var/run/docker.sock,target=/var/run/docker-host.sock \
  --mount "type=bind,source=$script,target=/tmp/socket-entrypoint.sh,readonly" \
  --entrypoint /bin/bash "$image" /tmp/socket-entrypoint.sh \
  /bin/bash -c 'test "$(id -un)" = vscode && test "$(stat -c %u:%a /var/run/docker.sock)" = 1001:600 && docker version --format "{{.Server.Version}}"'

# Exercise the Podman-style bind mount with a different socket group; usermod
# alone cannot grant an already-running process the new supplementary group.
MSYS_NO_PATHCONV=1 docker run --rm \
  --mount type=bind,source=/var/run/docker.sock,target=/tmp/backend.sock \
  --mount "type=bind,source=$script,target=/tmp/socket-entrypoint.sh,readonly" \
  --user root --entrypoint /bin/bash "$image" -c '
    groupadd -g 2001 socket-fixture
    socat "UNIX-LISTEN:/var/run/docker.sock,fork,mode=660,group=socket-fixture" "UNIX-CONNECT:/tmp/backend.sock" &
    for _ in {1..50}; do
      [[ -S /var/run/docker.sock ]] && break
      sleep 0.1
    done
    sudo -u vscode /bin/bash /tmp/socket-entrypoint.sh docker version --format "{{.Server.Version}}"
  '
