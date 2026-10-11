#!/usr/bin/env python3
"""Guard the default workspace build against accidentally including demo targets."""
import json
import subprocess

metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"]))
assert len(metadata["workspace_default_members"]) == 1, "default build must select only the CLI package"
cli = next(p for p in metadata["packages"] if p["id"] == metadata["workspace_default_members"][0])
assert all("custom-build" not in t["kind"] for t in cli["targets"]), "CLI build must not run the example build script"
assert [t["name"] for t in cli["targets"] if "bin" in t["kind"]] == ["blkit"], "default package must build only blkit"
