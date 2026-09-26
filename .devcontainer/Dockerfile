# If you modify this file, verify it still builds before committing:
#   docker build -f .devcontainer/Dockerfile .
# See AGENTS.md for details.

# Base image:
# - Ubuntu 26.04 LTS (Resolute Raccoon) was released in Apr2026.
# - Standard LTS support runs through April 2031.
# - Base image size is approx 40mb
FROM ubuntu:26.04

# Use bash with pipefail for all RUN instructions. Without this, a `curl | sh`
# style pipe only reports the exit code of the last command, so a failed
# download would silently succeed the build.
SHELL ["/bin/bash", "-o", "pipefail", "-c"]

# Build args for the default non-root dev user.
# - Microsoft Dev Container base images commonly use `vscode` with UID/GID 1000.
# - UID/GID 1001 is used instead of the more common 1000 because Ubuntu 26.04's
#   base image already ships a built-in `ubuntu` user/group at UID/GID 1000.
ARG USERNAME=vscode
ARG USER_UID=1001
ARG USER_GID=${USER_UID}

# Install tools and language runtimes/SDKs.
#
# Utility/runtime package rationale:
# - curl: commonly used in scripts to download files.
# - wget: commonly used in scripts to download files.
# - fish: friendly interactive shell, available as an alternative to bash for
#   terminals (bash remains the container's default login/non-interactive shell).
# - gh: GitHub CLI for repository, PR, issue, and auth workflows.
# - git: source control operations.
# - gnupg: `gpg` tooling for signing, encryption, and use with `pass`.
# - jq: JSON parsing in shell scripts.
# - less: terminal pager for viewing file output.
# - libatomic1: shared library (libatomic.so.1) required by the prebuilt
#   Node runtime that pre-commit downloads for its node-language hooks
#   (e.g. markdownlint). Without it those hooks fail to start with
#   "error while loading shared libraries: libatomic.so.1".
# - nano: lightweight terminal text editor.
# - nodejs: Node.js runtime required by the pi coding agent.
# - npm: package manager used to install the pi coding agent and markdownlint.
# - pass: standard Unix password manager.
# - pinentry-tty: passphrase prompt helper for terminal-based GPG workflows.
# - pre-commit: Git hook runner, available from the Ubuntu archive.
# - sudo: allow privileged actions from non-root `vscode` account.
#
# Language/toolchain package rationale:
# - golang: Go compiler/toolchain (`go`).
# - rustc and cargo: Rust compiler and package manager for the `.bl` transpiler.
#
# Package versions are pinned to what's currently available in the Ubuntu
# 26.04 archive (per apt-cache madison) for reproducible builds. Ubuntu's
# archive only keeps the latest point release per package, so a pinned
# version here will eventually be superseded by a security update and need
# bumping to the new version — that's an accepted maintenance cost of pinning.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates=20260601~26.04.1 \
        curl=8.18.0-1ubuntu2.7 \
        fish=4.2.1-3.2 \
        git=1:2.53.0-1ubuntu1 \
        gh=2.46.0-4 \
        gnupg=2.4.8-4ubuntu3.1 \
        golang=2:1.26~1 \
        jq=1.8.1-4ubuntu2 \
        less=668-1build1 \
        libatomic1=16-20260322-1ubuntu1 \
        make=4.4.1-3 \
        nano=8.7.1-1ubuntu0.1 \
        nodejs=22.22.1+dfsg+~cs22.19.15-1ubuntu1 \
        npm=9.2.0~ds3-1 \
        pass=1.7.4-8 \
        pinentry-tty=1.3.2-3ubuntu1 \
        pre-commit=4.5.1-1 \
        rustc=1.93.1ubuntu1 \
        cargo=1.93.1ubuntu1 \
        sudo=1.9.17p2-1ubuntu3.1 \
        wget=1.25.0-2ubuntu4.4 \
    && rm -rf /var/lib/apt/lists/*

# Install a Docker-compatible CLI for the rootless Podman API socket.
# The host Podman service is bind-mounted at /var/run/docker.sock via
# devcontainer.json, allowing existing Docker-based repository tooling (such as
# pre-commit hooks) to use Podman without changes.
#
# - docker-ce-cli: Docker-compatible command-line client, no daemon.
# - docker-buildx-plugin: BuildKit-native `docker buildx build` support.
#
# Use Docker's official client repository rather than Ubuntu's `docker.io`
# package, which includes a daemon that is unnecessary in this container.
RUN install -m 0755 -d /etc/apt/keyrings \
    && curl -fsSL https://download.docker.com/linux/ubuntu/gpg \
           -o /etc/apt/keyrings/docker.asc \
    && chmod a+r /etc/apt/keyrings/docker.asc \
    && . /etc/os-release \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] \
https://download.docker.com/linux/ubuntu ${VERSION_CODENAME} stable" \
       | tee /etc/apt/sources.list.d/docker.list > /dev/null \
    && apt-get update \
    && apt-get install -y --no-install-recommends \
        docker-ce-cli=5:29.6.1-1~ubuntu.26.04~resolute \
        docker-buildx-plugin=0.35.0-1~ubuntu.26.04~resolute \
    && rm -rf /var/lib/apt/lists/*

# Install hadolint (Dockerfile linter, run via .pre-commit-config.yaml).
# Ubuntu does not package hadolint, so it's installed as a static binary from
# GitHub releases; the sha256 checksum published alongside each release asset
# is verified before installing.
ARG HADOLINT_VERSION=2.14.0
WORKDIR /tmp
RUN arch="$(dpkg --print-architecture)" \
    && case "${arch}" in \
        amd64) hadolint_arch=x86_64 ;; \
        arm64) hadolint_arch=arm64 ;; \
        *) echo "Unsupported architecture: ${arch}" >&2; exit 1 ;; \
    esac \
    && hadolint_bin="hadolint-linux-${hadolint_arch}" \
    && curl -fsSLO "https://github.com/hadolint/hadolint/releases/download/v${HADOLINT_VERSION}/${hadolint_bin}" \
    && curl -fsSLO "https://github.com/hadolint/hadolint/releases/download/v${HADOLINT_VERSION}/${hadolint_bin}.sha256" \
    && sha256sum -c --strict "${hadolint_bin}.sha256" \
    && install -m 0755 "${hadolint_bin}" /usr/local/bin/hadolint \
    && rm -f "${hadolint_bin}" "${hadolint_bin}.sha256"
WORKDIR /

# Install the pi coding agent globally so the `pi` command is available to the
# non-root development user. Keep the version explicit for reproducible builds.
ARG PI_CODING_AGENT_VERSION=0.87.1
ARG PI_BETTER_COMPACTION_VERSION=0.7.1
ARG PI_KEEP_GOING_VERSION=1.1.0
ARG PI_QUOTAS_VERSION=0.5.0
ARG PI_SUBAGENTS_VERSION=0.71.0
ARG PI_WEB_ACCESS_VERSION=0.31.0
ARG PONYTAIL_VERSION=4.10.0
ARG SUPERPOWERS_REF=5bf4e78011075bcfc0dc295f0724994cd123ee71
RUN npm install --global "@earendil-works/pi-coding-agent@${PI_CODING_AGENT_VERSION}" \
    && npm install --global "@fission-ai/openspec@1.13.2" \
    && npm install --global "markdownlint-cli2@0.23.3"

# Install the pinned Meslo Nerd Font Mono variants. Download individual files
# instead of an archive so each artifact can be checksum-verified directly.
ARG MESLO_NERD_FONT_VERSION=v3.5.1
RUN install -d -m 0755 /usr/local/share/fonts/meslo \
    && for font in \
        MesloLGMNerdFontMono-Regular.ttf \
        MesloLGMNerdFontMono-Bold.ttf \
        MesloLGMNerdFontMono-Italic.ttf \
        MesloLGMNerdFontMono-BoldItalic.ttf; do \
        curl -fsSL "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/${MESLO_NERD_FONT_VERSION}/patched-fonts/Meslo/M/${font}" \
            -o "/usr/local/share/fonts/meslo/${font}"; \
    done \
    && printf '%s  %s\n' \
        35c4453cc6d0861e505b62fc7039b3fb8db9966ed75fd13dda2115e2ca94c7f7 /usr/local/share/fonts/meslo/MesloLGMNerdFontMono-Regular.ttf \
        59cde1794434d0eb36d73911f8b6b6e623dafeaf659796134d4841283eb5bf09 /usr/local/share/fonts/meslo/MesloLGMNerdFontMono-Bold.ttf \
        e392010ec1b8bcbbcc7c20e51953ddd93d4f4886394607b65142d776fb2c047c /usr/local/share/fonts/meslo/MesloLGMNerdFontMono-Italic.ttf \
        6682c02559b2334cf655b69dd9bd2b67c9079c92afc17ba61c875320aeb71282 /usr/local/share/fonts/meslo/MesloLGMNerdFontMono-BoldItalic.ttf \
    | sha256sum -c -

# Create non-root user/group and configure sudo.
# - Running as non-root is the default best practice for dev containers.
# - Passwordless sudo is intentional for developer convenience in ephemeral containers.
# - Sudoers file permissions are set to 0440 as required by sudo.
# - `GPG_TTY=$(tty)` helps `gpg`/`pinentry` and tools like `pass` talk to the active terminal.
# - Rootless Podman's user namespace is configured by `--userns=keep-id` in
#   devcontainer.json, so the user can access the bind-mounted API socket without
#   a container-side Docker group.
#
# GnuPG is left at its defaults so VS Code Dev Containers' credential forwarding
# works as shipped: it forwards both the host gpg-agent (private-key operations,
# incl. pinentry, stay on the host) and the host keyboxd socket, which is where
# public keys live. No key material is created in the image.
#
# The relay only pipes those sockets — it never launches the host daemons. That
# matters for keyboxd, which the host starts on demand: if nothing has used gpg
# on the host since boot, no keyboxd is listening. On Windows hosts there are no
# Unix domain sockets, so S.keyboxd is an Assuan socket-emulation file holding a
# localhost port and nonce; when the daemon is cold that file is stale and the
# forward answers "can't connect to the keyboxd: End of file". gpg then falls
# back to spawning a container-local keyboxd against an empty store, and commit
# signing fails with "No secret key" — the confusing part being that gpg
# *recovers* into a broken state rather than erroring. Host gpg never shows this
# because it relaunches keyboxd automatically on a stale nonce file; the relay
# has no such recovery.
#
# devcontainer.json's initializeCommand runs `gpgconf --launch keyboxd` on the
# host before each start to guarantee a live target. If signing breaks anyway,
# `ls -l ~/.gnupg/S.keyboxd` in the container says which case you are in: mode
# srwxr-xr-x is the forwarded relay socket (healthy), srwx------ means the local
# fallback won and the keyring will be empty. Ownertrust is a second tell — it
# is written straight to trustdb.gpg and needs no keyboxd, so a signing key
# showing [ultimate] while absent from `gpg --list-keys` is this exact failure.
#
# Disabling keyboxd via ~/.gnupg/common.conf is not an option: Dev Containers
# rewrites that file at attach time, so a build-time setting never survives.
#
# Keep the container's GnuPG no older than the host's as a precaution against
# protocol skew across the forwarded sockets; bump the base image rather than
# pinning an older gnupg.
#
# GPG_TTY is set for both shells: bash reads ~/.bashrc, fish reads
# ~/.config/fish/config.fish (fish ignores .bashrc and has no `export` keyword —
# `set -x` exports, and command substitution is `(tty)` with no leading `$`).
# ($(tty)/(tty) must stay single-quoted so they're evaluated by the shell at
# login, not by /bin/sh at build time)
# hadolint ignore=SC2016
RUN groupadd --gid ${USER_GID} ${USERNAME} \
    && useradd --uid ${USER_UID} --gid ${USER_GID} -m ${USERNAME} \
    && echo 'export GPG_TTY=$(tty)' >> /home/${USERNAME}/.bashrc \
    && install -d -m 700 -o ${USERNAME} -g ${USERNAME} /home/${USERNAME}/.config/fish \
    && echo 'set -x GPG_TTY (tty)' >> /home/${USERNAME}/.config/fish/config.fish \
    && chown -R ${USERNAME}:${USERNAME} /home/${USERNAME}/.config \
    && install -d -m 700 -o ${USERNAME} -g ${USERNAME} /home/${USERNAME}/.gnupg \
    && echo ${USERNAME} ALL=\(root\) NOPASSWD:ALL > /etc/sudoers.d/${USERNAME} \
    && chmod 0440 /etc/sudoers.d/${USERNAME}

# The mounted Podman API implements Docker's socket-compatible API.
ENV DOCKER_HOST=unix:///var/run/docker.sock

# Default runtime user for VS Code terminals/tasks inside the container.
USER ${USERNAME}

# Preinstall this project's Pi packages in the image. Project-local settings
# still declare the same packages so Pi uses the configuration when the repo is
# opened outside this dev container.
RUN pi install "npm:@dietrichgebert/ponytail@${PONYTAIL_VERSION}" \
    && pi install "npm:@lll9p/pi-better-compaction@${PI_BETTER_COMPACTION_VERSION}" \
    && pi install "npm:pi-keep-going@${PI_KEEP_GOING_VERSION}" \
    && pi install "npm:pi-subagents@${PI_SUBAGENTS_VERSION}" \
    && pi install "npm:pi-web-access@${PI_WEB_ACCESS_VERSION}" \
    && pi install "npm:@latentminds/pi-quotas@${PI_QUOTAS_VERSION}" \
    && pi install "git:github.com/obra/superpowers@${SUPERPOWERS_REF}"

# Install Fisher from a tagged release and verify the downloaded function before
# using it to install the pinned Tide release.
ARG FISHER_VERSION=4.4.8
RUN install -d -m 700 /home/${USERNAME}/.config/fish/functions \
    && curl -fsSL "https://raw.githubusercontent.com/jorgebucaran/fisher/${FISHER_VERSION}/functions/fisher.fish" \
        -o /home/${USERNAME}/.config/fish/functions/fisher.fish \
    && printf '%s  %s\n' \
        0fb6c81ae3003e95b5671766fa6c25c3597066e29965b7772f6c1b007387356d /home/${USERNAME}/.config/fish/functions/fisher.fish \
        | sha256sum -c - \
    && fish -c 'source ~/.config/fish/functions/fisher.fish; fisher install IlanCosman/tide@c4e3831dc4392979478d3d7b66a68f0274996c85'

# Apply the reviewed Tide configuration non-interactively and reproducibly.
RUN fish -c "tide configure --auto --style=Rainbow --prompt_colors='True color' --show_time=No --rainbow_prompt_separators=Angled --powerline_prompt_heads=Sharp --powerline_prompt_tails=Flat --powerline_prompt_style='Two lines, character and frame' --prompt_connection=Disconnected --powerline_right_prompt_frame=No --prompt_connection_andor_frame_color=Lightest --prompt_spacing=Sparse --icons='Many icons' --transient=No"

# Install uv — the Python package/tool manager used for all Python tooling in
# this repo (we standardise on uv rather than pip). The official installer
# places `uv`/`uvx` in ~/.local/bin for the non-root user; add that to PATH so
# uv and the tools it installs are resolvable in build steps and shells.
ENV PATH="/home/${USERNAME}/.local/bin:${PATH}"
RUN curl -LsSf https://astral.sh/uv/install.sh | sh

# Install project validation tools as isolated uv-managed tools. They are not
# available from the Ubuntu archive; uv provisions their Python runtimes.
# (pre-commit is installed above from Ubuntu's archive.)
RUN uv tool install zensical \
    && uv tool install check-jsonschema

# The host Podman socket's group is known only after it is mounted. Run this
# before the VS Code server starts; devcontainer.json preserves the entrypoint.
COPY --chmod=0755 .devcontainer/grant-podman-socket.sh /usr/local/bin/grant-podman-socket.sh
ENTRYPOINT ["/usr/local/bin/grant-podman-socket.sh"]
CMD ["sleep", "infinity"]
