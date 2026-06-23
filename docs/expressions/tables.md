# Tables

> Tabular data and the operations that filter, project, group, and aggregate it.

!!! note "Guide in progress"
    This page of the language guide is still being written. In the meantime,
    [Architecture → Expressions](../architecture/expressions.md) explains how the
    engine works, the generated [Reference](../reference/blkit.md) lists the API,
    and `specs/expressions/table.spec.md` defines the behaviour authoritatively.

## What this page will cover

Tables hold rows of typed columns; this page covers building tables and the
operations that reshape and summarise them.

- Constructing tables and accessing rows and columns
- Filtering, projecting, and adding columns
- Grouping and aggregation
