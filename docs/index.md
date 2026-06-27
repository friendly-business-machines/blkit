# blkit

> A Go SDK for expressing and executing business rules and processes in code.

blkit gives you a typed library for modelling business logic — decision rules,
process flows, and the data that moves through them — and executing it directly
from Go. It draws inspiration from [DMN](https://www.omg.org/dmn/) (Decision
Model and Notation) and [BPMN](https://www.bpmn.org/) (Business Process Model
and Notation), but it is a practical, self-contained library rather than a
conformance implementation of either standard.

## Where to go next

| Section | What you'll find |
|---|---|
| [Getting started](getting-started/index.md) | What blkit is, when to use it, and a quick orientation. |
| [Installation](installation/index.md) | Adding blkit to your project and pinning a version. |
| [Tutorials](tutorials/index.md) | Guided, narrative walkthroughs of complete use cases. |
| [Templates](templates/index.md) | Ready-to-copy project scaffolds for common patterns. |
| [Examples](examples/loan-eligibility.md) | Focused, self-contained demonstrations of specific features. |
| [Reference](reference/blkit.md) | The complete Go API reference, generated from source. |

!!! note "Documentation in progress"
    blkit is under active development. The expression engine (the root `blkit`
    package) is available today; the `decisions`, `processes`, and other
    packages are being built, and the Tutorials, Templates, and Examples
    implementations land alongside them.
