---
name: ProcessState
description: The data store for a single run of a process — each task writes its outputs into it, and later tasks read the values they need out of it
status: draft
code:
  - core/process_state.go
---

# ProcessState

## Purpose

A `ProcessState` holds all the data for **one run** of a process.

Every time a process runs, a fresh `ProcessState` is created for that run. As the
run progresses, each task writes its output values into the `ProcessState`, and
later tasks read the values they need back out of it. The `ProcessState` also keeps
an **execution history** of what happened along the way — each task starting and
finishing, which paths were taken, and so on. When the run finishes, the
`ProcessState` holds everything that happened during that run: both the values and
the execution history.

The process definition itself (the tasks, the graph, the wiring between them) is
shared by every run and never holds any run's data. The data only ever lives in
the `ProcessState`. This is the whole reason many runs of the same process can
happen at the same time without getting in each other's way — see
[Many runs at once](#many-runs-at-once).

## Design

### Structure

- **Tasks don't hand data to each other.** One task writes its outputs into the
  `ProcessState`; a later task reads the values it needs out of it. The two tasks
  never talk to each other directly, so tasks stay decoupled from one another —
  they only ever talk to the `ProcessState`.
- **Wiring records an address, not a delivery.** The wiring set up when a process
  is built does not move any values around; it only records *where in the
  `ProcessState`* a value should be read from — an address, not a parcel.
- **Handles are typed labels, not values.** Every input and output field has a
  handle carrying the owning task, the field name, and the value's type. Because
  the handle carries the type, a mis-typed wiring is caught before the process
  ever runs.
- **The definition holds no run data.** All of a run's data lives in its
  `ProcessState`; nothing is ever stored back on the shared task objects or
  handles. This is what makes concurrent runs of the same process safe.
- **One `ProcessState` per run.** The runtime creates a fresh, empty one at the
  start of each run; two runs share the process definition but never any data.
- **Inputs snapshot at begin; outputs land atomically at finish.** A task works
  with the fixed set of input values it was handed when it began, and its outputs
  appear in the `ProcessState` all together once it finishes — so a task never
  sees mid-run churn, and there is never a half-written set of outputs.
- **The run works with the current version; the store keeps the full history.**
  The running process only ever reads and writes the latest value for each entry.
  The state store behind it keeps every value ever written, in order, purely for
  diagnostics and audit.
- **Values and execution history are separate records.** The values tell you the
  *result* — what each task produced; the execution history tells you the
  *story* — what happened, in what order, and when. Together they are the full
  picture of the run.

### Non-Goals

To keep this first version simple, several things are left out on purpose. For now
they are still described by [execution-context.spec.md](../data/execution-context.spec.md):

- How a task's writes are kept hidden from other tasks until the task finishes
  successfully (and thrown away if it fails).
- Looking at the state as it was at an earlier point in the run.
- How sub-processes get their own view of the state.
- How a task that runs many times (loops, or once-per-item) records each run.
- The finer detail of the execution history — exactly what each entry contains, and
  how to tell from it where the run has currently got to.
- How the state is saved so a run can be paused and resumed later.

These will be folded into this spec as it grows.

## API Contract

The concrete Go surface is still being shaped (`status: draft`). What is settled
is the addressing convention — handles — and per-run construction, shown below in
the forms the examples use.

### Handles: typed labels for a task's inputs and outputs

Every task declares its inputs and outputs as typed fields. Each of those fields
has a **handle** — a small, fixed label that says:

- which task it belongs to,
- the name of the field, and
- the type of the value (a number, a string, a boolean, and so on).

A handle does **not** hold a value. It is only a label. For example, a task with
the id `screen` and an output field `Score` has an output handle you can write as
`screen.Outputs.Score`. That handle means *"the `Score` value produced by the
`screen` task"* — it's the address, not the number.

When you wire a later task's input to `screen.Outputs.Score`, you are telling that
task: *"when you need this input, read it from the `Score` that `screen` wrote."*

### Configuration & Construction

A `ProcessState` belongs to exactly one run. The runtime creates a fresh, empty
one at the start of each run and fills it in as the run goes.

```go
// Start a new run. This makes a brand-new, empty ProcessState for this run only.
var state = store.NewProcessState(admissions, "start", input)

// The runtime reads from and writes to `state` as the run progresses.
```

Running the same process again makes a **different** `ProcessState`. The two runs
share the same process definition (the same tasks and wiring), but they do not
share any data.

## Behaviour

### The main idea: tasks don't hand data to each other

It's tempting to picture one task passing its result straight to the next task.
That is **not** how it works.

Instead:

1. When a task finishes, it **writes** its outputs into the `ProcessState`.
2. When a later task starts, it **reads** the values it needs out of the
   `ProcessState`.

The two tasks never talk to each other directly. They only ever talk to the
`ProcessState`. One writes; the other reads.

The wiring you set up when you build the process (connecting one task's output to
another task's input) does **not** move any values around. All it does is record
**where in the `ProcessState`** a value should be read from. Think of it as
writing down an address, not delivering a parcel.

### Reading a task's inputs (when it begins)

A task receives its input values **at the moment it begins**. Just before the task
starts, the runtime looks at how each of its inputs was wired, reads each value out
of the `ProcessState` at the wired address, and hands the whole set to the task.

The task then works with that fixed set of values for the entire time it runs. If
something else in the process changes the state while the task is still running,
the task does not see it — it keeps the values it was given at the start.

So if the `decide` task wired its `Score` input to `screen.Outputs.Score`, then
the moment `decide` begins, the runtime reads `screen.Score` from the
`ProcessState` (which is `88`) and gives it to `decide` as its `Score` input.

### Writing a task's outputs (when it finishes)

A task's outputs are written into the `ProcessState` **all together, once the task
finishes**. While the task is running, none of its outputs are in the process state
yet. The moment it finishes successfully, the runtime writes all of its output
values in at once, filed under the task's id and each output field's name.

There is never a half-written moment where some of a task's outputs are present and
others are still missing — either all of them are in the `ProcessState`, or none
of them are.

So if the `screen` task finishes and produces `Score = 88` and `Band = "high"`,
both entries appear in the `ProcessState` together:

```
screen.Score = 88
screen.Band  = "high"
```

Those two entries are exactly the addresses that `screen`'s output handles
(`screen.Outputs.Score` and `screen.Outputs.Band`) point at.

### A full example

A small admissions process: `screen` an application, then `decide` whether to
admit.

```go
// The process input arrives at the start event; tasks read it from the
// start event's Outputs.
var start = bl.Start("start", "Start", bl.NewInputContract(
    bl.RequiredField("application", bl.BlDictionary),
))

type ScreenInputs struct {
    Application BlDictionary
}
type ScreenOutputs struct {
    Score BlNumber
    Band  BlString
}

var screen = bl.NewNativeFunctionTask(NativeFunctionTaskOpts[ScreenInputs, ScreenOutputs]{
    Id:   "screen",
    Name: "Screen Application",
    InputBindings: func(in ScreenInputs) []ParameterBinding {
        return []ParameterBinding{
            bl.Bind(in.Application, start.Outputs.Application),
        }
    },
    Fn: func(in *ScreenInputs) (ScreenOutputs, error) {
        return ScreenOutputs{Score: bl.Number(88), Band: bl.String("high")}, nil
    },
})

type DecideInputs struct {
    Score BlNumber
}
type DecideOutputs struct {
    Admit BlBoolean
}

var decide = bl.NewNativeFunctionTask(NativeFunctionTaskOpts[DecideInputs, DecideOutputs]{
    Id:   "decide",
    Name: "Admission Decision",
    // Wire this task's Score input to the Score that `screen` produces.
    InputBindings: func(in DecideInputs) []ParameterBinding {
        return []ParameterBinding{
            bl.Bind(in.Score, screen.Outputs.Score),
        }
    },
    Fn: func(in *DecideInputs) (DecideOutputs, error) {
        return DecideOutputs{Admit: bl.Boolean(in.Score.ToNativeFloat() >= 70)}, nil
    },
})

var admitted = bl.End("admitted", "Admitted")

// Create the process, connecting the tasks in order: start -> screen -> decide -> end.
var admissions = bl.NewProcess("admissions", "1.0", ProcessOpts{
    Name: "Admissions Process",
    Graph: []ProcessNode{
        start.To(screen).To(decide).To(admitted),
    },
})
```

When this process runs, here is what happens to the `ProcessState`:

```
1. The run starts.
   A new, empty ProcessState is created for this run and given a fresh
   id that identifies this run. The start values that were passed in are
   written in under the start event:
       start.application = { ...the submitted application... }

2. screen BEGINS.
   Its Application input is wired to start.Outputs.Application, so the
   runtime READS start.application and hands it to screen.
   When screen FINISHES, the runtime WRITES its outputs in at once:
       screen.Score = 88
       screen.Band  = "high"

3. decide BEGINS.
   Its Score input is wired to screen.Outputs.Score, so the runtime
   READS screen.Score from the ProcessState -> 88 and hands 88 to decide.
   decide keeps this value for its whole run.

4. decide FINISHES with Admit = true.
   The runtime WRITES its output in:
       decide.Admit = true
```

The wiring `bl.Bind(in.Score, screen.Outputs.Score)` did one job only: it said
*"read this task's Score from `screen.Score`"*. The value itself travelled
`screen -> ProcessState -> decide`; it never passed through the wiring or through
the `screen` task object.

### The current version, and the full history

Behind every `ProcessState` is a **state store** that saves everything that
happens during the run. Every value a task writes is kept there, in the order it
was written — so the state store holds the **full history** of the run.

The `ProcessState` that the running process actually uses is different: it only
ever shows the **current version** — the latest value for each thing that has been
written. When a task reads one of its inputs, it gets the current value, not any
older ones that came before it.

So there are two things to keep separate:

- **The state store** keeps the full history — every value ever written during the
  run, from start to finish.
- **The `ProcessState` the run works with** is just the current version — the
  latest values, and nothing older.

The full history in the state store is there purely for diagnostics and audit —
being able to look back later and see what happened during the run. The running
process does not work with the history directly — it only ever reads and writes the
current version.

### The execution history: what happened, and when

Alongside the values, the `ProcessState` keeps an **execution history** of the
run — a list, in order, of everything that happened. Each entry records one event
and when it happened. For example:

- the run started
- a task was lined up to run
- a task began
- a task finished (or failed, along with the error)
- a point where the process chose which path (or paths) to take next
- the run finished (or failed)

The values and the execution history answer two different questions:

- **The values** tell you the *result* — what each task produced.
- **The execution history** tells you the *story* — what happened, in what order,
  and when.

Together they are the full picture of the run.

Like the value history, the whole execution history is kept in the state store, so
the run's story can be read back afterwards for diagnostics and audit.

The execution history is what
[execution-history.spec.md](../data/execution-history.spec.md) describes today. In
time, keeping it will become the `ProcessState`'s job, and that separate spec will
fold into this one.

### Many runs at once

Because the process definition holds no data and every run gets its own
`ProcessState`, many runs of the same process can happen at the same time safely.

- Two runs may both use the same shared `screen` task object at the same moment,
  but each one reads from and writes to its **own** `ProcessState`. Their data
  never mixes.
- Nothing about a run is ever stored back on the shared tasks or handles — the
  handles are only labels, and the values always live in that run's
  `ProcessState`.

There is one place that does need care. **Within a single run**, a process can run
several tasks in parallel (for example after a parallel split). Those parallel
tasks all write into the **same** `ProcessState` at the same time. So a
`ProcessState` is built to be safe to write to from several tasks at once within
its own run.

To sum up the two cases:

- **Different runs:** completely separate `ProcessState`s, so they never interfere.
- **Parallel tasks in one run:** share that run's `ProcessState`, which is built to
  handle several writers at once.

## Edge Cases

- Wiring an output of one type into an input of a different type is caught as a
  mistake straight away, before the process ever runs.
- A task's outputs land all together or not at all — there is never a moment where
  some of a task's outputs are in the `ProcessState` and others are missing.
- A task never observes writes made to the `ProcessState` after it began — it
  keeps the input values it was handed at the start for its whole run.
- Two runs of the same process never share data, even when both use the same
  shared task objects at the same moment.
- Parallel tasks within one run write to the same `ProcessState` concurrently;
  it must be safe under several writers at once.

Boundary behaviour for the deferred areas (visibility of uncommitted writes,
loops, sub-process views, pause and resume) is not yet enumerated here — see
[Non-Goals](#non-goals) for where those semantics currently live.

## Verification

No implementation exists yet (`status: draft`). The intended verification home is
`core/process_state_test.go`, alongside the intended `code:` location
`core/process_state.go`; test links will be added here when the spec is agreed and
the implementation lands.
