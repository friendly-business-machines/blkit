#!/usr/bin/env bash
set -euo pipefail

script=$(cd "$(dirname "$0")" && pwd)/fetch-symposium-skills.sh
root=$(mktemp -d)
trap 'rm -rf -- "$root"' EXIT
mkdir -p "$root/bin"
cat > "$root/bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1 $2" == 'agents sync' ]]
[[ "$PWD" == "$PROJECT" ]]
mkdir -p .agents/skills
cp -R "$FIXTURE"/. .agents/skills/
[[ "${SYNC_FAIL:-}" != 1 ]]
EOF
chmod +x "$root/bin/cargo"
export PATH="$root/bin:$PATH"

setup() {
    PROJECT="$root/$1"
    FIXTURE="$root/fixture-$1"
    export PROJECT FIXTURE
    mkdir -p "$PROJECT/scripts" "$FIXTURE/new-skill"
    cp "$script" "$PROJECT/scripts/fetch-symposium-skills.sh"
    printf 'downloaded\n' > "$FIXTURE/new-skill/SKILL.md"
    printf '*\n' > "$FIXTURE/new-skill/.gitignore"
    : > "$FIXTURE/new-skill/.symposium"
    cd "$PROJECT"
}

# A fresh sync moves the skill, then removes only the directories it created.
setup fresh
bash scripts/fetch-symposium-skills.sh
[[ $(<.pi/skills/symposium/new-skill/SKILL.md) == downloaded ]]
[[ ! -e .pi/skills/symposium/new-skill/.gitignore ]]
[[ -f .pi/skills/symposium/new-skill/.symposium ]]
[[ ! -e .agents ]]

# A pre-existing .agents tree and skill remain intact; only new names move.
setup existing
mkdir -p .agents/skills/keep
printf 'original\n' > .agents/skills/keep/SKILL.md
bash scripts/fetch-symposium-skills.sh
[[ $(<.agents/skills/keep/SKILL.md) == original ]]
[[ $(<.pi/skills/symposium/new-skill/SKILL.md) == downloaded ]]

# Matching content needs no prompt; even bundled local files stay unchanged.
setup identical
mkdir -p .pi/skills/symposium/new-skill
printf 'downloaded\n' > .pi/skills/symposium/new-skill/SKILL.md
printf 'local\n' > .pi/skills/symposium/new-skill/local.txt
bash scripts/fetch-symposium-skills.sh </dev/null
[[ $(<.pi/skills/symposium/new-skill/local.txt) == local ]]
[[ ! -e .agents ]]

# Refusal preserves the old skill; approval replaces the directory as a unit.
setup declined
mkdir -p .pi/skills/symposium/new-skill
printf 'old\n' > .pi/skills/symposium/new-skill/SKILL.md
printf 'local\n' > .pi/skills/symposium/new-skill/local.txt
printf 'n\n' | bash scripts/fetch-symposium-skills.sh
[[ $(<.pi/skills/symposium/new-skill/SKILL.md) == old ]]
[[ -e .pi/skills/symposium/new-skill/local.txt ]]
[[ ! -e .agents ]]
setup approved
mkdir -p .pi/skills/symposium/new-skill
printf 'old\n' > .pi/skills/symposium/new-skill/SKILL.md
printf 'y\n' | bash scripts/fetch-symposium-skills.sh
[[ $(<.pi/skills/symposium/new-skill/SKILL.md) == downloaded ]]
[[ ! -e .pi/skills/symposium/new-skill/.gitignore ]]
[[ -f .pi/skills/symposium/new-skill/.symposium ]]
[[ ! -e .agents ]]

# A failed sync leaves its output for inspection rather than deleting it.
setup failed
if SYNC_FAIL=1 bash scripts/fetch-symposium-skills.sh; then exit 1; fi
[[ -e .agents/skills/new-skill/SKILL.md ]]

# An existing symlink must not redirect sync into somebody else's directory.
setup symlink
mkdir -p "$root/shared"
ln -s "$root/shared" .agents
if bash scripts/fetch-symposium-skills.sh; then exit 1; fi
[[ ! -e "$root/shared/skills" ]]

echo 'fetch-symposium-skills: OK'
