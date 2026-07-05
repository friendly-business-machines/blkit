#!/bin/sh
# Reconcile the container's docker group GID with the bind-mounted host Docker
# socket GID, so the non-root `vscode` user can talk to the daemon socket
# (Docker-outside-of-Docker) without sudo.
#
# WHY THIS RUNS FROM postStartCommand AND NOT AN ENTRYPOINT
# --------------------------------------------------------
# This must run at container *start* (not build time) because the socket is
# bind-mounted at runtime and its GID varies by host OS. The obvious home for
# start-time logic is an image ENTRYPOINT — but that does NOT work under VS Code
# Dev Containers. Dev Containers defaults `overrideCommand: true` for
# Dockerfile-based configs, which replaces the image's ENTRYPOINT/CMD with its
# own keep-alive command (`while sleep 1 ...`). An ENTRYPOINT baked into the
# image is therefore silently bypassed and never executes.
#
# `postStartCommand` runs on every container start regardless of
# `overrideCommand`, and runs with sudo available, so it is the correct hook for
# this reconciliation. It is wired up in devcontainer.json:
#   "postStartCommand": "sudo /usr/local/bin/reconcile-docker-gid.sh"
#
# When the socket is root-owned (GID 0) — common in some CI/cloud environments
# and Docker Desktop's Linux VM — GID 0 is already taken by the root group, so
# groupmod would collide. The non-root vscode user is never a member of the
# root group, so make the socket world-writable instead; this is the same
# fallback the official docker-outside-of-docker devcontainer feature uses.
SOCK_GID=$(stat -c '%g' /var/run/docker.sock 2>/dev/null || echo 0)
if [ "$SOCK_GID" = '0' ]; then
    chmod 666 /var/run/docker.sock
else
    groupmod -g "$SOCK_GID" docker
fi
