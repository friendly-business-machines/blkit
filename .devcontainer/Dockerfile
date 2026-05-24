# If you modify this file, verify it still builds before committing:
#   docker build -f .devcontainer/Dockerfile .
# See AGENTS.md for details.

# Base image:
# - Ubuntu 24.04 LTS (Noble Numbat) was released on 25Apr2024.
# - Standard LTS support runs through April 2029.
# - Base image size is approx 117mb
FROM ubuntu:24.04

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
# - gh: GitHub CLI for repository, PR, issue, and auth workflows.
# - git: source control operations.
# - gnupg: `gpg` tooling for signing, encryption, and use with `pass`.
# - jq: JSON parsing in shell scripts.
# - less: terminal pager for viewing file output.
# - nano: lightweight terminal text editor.
# - pass: standard Unix password manager.
# - pinentry-tty: passphrase prompt helper for terminal-based GPG workflows.
# - sudo: allow privileged actions from non-root `vscode` account.
#
# Language/toolchain package rationale:
# - golang: Go compiler/toolchain (`go`).
#
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        curl \
        git \
        gh \
        gnupg \
        golang \
        jq \
        less \
        make \
        nano \
        pass \
        pinentry-tty \
        sudo \
        wget \
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
        docker-ce-cli \
        docker-buildx-plugin \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user/group and configure sudo.
# - Running as non-root is the default best practice for dev containers.
# - Passwordless sudo is intentional for developer convenience in ephemeral containers.
# - Sudoers file permissions are set to 0440 as required by sudo.
# - `GPG_TTY=$(tty)` helps `gpg`/`pinentry` and tools like `pass` talk to the active terminal.
# - A `docker` group is created so the user can access the bind-mounted daemon socket
#   without sudo. Its GID is reconciled with the host socket's GID at container start
#   via postStartCommand in devcontainer.json.
RUN groupadd --gid ${USER_GID} ${USERNAME} \
    && useradd --uid ${USER_UID} --gid ${USER_GID} -m ${USERNAME} \
    && groupadd docker \
    && usermod -aG docker ${USERNAME} \
    && echo 'export GPG_TTY=$(tty)' >> /home/${USERNAME}/.bashrc \
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
