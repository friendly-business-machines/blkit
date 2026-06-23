# Tasks

> The work items a process is built from — service, decision, and user tasks —
> and how each one executes and hands control on.

!!! warning "Coming soon"
    This guide is being written and will be published when the process and
    worker packages land. The behaviour will be defined authoritatively by the
    process specs under `specs/` and the worker spec under `specs/worker/`.

## What this page will cover

Tasks are the individual steps of a process — each does a unit of work, then
passes control to whatever follows.

- The kinds of task (service, decision, user, …)
- Binding inputs and capturing outputs
- How a task signals completion and errors
