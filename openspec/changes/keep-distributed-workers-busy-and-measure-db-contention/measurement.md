# PostgreSQL single-connection baseline (task 1.1)

Measured at `34f54272` on 2026-10-16 UTC, **before** changing worker refill or connection management. This is an instrumented diagnostic build, not a production connection-pool result. No pooling decision is made by this baseline alone; repeat this exact workload after task 2.1 before evaluating connection changes.

## Setup and reproducibility

- Linux 6.18.54.1-microsoft-standard-WSL2, x86_64 AMD Ryzen 9 5900X (24 logical CPUs), 31 GiB RAM (~27 GiB available); Rust/cargo 1.96.1; Docker server 28.5.1. Shared WSL2 host; not isolated from other host/container workloads.
- One disposable `postgres:17.6-alpine` Docker container (PostgreSQL 17.6, Alpine/musl), default `max_connections=100`, `shared_buffers=128 MiB`, `track_activities=on`, `track_io_timing=off`; only nondefault server option `shared_preload_libraries=pg_stat_statements`. No CPU/memory cgroup quota; connection from test process through `host.docker.internal:15439` to host-mapped PostgreSQL port. Database: `postgres` (disposable, no real user data; benchmark requires explicit `BLKIT_MEASURE_DISPOSABLE=yes` before its TRUNCATE); one `PostgresStore::connect` for all API, worker and reconciler clones, plus one separate **observer** connection for pg_stat_statements. Server connection password was local/disposable; do not copy credentials into measurements.
- At the start of **each mode**: `TRUNCATE instances, workers, process_policies CASCADE`, re-register one capable worker, prequeue only combined-mode work, reset `pg_stat_statements`; no warm-up discarded. API-only executes eight concurrent clients × twelve cycles of POST start (input 400ms), GET status, POST cancel = 96 each / 288 requests. Axum in-process `router_distributed(...).oneshot(...)` exercises the actual API handlers, but does **not** measure HTTP socket/network overhead. A 100ms reconciler runs in the same test process during each mode. Worker's capability enables admission even in API-only mode, but the worker never executes there.
- Combined mode has the same eight API clients and reconciler plus one `DistributedWorker` on the **same cloned store/connection**: limit=32, lease=5000ms, eight queued 4200ms and 64 queued 15ms tasks, a graph with a task and checkpoint writes; the worker calls `run_once` in a 100ms outer loop. Tasks sleep in `spawn_blocking`, not in PostgreSQL. 72 prequeued tasks finish with checkpoints, and zero fail, asserted on each successful run. API workload starts after queuing, so queue-setup SQL and registration do not enter the reported timing or DB execution totals.
- Test source: `tools/dev-harness/tests/postgres_contention.rs`; temporarily apply `measurement-instrumentation.patch` to existing `PostgresStore` lock sites, **revert it after measurement**. It times every acquisition of the existing Arc<tokio::sync::Mutex<Client>> (23 sites, including API, reconciler and worker) from immediately before `.lock().await` until guard acquisition, logging `LOCK_WAIT_US` (integer microseconds, truncated); the test writes mode begin/end markers. Diagnostic printing alters absolute timings, so all baseline runs use the same instrumented binary. `pg_stat_statements` sum of `total_exec_time` in milliseconds is actual server SQL execution time across queries in the database, including the tiny observer query; it excludes client/network wait, task sleeps, and lock acquisition. It is *not* wall time, nor per-operation latency. Lock-wait histograms pool all store calls in a mode, not per-API-call waits. Percentile definition is the sorted sample at `floor(n*p/100)` (0-indexed). Throughput = completed API requests / duration until all 288 API responses, excluding later worker completion.

Reproduce from this worktree on a machine where the Docker socket is accessible (keep the DB password out of logs and shell history):

```sh
# Supply a disposable DB password privately; e.g. read -rs PG_PASSWORD; echo
# Use an unused published port, matching the example below.
docker run -d --rm --name blkit-db-refill-measure \
  -e "POSTGRES_PASSWORD=$PG_PASSWORD" -p 127.0.0.1:15439:5432 \
  postgres:17.6-alpine -c shared_preload_libraries=pg_stat_statements
# Wait for: docker exec blkit-db-refill-measure pg_isready -U postgres
# With BLKIT_TESTCONTAINERS_HOST=host.docker.internal on this WSL2 test host:
export BLKIT_MEASURE_DATABASE_URL="postgres://postgres:$PG_PASSWORD@host.docker.internal:15439/postgres"
export BLKIT_MEASURE_DISPOSABLE=yes  # mandatory explicit opt-in before truncation
export CARGO_TARGET_DIR=/tmp/blkit-db-refill-target.LmFOuM
git apply openspec/changes/keep-distributed-workers-busy-and-measure-db-contention/measurement-instrumentation.patch
for i in 1 2 3 4 5; do
  cargo test -q -p blkit-dev-harness --test postgres_contention -- --nocapture >"/tmp/blkit-contention-baseline-$i.log" 2>&1 || break
  grep '^mode=' "/tmp/blkit-contention-baseline-$i.log"
  df -h /tmp .; du -sh "$CARGO_TARGET_DIR"
done
# Analyze wait samples between the mode=... phase=begin/end markers, sorted at indices n/2 and n*95/100.
git apply -R openspec/changes/keep-distributed-workers-busy-and-measure-db-contention/measurement-instrumentation.patch
docker rm -f blkit-db-refill-measure
unset BLKIT_MEASURE_DATABASE_URL BLKIT_MEASURE_DISPOSABLE PG_PASSWORD
```

