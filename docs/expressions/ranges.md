# Ranges

> Intervals like `[1..10]` — membership tests, open and closed bounds, and how
> ranges drive unary tests.

!!! note "Guide in progress"
    This page of the language guide is still being written. In the meantime,
    [Architecture → Expressions](../architecture/expressions.md) explains how the
    engine works, the generated [Reference](../reference/blkit.md) lists the API,
    and `specs/expressions/range.spec.md` defines the behaviour authoritatively.

## What this page will cover

A range is an interval between two bounds; this page covers range literals,
membership, and their role in unary tests.

- Range literals and open / closed bounds
- Membership and overlap tests
- Ranges in decision-table unary tests
