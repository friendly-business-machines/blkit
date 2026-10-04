#!/usr/bin/env bash
set -euo pipefail
shopt -s nullglob dotglob
cd "$(dirname "$0")/.."

had_agents=0
[[ -d .agents ]] && had_agents=1
[[ ! -L .agents && ( ! -e .agents || -d .agents ) ]] || { echo '.agents is not a directory or is a symlink' >&2; exit 1; }
[[ ! -L .agents/skills && ( ! -e .agents/skills || -d .agents/skills ) ]] || { echo '.agents/skills is not a directory or is a symlink' >&2; exit 1; }

declare -A before=()
for skill in .agents/skills/*; do
    before["${skill##*/}"]=1
done

cargo agents sync

for skill in .agents/skills/*; do
    name=${skill##*/}
    [[ ${before[$name]+yes} ]] && continue
    [[ -d "$skill" && ! -L "$skill" && -f "$skill/SKILL.md" ]] || {
        printf 'Unexpected skill entry; left in place: %s\n' "$skill" >&2
        exit 1
    }
    dest=.pi/skills/symposium/$name
    if [[ -e "$dest" || -L "$dest" ]]; then
        [[ -d "$dest" && ! -L "$dest" ]] || {
            printf 'Not a skill directory: %s\n' "$dest" >&2
            exit 1
        }
        if cmp -s -- "$skill/SKILL.md" "$dest/SKILL.md"; then
            rm -r -- "$skill"
            continue
        fi
        printf 'Replace %s? [y/N] ' "$dest" >&2
        if ! read -r answer; then
            printf 'No answer; downloaded skill left in %s\n' "$skill" >&2
            exit 1
        fi
        if [[ $answer != [yY] && $answer != [yY][eE][sS] ]]; then
            rm -r -- "$skill"
            continue
        fi
        rm -f -- "$skill/.gitignore"
        backup=$(mktemp -d .pi/skills/symposium/.symposium-backup.XXXXXX)
        mv -- "$dest" "$backup/skill"
        if ! mv -- "$skill" "$dest"; then
            mv -- "$backup/skill" "$dest"
            rmdir -- "$backup"
            exit 1
        fi
        rm -r -- "$backup"
    else
        mkdir -p -- .pi/skills/symposium
        rm -f -- "$skill/.gitignore"
        mv -- "$skill" "$dest"
    fi
done

if (( ! had_agents )); then
    if ! rmdir -- .agents/skills .agents 2>/dev/null; then
        echo 'Left nonempty .agents in place for inspection' >&2
    fi
fi
