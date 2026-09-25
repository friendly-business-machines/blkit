#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

for dockerfile in "$@"; do
  case "$dockerfile" in
    .devcontainer/Dockerfile)
      context="."
      ;;
    *)
      context="$(dirname "$dockerfile")"
      ;;
  esac

  echo "Building $dockerfile (context: $context)"
  DOCKER_BUILDKIT=1 docker build --file "$dockerfile" "$context"
done
