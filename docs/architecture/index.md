# Architecture

> How blkit is built, subsystem by subsystem — the internal design of the
> engine behind the public API.

The **Architecture** section explains how blkit works on the inside. Where the
[Reference](../reference/blkit.md) section documents *what* the API is and the
[Examples](../examples/index.md) section shows *how* to use it, the pages here
explain *why the machinery is shaped the way it is* — the pipelines, the
intermediate representations, and the design decisions that hold blkit together.

It is written for contributors, the curious, and anyone debugging behaviour that
only makes sense once you can see the layers underneath.

## Subsystems

blkit is built from a small number of layered subsystems. Each gets its own
chapter:

| Subsystem | What it covers |
|---|---|
| **[Expressions](expressions.md)** | The expression language: how a source string becomes type-checked, compiled, repeatedly-evaluable bytecode — normalisation, parsing, AST patching, compilation, and the value system. |

!!! note "More chapters to come"
    Decisions, processes, and workers each build on the layer beneath them and
    will get their own chapters as those packages land. The intended shape is a
    stack:

    ```text
    Workers      run processes to completion, handling state and retries
       ▲
    Processes    orchestrate decisions and tasks as a graph of steps
       ▲
    Decisions    evaluate rules — tables, expressions — to reach an outcome
       ▲
    Expressions  the language every layer above is written in   ← available today
    ```

    Each layer is a consumer of the one below it. The expression engine is the
    foundation: decision rules, and the conditions that route a process, are all
    blkit expressions under the hood. Start there.
