# Processes

> Assemble tasks and gateways into an executable process graph that runs from a
> start event through to an end event.

!!! warning "Coming soon"
    This guide is being written and will be published when the process and
    worker packages land. The behaviour will be defined authoritatively by the
    process specs under `specs/`.

## What this page will cover

A process is the whole graph — tasks wired together by gateways and sequence
flows — that blkit executes from start to finish.

- Modelling a process as a graph of tasks and gateways
- Start and end events and the execution lifecycle
- How process state is carried between steps
