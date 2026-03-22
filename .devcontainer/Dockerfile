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
# - dotnet-sdk-8.0: .NET SDK for C# build/run (`dotnet`).
# - golang: Go compiler/toolchain (`go`).
# - openjdk-21-jdk: JDK (includes `javac` + `java`).
# - nodejs + npm: Node runtime and package manager.
# - python3: Python interpreter.
# - rustc + cargo: Rust compiler and package/build tool.
# - node-typescript: TypeScript compiler (`tsc`) from Ubuntu repositories.
#
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        cargo \
        curl \
        dotnet-sdk-8.0 \
        git \
        gh \
        gnupg \
        golang \
        jq \
        less \
        make \
        nano \
        node-typescript \
        nodejs \
        npm \
        openjdk-21-jdk \
        pass \
        pinentry-tty \
        python3 \
        rustc \
        sudo \
        wget \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user/group and configure sudo.
# - Running as non-root is the default best practice for dev containers.
# - Passwordless sudo is intentional for developer convenience in ephemeral containers.
# - Sudoers file permissions are set to 0440 as required by sudo.
# - `GPG_TTY=$(tty)` helps `gpg`/`pinentry` and tools like `pass` talk to the active terminal.
RUN groupadd --gid ${USER_GID} ${USERNAME} \
    && useradd --uid ${USER_UID} --gid ${USER_GID} -m ${USERNAME} \
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
