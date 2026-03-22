#!/usr/bin/env bash
set -euo pipefail

print_version() {
  local label="$1"
  local binary="$2"
  shift 2

  if command -v "$binary" >/dev/null 2>&1; then
    local first_line
    first_line="$({ "$binary" "$@"; } 2>&1 | head -n 1)"
    printf "%-12s %s\n" "$label" "$first_line"
  else
    printf "%-12s %s\n" "$label" "not found"
  fi
}

echo "Tool versions"
echo "-------------"
print_version "git" git --version
print_version "gh" gh --version
print_version "python" python3 --version
print_version "go" go version
print_version "rust" rustc --version
print_version "node" node --version
print_version "typescript" tsc --version
print_version "dotnet" dotnet --version
print_version "java" java --version
