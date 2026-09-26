#!/usr/bin/env python3
"""Validate a JSONC file with check-jsonschema after removing comments."""
from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path


def without_comments(text: str) -> str:
    out: list[str] = []
    in_string = False
    escaped = False
    in_line_comment = False
    in_block_comment = False
    i = 0
    while i < len(text):
        char = text[i]
        next_char = text[i + 1] if i + 1 < len(text) else ""
        if in_line_comment:
            if char == "\n":
                in_line_comment = False
                out.append(char)
            else:
                out.append(" ")
        elif in_block_comment:
            if char == "*" and next_char == "/":
                in_block_comment = False
                out.extend((" ", " "))
                i += 1
            else:
                out.append("\n" if char == "\n" else " ")
        elif in_string:
            out.append(char)
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
        elif char == '"':
            in_string = True
            out.append(char)
        elif char == "/" and next_char == "/":
            in_line_comment = True
            out.extend((" ", " "))
            i += 1
        elif char == "/" and next_char == "*":
            in_block_comment = True
            out.extend((" ", " "))
            i += 1
        else:
            out.append(char)
        i += 1
    return "".join(out)


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} FILE", file=sys.stderr)
        return 2
    source = Path(sys.argv[1])
    with tempfile.TemporaryDirectory() as directory:
        cleaned = Path(directory) / source.name
        cleaned.write_text(without_comments(source.read_text()), encoding="utf-8")
        return subprocess.run(
            [
                "check-jsonschema",
                "--schemafile",
                "https://raw.githubusercontent.com/devcontainers/spec/main/schemas/devContainer.base.schema.json",
                str(cleaned),
            ],
            check=False,
        ).returncode


if __name__ == "__main__":
    raise SystemExit(main())
