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

# Post-refill measurement (task 3.1)

Measured on 2026-10-16 UTC at `94704217`, after the worker refill change. Five fresh paired runs (API-only followed by combined within each invocation) used the **unchanged** `tools/dev-harness/tests/postgres_contention.rs`, the same temporary 23-site `measurement-instrumentation.patch`, and the same setup, admission, eight clients × twelve cycles (96 each of start/status/cancel per mode), 100ms reconciler, worker limit=32, lease=5000ms, eight 4200ms + 64 15ms prequeued tasks, checkpoints, 100ms worker outer loop, and 30s ceiling described above. No warm-up was discarded. The single shared store connection and separate statistics observer were unchanged; the only product behavior difference was worker refill. All five combined runs asserted **72 completed, 72 checkpointed, zero failed**. API-only runs had no executing worker.

## Environment and commands

Same shared Linux 6.18.54.1-microsoft-standard-WSL2 x86_64 Ryzen 9 5900X (24 logical CPUs), 31 GiB RAM (~28 GiB available at start), Rust/cargo 1.96.1, Docker server 28.5.1. A newly created disposable `postgres:17.6-alpine` container used `max_connections=100`, `shared_buffers=128MB`, `track_activities=on`, `track_io_timing=off`, and only `shared_preload_libraries=pg_stat_statements` as a nondefault option; no cgroup quota. Its mapped port was `127.0.0.1:15439`, reached from the test process via `host.docker.internal:15439`, database `postgres`, `NoTls`. The disposable password was generated privately and is not recorded. Target `/tmp/blkit-db-refill-target.LmFOuM` was 17 GiB throughout; `/tmp` overlay had ~893 GiB free and the worktree mount ~93 GiB free (96% used). No full suite was run.

Reproduction uses the baseline command sequence above, with `BLKIT_MEASURE_DISPOSABLE=yes` explicitly set, a **new disposable database only**, `CARGO_TARGET_DIR=/tmp/blkit-db-refill-target.LmFOuM` exported before every Cargo invocation (including nested builds), and log names `/tmp/blkit-contention-post-refill-{1..5}.log`. The password was generated with `openssl rand -hex 24` into a private shell variable; neither the database URL nor the password was echoed. Specifically: `docker run -d --rm --name blkit-db-refill-measure -e "POSTGRES_PASSWORD=$PG_PASSWORD" -p 127.0.0.1:15439:5432 postgres:17.6-alpine -c shared_preload_libraries=pg_stat_statements`; wait for `docker exec blkit-db-refill-measure pg_isready -U postgres`; `git apply openspec/changes/keep-distributed-workers-busy-and-measure-db-contention/measurement-instrumentation.patch`; for `i` from 1 to 5, `cargo test -q -p blkit-dev-harness --test postgres_contention -- --nocapture >"/tmp/blkit-contention-post-refill-$i.log" 2>&1`, followed by `grep '^mode='`, `df -h /tmp .`, and `du -sh "$CARGO_TARGET_DIR"`. An exit trap reversed the patch, removed the container, and unset the URL/opt-in/password variables. All five tests passed; no benchmark ran without the explicit disposable opt-in.

Wait extraction used the baseline standard-library Python marker parser above, replacing `/tmp/blkit-contention-review-{run}.log` with `/tmp/blkit-contention-post-refill-{run}.log` and converting microseconds to milliseconds. It confirmed 512 API-only acquisitions in each API/full window, nonempty combined API/full windows, one completed mode-end marker per mode, and one `completed=72 checkpointed=72 failed=0` marker per run. Percentiles use the baseline sorted-index rule; API waits include only acquisitions from begin to API-end. DB API snapshots followed API-end by 2.013–3.144ms, potentially including SQL finishing in that interval. Full-window counts include acquisitions until the end marker, as in the baseline; the full DB query follows work completion.

## Raw summary: API window after refill

Durations and DB SQL execution totals are milliseconds; each mode has 96 observations per operation and 288 API requests. Wait is per acquired store lock (API, worker, or reconciler), not per request. DB API is `pg_stat_statements` summed server execution time at the immediate API snapshot.

