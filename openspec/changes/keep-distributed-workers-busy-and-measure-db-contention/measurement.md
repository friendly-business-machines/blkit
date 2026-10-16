# PostgreSQL single-connection baseline (task 1.1)

Measured on 2026-10-16 UTC using the unchanged `34f54272` store/worker, **before** changing worker refill or connection management. The five-run baseline below supersedes the original task-1.1 run summary: the reviewer identified mismatched API and worker-completion timing windows. No pool decision follows from this baseline alone; repeat the exact workload after refill before evaluating connection changes.

## Environment and fixed workload

- Linux 6.18.54.1-microsoft-standard-WSL2, x86_64 AMD Ryzen 9 5900X (24 logical CPUs), 31 GiB RAM (~27 GiB available); Rust/cargo 1.96.1; Docker server 28.5.1. Shared WSL2 host, not CPU-isolated.
- One disposable `postgres:17.6-alpine` Docker container (PostgreSQL 17.6, Alpine/musl), default `max_connections=100`, `shared_buffers=128 MiB`, `track_activities=on`, `track_io_timing=off`; only nondefault server option `shared_preload_libraries=pg_stat_statements`. No CPU/memory cgroup quota; connection from test process via `host.docker.internal:15439` to mapped PostgreSQL port. Database: `postgres` (disposable, no user data; explicit `BLKIT_MEASURE_DISPOSABLE=yes` required before TRUNCATE). One `PostgresStore::connect` shared by API, worker and reconciler, plus a separate **observer** connection for server statistics. Disposable password not recorded.
- Each mode starts with `TRUNCATE instances, workers, process_policies CASCADE`, re-registers one capable worker, prequeues only combined-mode work, then resets `pg_stat_statements`; no warm-up discarded. API-only: eight concurrent clients × twelve cycles of POST start (input 400ms), GET status, POST cancel: 96 of each, 288 requests. An in-process Axum `router_distributed(...).oneshot(...)` exercises actual handlers without HTTP socket/network overhead. A 100ms reconciler runs in the same test process. Worker capability enables API admission but no worker executes in API-only mode.
- Combined: same API clients/reconciler, plus one `DistributedWorker` using the **same cloned store/connection**: limit=32, lease=5000ms, eight prequeued 4200ms and 64 prequeued 15ms tasks, a graph with checkpoint writes. Worker calls `run_once` in a 100ms outer loop. Tasks sleep in `spawn_blocking`, not PostgreSQL. Each successful run asserts 72 prequeued completed, 72 checkpointed, zero failed. Prequeue/registration SQL is excluded from measured windows.
- `tools/dev-harness/tests/postgres_contention.rs` writes mode begin, API-end, and full-end markers. Apply the reversible `measurement-instrumentation.patch` only while measuring; it times all 23 existing Arc<tokio::sync::Mutex<Client>> acquisitions immediately before `.lock().await` until guard acquisition, emitting `LOCK_WAIT_US` (integer microseconds, truncated). Because `eprintln!` executes **while holding the mutex**, printing lengthens the critical section and biases absolute timings. Both sides of a pre/post comparison must use this *same* diagnostic, or both must be remeasured with a consistently lower-overhead diagnostic. No instrumentation persists in product source.
- API-window wait samples include all acquired store locks (API/worker/reconciler) from mode begin through the API-end marker. Full-work samples include those plus locks acquired until all queued jobs finish. A waiter begun before the API marker but acquiring afterwards counts only in full-work. Both are pooled per-lock acquisition, **not** per-API-call wait. `pg_stat_statements` `sum(total_exec_time)` (ms) is server SQL execution time, not wall time; it excludes client/network wait, task sleep and mutex acquisition. The API snapshot runs immediately after API-end, *without pausing* worker/reconciler, so it may include statements completing in the observer lag (1.993–3.150ms here); the full snapshot follows worker completion. SQL observer query itself is negligible but included in later snapshots. API throughput = 288 completed requests / elapsed time until the final API response. Percentile = sorted sample at zero-indexed `floor(n*p/100)`.

