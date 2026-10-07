#!/usr/bin/env bash
set -euo pipefail

root=$(mktemp -d)
entry_pid=
backend_pid=
cleanup() {
  [[ -z "$entry_pid" ]] || kill "$entry_pid" 2>/dev/null || true
  [[ -z "$backend_pid" ]] || kill "$backend_pid" 2>/dev/null || true
  [[ -z "$entry_pid" ]] || wait "$entry_pid" 2>/dev/null || true
  [[ -z "$backend_pid" ]] || wait "$backend_pid" 2>/dev/null || true
  rm -rf "$root"
}
trap cleanup EXIT

mkdir "$root/bin"
printf '#!/bin/sh\nexec "$@"\n' > "$root/bin/sudo"
chmod +x "$root/bin/sudo"
sed -e "s@/var/run/docker-host.sock@$root/docker-host.sock@g" \
    -e "s@/var/run/docker.sock@$root/docker.sock@g" \
    "$(dirname "$0")/grant-container-engine-socket.sh" > "$root/entry.sh"

# A bind()ed socket left behind without a listener reproduces the broken restart.
python3 - "$root/docker.sock" <<'PY'
import socket, sys
s = socket.socket(socket.AF_UNIX)
s.bind(sys.argv[1])
s.close()
PY
python3 -u - "$root/docker-host.sock" <<'PY' &
import socket, sys
s = socket.socket(socket.AF_UNIX)
s.bind(sys.argv[1])
s.listen()
while True:
    connection, _ = s.accept()
    with connection:
        connection.recv(4096)
        connection.sendall(b'HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK')
PY
backend_pid=$!
for _ in {1..50}; do
  [[ ! -S "$root/docker-host.sock" ]] || break
  sleep 0.1
done

PATH="$root/bin:$PATH" bash "$root/entry.sh" sleep 60 >"$root/entry.log" 2>&1 &
entry_pid=$!
ping_proxy() {
  [[ $(curl -fsS --max-time 1 --unix-socket "$root/docker.sock" http://localhost/_ping 2>/dev/null) == OK ]]
}
for _ in {1..50}; do
  if ping_proxy; then break; fi
  sleep 0.1
done
if ! ping_proxy; then
  printf 'stale socket was not replaced:\n' >&2
  tail -15 "$root/entry.log" >&2
  exit 1
fi

old_proxy=$(pgrep -P "$entry_pid" -x socat | head -1)
kill "$old_proxy"
for _ in {1..50}; do
  new_proxy=$(pgrep -P "$entry_pid" -x socat | head -1 || true)
  if [[ -n "$new_proxy" && "$new_proxy" != "$old_proxy" ]] && ping_proxy; then
    printf 'socket proxy recovered after exit\n'
    exit 0
  fi
  sleep 0.1
done
printf 'socket proxy did not restart:\n' >&2
tail -15 "$root/entry.log" >&2
exit 1