| run | mode | API ops/s | start p50/p95 | status p50/p95 | cancel p50/p95 | API wait n, p50/p95 | DB API ms |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | API | 186.5 | 44.010/46.412 | 28.694/42.118 | 55.991/69.670 | 512, 16.867/40.151 | 10.744 |
| 1 | combined | 130.1 | 47.396/198.195 | 31.600/49.931 | 67.051/377.311 | 717, 28.505/191.189 | 17.285 |
| 2 | API | 193.0 | 41.961/44.398 | 27.597/40.648 | 55.129/67.548 | 512, 16.393/38.471 | 10.624 |
| 2 | combined | 129.2 | 46.050/182.043 | 33.652/56.074 | 68.169/353.828 | 767, 29.692/173.927 | 18.481 |
| 3 | API | 174.5 | 42.683/177.602 | 28.567/41.462 | 55.816/69.088 | 512, 17.159/39.514 | 10.890 |
| 3 | combined | 130.5 | 47.295/185.326 | 33.385/52.612 | 67.791/354.935 | 753, 29.235/177.096 | 18.007 |
| 4 | API | 189.9 | 42.926/45.156 | 28.174/41.427 | 55.526/68.781 | 512, 16.866/38.944 | 11.043 |
| 4 | combined | 131.8 | 45.872/191.556 | 32.745/50.820 | 67.574/363.301 | 728, 28.746/182.704 | 17.704 |
| 5 | API | 189.8 | 42.737/45.015 | 28.263/41.027 | 56.156/69.092 | 512, 16.873/38.782 | 11.066 |
| 5 | combined | 135.2 | 45.406/204.065 | 30.305/44.099 | 62.963/375.800 | 692, 26.511/194.085 | 17.000 |

## Full-window totals after refill (not API throughput)

API-only full-window wait count and p50/p95 equal its API window because the mode has no worker. Full DB includes the earlier API observer query; combined work elapsed is measured from mode begin until all prequeued tasks complete, independently of API-end. All combined rows had 72 completed and checkpointed, zero failed.

| run | mode | full wait n, p50/p95 (ms) | DB full ms | work elapsed ms | completed/checkpointed/failed |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 | API | 512, 16.867/40.151 | 10.802 | — | — |
| 1 | combined | 2387, 6.518/97.174 | 62.968 | 8121 | 72/72/0 |
| 2 | API | 512, 16.393/38.471 | 10.685 | — | — |
| 2 | combined | 1979, 14.924/133.073 | 50.941 | 5337 | 72/72/0 |
| 3 | API | 512, 17.159/39.514 | 10.952 | — | — |
| 3 | combined | 2086, 14.519/135.815 | 53.657 | 6693 | 72/72/0 |
| 4 | API | 512, 16.866/38.944 | 11.110 | — | — |
| 4 | combined | 2292, 10.425/94.039 | 59.722 | 7672 | 72/72/0 |
| 5 | API | 512, 16.873/38.782 | 11.130 | — | — |
| 5 | combined | 2385, 6.597/64.786 | 60.858 | 7139 | 72/72/0 |

## Baseline comparison and task-3.1 assessment

Medians across **all five runs**, comparing identical API windows: API-only throughput 188.3 → 189.8 ops/s (+0.8%), wait p50/p95 16.891/39.466 → 16.867/38.944ms, SQL execution 10.878 → 10.890ms. Combined throughput 129.4 → 130.5 ops/s (+0.9%), wait p50/p95 28.739/193.277 → 28.746/182.704ms, SQL execution 17.359 → 17.704ms. These small shifts do **not** demonstrate a consistent API performance improvement from refill. The baseline combined run 4 was a pronounced outlier (106.9 ops/s and 540.790ms API-window wait p95), absent in the post-refill combined runs (129.2–135.2 ops/s, 173.927–194.085ms wait p95). Conversely post-refill API-only run 3 is an outlier (174.5 ops/s and start p95 177.602ms, versus 186.5–193.0 ops/s and start p95 44.398–46.412ms in the other four); its API wait p99 was 165.841ms, but p95 39.514ms. Preserve both, not an outlier-excluded universal speedup claim. Combined full-work elapsed was 5.3–8.1s post-refill versus 6.4–14.1s baseline, with variable lock sample counts and task scheduling; this is not the API window or controlled evidence of a universal completion-time speedup.

**Lock waiting remains material versus SQL execution time** in this diagnostic: post-refill combined API-window p95 acquisition wait is 174–194ms, close to start p95 182–204ms, with only 17–18.5ms summed server SQL execution across the entire API window. Even API-only wait p95 is ~38–40ms with ~10.6–11.1ms total SQL time. The per-lock wait and per-request latency distributions are not one-to-one, and summed SQL CPU/server time is not an operation percentile; nevertheless the relative magnitude indicates serialized client acquisition, not PostgreSQL SQL execution, dominates observed latency under this workload. As in baseline, `eprintln!` under the lock biases absolute wait upward, observer lag blurs SQL window boundaries, WSL2 is shared, and in-process router measurements exclude network/TLS costs. Task 3.1 makes **no connection/pool change or trial**; the bounded prototype/correctness decision belongs to task 3.2.