## Reproduce

From this worktree on a machine with Docker socket access (supply a disposable password privately, do not log it):

```sh
# read -rs PG_PASSWORD; echo    # use only a disposable database
# Use an unused host port, matching this example.
docker run -d --rm --name blkit-db-refill-measure \
  -e "POSTGRES_PASSWORD=$PG_PASSWORD" -p 127.0.0.1:15439:5432 \
  postgres:17.6-alpine -c shared_preload_libraries=pg_stat_statements
# Wait for: docker exec blkit-db-refill-measure pg_isready -U postgres
# With BLKIT_TESTCONTAINERS_HOST=host.docker.internal on this WSL2 dev host:
export BLKIT_MEASURE_DATABASE_URL="postgres://postgres:$PG_PASSWORD@host.docker.internal:15439/postgres"
export BLKIT_MEASURE_DISPOSABLE=yes  # mandatory explicit opt-in before truncation
export CARGO_TARGET_DIR=/tmp/blkit-db-refill-target.LmFOuM
git apply openspec/changes/keep-distributed-workers-busy-and-measure-db-contention/measurement-instrumentation.patch
for i in 1 2 3 4 5; do
  cargo test -q -p blkit-dev-harness --test postgres_contention -- --nocapture >"/tmp/blkit-contention-review-$i.log" 2>&1 || break
  grep '^mode=' "/tmp/blkit-contention-review-$i.log"
  df -h /tmp .; du -sh "$CARGO_TARGET_DIR"
done
git apply -R openspec/changes/keep-distributed-workers-busy-and-measure-db-contention/measurement-instrumentation.patch
docker rm -f blkit-db-refill-measure
unset BLKIT_MEASURE_DATABASE_URL BLKIT_MEASURE_DISPOSABLE PG_PASSWORD
```

`127.0.0.1:15439` was not reachable *inside* this WSL2 dev container; `host.docker.internal:15439` was. Parse logged wait samples with only the standard library (logs are outside the repository):

```sh
python3 - <<'PY'
from pathlib import Path
for run in range(1, 6):
    mode = window = None
    waits = {(mode, window): [] for mode in ('api', 'combined') for window in ('api', 'full')}
    for line in Path(f'/tmp/blkit-contention-review-{run}.log').read_text().splitlines():
        if line.startswith('mode=api phase=begin'): mode, window = 'api', 'api'
        elif line.startswith('mode=combined phase=begin'): mode, window = 'combined', 'api'
        elif 'phase=api-end' in line: window = 'full'
        elif 'phase=end' in line: mode = window = None
        elif line.startswith('LOCK_WAIT_US ') and mode and window:
            waits[mode, 'full'].append(int(line.split()[1]))
            if window == 'api': waits[mode, 'api'].append(int(line.split()[1]))
    for (mode, window), values in waits.items():
        values.sort()
        assert values, (run, mode, window)
        n = len(values)
        print(run, mode, window, n, values[n // 2], values[n * 95 // 100], values[n * 99 // 100])
PY
```

## Raw summary: API window (corrected baseline)

All durations below are **milliseconds**, including operation and wait p50/p95 (converted from raw microseconds); DB API is the near-immediate PostgreSQL snapshot. Each mode has 96 start/status/cancel operations, and API-only has 512 sampled lock acquisitions. These runs replace the original summary, which compared API throughput against combined full-work wait and DB time.