The host's Docker-published `127.0.0.1` port was **not** reachable from this dev container; `host.docker.internal` was reachable. Locally, capture the same summary with: `grep '^mode=' /tmp/blkit-contention-baseline-N.log`; parse lock samples with the stdlib snippet below, which avoids storing thousands of per-call lines in the repository:

```sh
python3 - <<'PY'
from pathlib import Path
for run in range(1, 6):
    mode = None
    waits = {'api': [], 'combined': []}
    for line in Path(f'/tmp/blkit-contention-baseline-{run}.log').read_text().splitlines():
        if 'mode=api phase=begin' in line: mode = 'api'
        elif 'mode=combined phase=begin' in line: mode = 'combined'
        elif 'phase=end' in line: mode = None
        elif line.startswith('LOCK_WAIT_US ') and mode:
            waits[mode].append(int(line.split()[1]))
    for mode, values in waits.items():
        values.sort()
        assert values, (run, mode)
        n = len(values)
        print(run, mode, n, values[n // 2], values[n * 95 // 100], values[n * 99 // 100])
PY
```

## Raw run summary

All time figures below are **milliseconds**, except operation p50/p95 pairs (also milliseconds, converted from test output microseconds); `DB` is PostgreSQL `sum(total_exec_time)` in milliseconds. `wait` is pooled per-lock acquisition (not a request latency). API-only: 512 instrumented lock acquisitions in every run; combined: variable count because reconciliation, renewal and worker polling are asynchronous. Each row has 96 start, 96 status, 96 cancel requests. Successful combined runs: 72 completed/checkpointed, 0 failed; API-only enqueued requests are cancelled.

| run | mode | API ops/s | start p50/p95 | status p50/p95 | cancel p50/p95 | wait n, p50/p95 | DB ms | combined work elapsed ms |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | API | 189.5 | 42.908/46.661 | 28.091/41.060 | 55.499/68.906 | 512, 16.999/39.188 | 11.307 | — |
| 1 | combined | 88.9 | 46.096/827.390 | 31.666/47.618 | 67.741/578.874 | 2330, 0.838/150.686 | 58.015 | 14125 |
| 2 | API | 187.4 | 43.874/45.721 | 28.835/41.949 | 56.238/69.535 | 512, 16.991/39.576 | 10.777 | — |
| 2 | combined | 132.1 | 45.493/189.378 | 30.810/45.702 | 66.391/371.938 | 2578, 0.090/143.548 | 64.208 | 14122 |
| 3 | API | 191.5 | 42.473/44.385 | 27.419/40.570 | 55.424/68.474 | 512, 16.927/38.448 | 10.679 | — |
| 3 | combined | 128.2 | 45.725/185.915 | 33.381/53.129 | 70.441/356.682 | 2186, 1.684/150.059 | 50.315 | 6366 |
| 4 | API | 190.3 | 42.542/46.908 | 27.394/40.710 | 56.399/68.994 | 512, 16.757/38.523 | 10.901 | — |
| 4 | combined | 135.8 | 44.905/197.283 | 29.863/43.990 | 63.149/379.781 | 2679, 0.002/148.028 | 65.004 | 14060 |
| 5 | API | 191.6 | 42.371/44.689 | 27.880/40.832 | 55.419/67.844 | 512, 16.971/38.491 | 10.707 | — |
| 5 | combined | 132.7 | 45.077/187.583 | 31.239/45.716 | 66.924/370.719 | 2407, 0.869/147.535 | 56.913 | 9993 |

## Baseline / comparability / caveats

- Median across five runs: API-only 190.3 API ops/s, wait p50/p95 16.971/38.523ms; combined 132.1 API ops/s, wait p50/p95 0.838/148.028ms. The combined lock wait distribution includes many nearly uncontended worker calls, so its p50 cannot be compared directly with the API-only wait p50. Combined p95 wait (~144–151ms) is large relative to start p95 (~186–197ms) in the four stable runs, while aggregate DB SQL time stays ~50–65ms over the entire combined mode; on this workload serialized connection/mutex work and client-side transport/scheduling dominate server execution time. This is evidence for examining contention, **not** evidence that a pool will improve p95 or preserve fencing.
- Five API-only runs are comparable: 187.4–191.6 ops/s (2.2% span of median), p95 wait 38.448–39.576ms (2.9% span). Combined runs 2–5 are comparable for API throughput (128.2–135.8 ops/s, 5.8% span of median), start p95 (185.915–197.283ms, 6.0% span) and lock p95 (143.548–150.059ms, 4.4% span). Combined **run 1 is an outlier**: 88.9 ops/s, start p95 827.390ms, wait p99 1228.653ms (versus 179–193ms for runs 2–5). Worker wall completion is variable (6.4–14.1s) even when API metrics mostly agree. Do not claim whole-run timing stability or universal speedups from these data; use repeated paired post-change runs on the same host and investigate/repeat any new outlier.
- Before the final five, a 900ms lease diagnostic run failed 32 claims (`owner lease expired`) under contention; the 5000ms lease and 4200ms long task produced renewal and 72/72 correct completed/checkpointed in every final run. A first 12s worker-completion timeout likewise failed once with 71 completed and one running, so the focused harness now has a 30s ceiling. These failed exploratory runs are **not** included in the baseline. The temporary lock-print diagnostic and blocking tasks amplify scheduling variance; consider fixed CPU isolation and less-intrusive histograms if a connection choice hinges on small differences.
- Target dir grew from 4.9 GiB at start to 9.4 GiB after instrumented compile; disk free during measured runs: `/tmp` overlay ~900 GiB, worktree mounted filesystem ~93 GiB free. No disk exhaustion was observed. No full suite was run for this measurement task.
