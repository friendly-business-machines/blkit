# If you modify this file, verify it still builds before committing:
#   docker build -f .devcontainer/Dockerfile .
# See AGENTS.md for details.

# Base image:
# - Ubuntu 24.04 LTS (Noble Numbat) was released on 25Apr2024.
# - Standard LTS support runs through April 2029.
# - Base image size is approx 117mb
FROM ubuntu:24.04

# Use bash with pipefail for all RUN instructions. Without this, a `curl | sh`
# style pipe only reports the exit code of the last command, so a failed
# download would silently succeed the build.
SHELL ["/bin/bash", "-o", "pipefail", "-c"]

# Build args for the default non-root dev user.
# - Microsoft Dev Container base images commonly use `vscode` with UID/GID 1000.
# - UID/GID 1001 is used instead of the more common 1000 because Ubuntu 24.04's
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
# - pass: standard Unix password manager.
# - pinentry-tty: passphrase prompt helper for terminal-based GPG workflows.
# - sudo: allow privileged actions from non-root `vscode` account.
#
# Language/toolchain package rationale:
# - golang: Go compiler/toolchain (`go`).
#
# Package versions are pinned to what's currently available in the Ubuntu
# 24.04 archive (per apt-cache madison) for reproducible builds. Ubuntu's
# archive only keeps the latest point release per package, so a pinned
# version here will eventually be superseded by a security update and need
# bumping to the new version — that's an accepted maintenance cost of pinning.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates=20260601~24.04.1 \
        curl=8.5.0-2ubuntu10.10 \
        fish=3.7.0-1 \
        git=1:2.43.0-1ubuntu7.3 \
        gh=2.45.0-1ubuntu0.3 \
        gnupg=2.4.4-2ubuntu17.4 \
        golang=2:1.22~2build1 \
        jq=1.7.1-3ubuntu0.24.04.2 \
        less=590-2ubuntu2.1 \
        libatomic1=14.2.0-4ubuntu2~24.04.1 \
        make=4.3-4.1build2 \
        nano=7.2-2ubuntu0.2 \
        pass=1.7.4-6 \
        pinentry-tty=1.2.1-3ubuntu5 \
        sudo=1.9.15p5-3ubuntu5.24.04.2 \
        wget=1.21.4-1ubuntu4.1 \
    && rm -rf /var/lib/apt/lists/*

# Install Docker CLI (Docker-outside-of-Docker / DooD).
# The Docker daemon runs on the host; only the CLI is needed in the container.
# The host daemon socket is bind-mounted at /var/run/docker.sock via devcontainer.json.
#
# - docker-ce-cli: Docker command-line interface (client only, no daemon).
# - docker-buildx-plugin: BuildKit-native `docker buildx build` support.
#
# Using Docker's official apt repository rather than Ubuntu's `docker.io` package,
# which lags significantly behind upstream releases.
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
        docker-ce-cli=5:29.6.1-1~ubuntu.24.04~noble \
        docker-buildx-plugin=0.35.0-1~ubuntu.24.04~noble \
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

# Create non-root user/group and configure sudo.
# - Running as non-root is the default best practice for dev containers.
# - Passwordless sudo is intentional for developer convenience in ephemeral containers.
# - Sudoers file permissions are set to 0440 as required by sudo.
# - `GPG_TTY=$(tty)` helps `gpg`/`pinentry` and tools like `pass` talk to the active terminal.
# - A `docker` group is created so the user can access the bind-mounted daemon socket
#   without sudo. Its GID is reconciled with the host socket's GID at container start
#   via postStartCommand in devcontainer.json (see reconcile-docker-gid.sh).
#
# GnuPG keyboxd is deliberately left disabled for the dev user (empty
# ~/.gnupg/common.conf). Ubuntu 24.04 ships GnuPG 2.4.4, whose keyboxd is not
# wire-compatible with a newer host keyboxd: hosts on GnuPG 2.4.5+ (e.g. 2.4.8
# on Windows/macOS) store public keys in keyboxd, and when VS Code Dev
# Containers forwards that keyboxd socket, a 2.4.4 client reads *zero* keys back
# from it — so `git` finds no public key for the signing key and commit signing
# fails with "No secret key". With keyboxd off, GnuPG uses a classic pubring and
# stays on the robust, version-tolerant path: VS Code forwards the gpg-agent
# (private-key operations, incl. pinentry, stay on the host) and copies the
# developer's public keys into pubring.gpg. common.conf has no negation
# directive — keyboxd is simply inactive unless `use-keyboxd` is present, so a
# comment-only file keeps it off. This is generic: it works for any developer's
# own host keys, nothing is hardcoded.
# GPG_TTY is set for both shells: bash reads ~/.bashrc, fish reads
# ~/.config/fish/config.fish (fish ignores .bashrc and has no `export` keyword —
# `set -x` exports, and command substitution is `(tty)` with no leading `$`).
# ($(tty)/(tty) must stay single-quoted so they're evaluated by the shell at
# login, not by /bin/sh at build time)
# hadolint ignore=SC2016
RUN groupadd --gid ${USER_GID} ${USERNAME} \
    && useradd --uid ${USER_UID} --gid ${USER_GID} -m ${USERNAME} \
    && groupadd docker \
    && usermod -aG docker ${USERNAME} \
    && echo 'export GPG_TTY=$(tty)' >> /home/${USERNAME}/.bashrc \
    && install -d -m 700 -o ${USERNAME} -g ${USERNAME} /home/${USERNAME}/.config/fish \
    && echo 'set -x GPG_TTY (tty)' >> /home/${USERNAME}/.config/fish/config.fish \
    && chown -R ${USERNAME}:${USERNAME} /home/${USERNAME}/.config \
    && install -d -m 700 -o ${USERNAME} -g ${USERNAME} /home/${USERNAME}/.gnupg \
    && printf '# keyboxd intentionally disabled - see Dockerfile rationale above\n' \
        > /home/${USERNAME}/.gnupg/common.conf \
    && chown ${USERNAME}:${USERNAME} /home/${USERNAME}/.gnupg/common.conf \
    && chmod 600 /home/${USERNAME}/.gnupg/common.conf \
    && echo ${USERNAME} ALL=\(root\) NOPASSWD:ALL > /etc/sudoers.d/${USERNAME} \
    && chmod 0440 /etc/sudoers.d/${USERNAME}

# Default runtime user for VS Code terminals/tasks inside the container.
USER ${USERNAME}

# Install Tessl CLI.
# ADD downloads the install script.
# --chown sets ownership to the non-root user; --chmod=+x marks it executable.
ADD --chown=${USERNAME}:${USERNAME} --chmod=+x https://get.tessl.io /tmp/install-tessl.sh
RUN /tmp/install-tessl.sh \
    && rm /tmp/install-tessl.sh

# Install uv — the Python package/tool manager used for all Python tooling in
# this repo (we standardise on uv rather than pip). The official installer
# places `uv`/`uvx` in ~/.local/bin for the non-root user; add that to PATH so
# uv and the tools it installs are resolvable in build steps and shells.
ENV PATH="/home/${USERNAME}/.local/bin:${PATH}"
RUN curl -LsSf https://astral.sh/uv/install.sh | sh

# Install the Python-based developer tooling as isolated uv-managed tools.
# uv provisions a managed Python for each automatically, so no system pip is
# needed.
# - zensical: documentation site builder (see specs/project/documentation.spec.md).
# - pre-commit: git hook runner (see .pre-commit-config.yaml).
RUN uv tool install zensical \
    && uv tool install pre-commit

# Install the docker-group GID reconciliation script. This is NOT an ENTRYPOINT:
# VS Code Dev Containers defaults `overrideCommand: true` for Dockerfile-based
# configs, which replaces the image's ENTRYPOINT/CMD with its own keep-alive
# command — so a baked-in ENTRYPOINT would be silently bypassed and never run.
# Instead it is invoked from devcontainer.json's postStartCommand (with sudo),
# which runs on every container start regardless of overrideCommand. See the
# script's header for the full rationale.
COPY --chmod=755 .devcontainer/reconcile-docker-gid.sh /usr/local/bin/reconcile-docker-gid.sh