| run | mode | API ops/s | start p50/p95 | status p50/p95 | cancel p50/p95 | API wait n, p50/p95 | DB API ms |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | API | 187.0 | 43.473/45.877 | 28.568/41.106 | 57.480/71.453 | 512, 17.094/39.441 | 10.862 |
| 1 | combined | 131.6 | 46.305/207.286 | 30.576/46.658 | 66.226/381.421 | 704, 27.411/196.258 | 16.810 |
| 2 | API | 190.3 | 42.523/49.623 | 27.901/40.907 | 55.348/68.336 | 512, 16.830/38.835 | 10.835 |
| 2 | combined | 134.3 | 45.672/200.307 | 29.948/45.969 | 64.697/377.761 | 703, 26.514/193.277 | 17.057 |
| 3 | API | 188.3 | 43.348/46.213 | 28.397/41.592 | 56.073/69.214 | 512, 16.914/39.630 | 10.930 |
| 3 | combined | 128.2 | 46.160/182.168 | 33.774/53.402 | 69.880/355.042 | 768, 30.052/173.851 | 17.923 |
| 4 | API | 189.1 | 43.236/46.720 | 28.323/41.573 | 55.594/69.257 | 512, 16.891/39.466 | 10.878 |
| 4 | combined | 106.9 | 46.014/373.119 | 31.305/46.854 | 66.647/668.086 | 715, 28.739/540.790 | 17.384 |
| 5 | API | 186.8 | 43.914/46.493 | 28.754/42.143 | 56.114/69.680 | 512, 16.828/39.856 | 10.919 |
| 5 | combined | 129.4 | 48.466/197.960 | 31.657/46.279 | 68.269/383.361 | 728, 28.851/191.271 | 17.359 |

## Full-work totals (combined only; **not** the API throughput window)

All five had 72 completed and checkpointed, zero failed. Worker completion is independent of when the API requests finish.

| run | full wait n, p50/p95 (ms) | DB full ms | work elapsed ms |
| --- | ---: | ---: | ---: |
| 1 | 2446, 0.837/152.824 | 59.311 | 10429 |
| 2 | 2451, 0.866/146.163 | 61.103 | 10015 |
| 3 | 2187, 1.657/150.521 | 50.505 | 6353 |
| 4 | 2349, 0.823/149.856 | 56.957 | 10029 |
| 5 | 2570, 0.082/149.097 | 65.396 | 14073 |

## Comparability and limits

- Across five **API-window** runs: API-only median 188.3 ops/s, p50/p95 wait 16.891/39.466ms, DB SQL time 10.878ms; combined median 129.4 ops/s, p50/p95 wait 28.739/193.277ms, DB SQL time 17.359ms. Combined API-window p95 wait is near start p95 (~182–207ms in the four clustered runs) while total server SQL execution is ~17ms. This warrants investigation, **not** a pool decision without a paired prototype and correctness checks.
- API-only throughput range 186.8–190.3 ops/s (1.9% of median span), API-window p95 wait 38.835–39.856ms (2.6%). Combined runs 1, 2, 3, 5 cluster at 128.2–134.3 ops/s (4.7% of median) with p95 wait 173.851–196.258ms (~12% span); combined **run 4 is an outlier** at 106.9 ops/s and 540.790ms API-window p95 wait. Worker completion spans 6.4–14.1s. Preserve the outlier rather than claiming all runs are stable; repeat paired post-refill runs and compare **API-window against API-window** for both wait and DB time.
- The observer query can include SQL finishing in the 1.993–3.150ms interval after API-end; it is an approximate boundary, unlike API throughput's end timestamp. Logging under lock and the shared WSL2 host perturb timing; use the identical diagnostic when comparing before/after. These in-process HTTP-router timings cannot establish uninstrumented end-to-end network p95.
- Before these corrected runs, a 900ms lease exploratory run failed 32 claims (`owner lease expired`) under heavy contention; 5000ms lease/4200ms long tasks completed all 72 with checkpoints in each corrected run. An initial 12s worker timeout expired with 71/72 done, so the harness uses a 30s ceiling. Failed exploratory runs are not included. Target dir was 11 GiB throughout remeasurement; disk free was ~899 GiB for `/tmp` overlay and ~93 GiB for the worktree mount. No full suite was run.
