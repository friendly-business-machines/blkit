#!/bin/sh
# Git exports repository-local variables to hooks; subprocesses may use other repositories.
for name in $(git rev-parse --local-env-vars); do
    unset "$name"
done
exec "$@"
