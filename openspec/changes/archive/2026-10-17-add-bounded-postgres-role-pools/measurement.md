# Single-client baseline — task 1.1

Measured 2026-10-17 UTC with the task-1.1 harness at `22e635916fd756449a1565a3a249d1df1ab91179` and the unchanged single-client `PostgresStore` from base `219d09596a4ec1bc5ee9752a350bd0732a7bbb5e`, before any permanent pool change. Five successive, uninstrumented test invocations on one privately provisioned disposable PostgreSQL database passed in all three modes. This is a baseline, **not** a pool-size/default decision; task 4.1 owns the size sweep and decision. Raw test logs: `/tmp/blkit-task11-baseline-{1..5}.log` on the measurement host (ephemeral, not committed).

## Environment and reproduction

Shared WSL2 Linux 6.18.54.1 x86_64; Ryzen 9 5900X, 24 logical CPUs; 31 GiB RAM (~26 GiB available at inspection); Rust/Cargo 1.96.1; Docker 28.5.1. Own `postgres:17.6-alpine` container, no CPU/memory quota, default `max_connections=100`, `shared_buffers=128MB`, `track_io_timing=off`, with `shared_preload_libraries=pg_stat_statements`. `NoTls` connection through `host.docker.internal` to a Docker-selected separate mapped host port; one observer connection. No existing container/database was used. A private random 24-byte hex password was generated, never logged. Target directory `/tmp/blkit-pg-role-pools-target.PEwRyP` grew from ~842 MiB to 6.8 GiB; `/tmp` overlay had >=812 GiB available, worktree mount ~93 GiB available (96% used). No CPU isolation or warm-up discard.

Run the following baseline commands from a **separate disposable checkout** at task-1.1 harness commit `22e635916fd756449a1565a3a249d1df1ab91179` (single-client store unchanged from `219d09596a4ec1bc5ee9752a350bd0732a7bbb5e`), not from the current pooled feature checkout: its harness now requires `BLKIT_MEASURE_POOL_MAX`. Leave the active feature worktree untouched. Reproduce with your **own** disposable server; these commands never remove any container except the one they just created. Do not run the harness against a database containing valuable data: it TRUNCATEs tables and resets database statistics. In this environment, the host's `127.0.0.1` mapped port was reached via `host.docker.internal` from the workspace container.

```sh
set -e
name="blkit-pg-baseline-$(date +%s)-$$"
password="$(openssl rand -hex 24)"
docker run -d --rm --name "$name" -e "POSTGRES_PASSWORD=$password" \
  -p 127.0.0.1::5432 postgres:17.6-alpine \
  -c shared_preload_libraries=pg_stat_statements
# Clean up only the container created above, including on test failure.
trap 'docker rm -f "$name" >/dev/null 2>&1 || :; unset BLKIT_MEASURE_DATABASE_URL BLKIT_MEASURE_DISPOSABLE password' EXIT
until docker exec "$name" pg_isready -h 127.0.0.1 -U postgres >/dev/null 2>&1; do sleep 1; done
port="$(docker port "$name" 5432/tcp | sed 's/.*://')"
export BLKIT_MEASURE_DATABASE_URL="postgres://postgres:$password@host.docker.internal:$port/postgres"
export BLKIT_MEASURE_DISPOSABLE=yes
export CARGO_TARGET_DIR=/tmp/blkit-pg-role-pools-target.PEwRyP
for i in 1 2 3 4 5; do
  cargo test -q -p blkit-dev-harness --test postgres_contention -- --nocapture >"/tmp/blkit-task11-baseline-$i.log" 2>&1
  grep -E '^mode=|test result:' "/tmp/blkit-task11-baseline-$i.log"
  du -sh "$CARGO_TARGET_DIR"; df -h /tmp .
done
```

The real run used an equivalent private URL file (`0600`) and container-specific name; the URL/password were not echoed or written into the logs. `CARGO_TARGET_DIR` was exported for every Cargo invocation and nested build. The initial RED and subsequent GREEN focused tests used the same isolated server. The opt-in remains mandatory: if the URL is absent, the test skips; with a URL but without `BLKIT_MEASURE_DISPOSABLE=yes`, it fails before connecting/truncating.

## Workload and windows

Each mode first truncates `instances, workers, process_policies`, constructs the API/reconciler store and (for worker/combined) a separate worker store, advertises the worker, prequeues the fixed workload, counts server sessions, and resets `pg_stat_statements` **outside** the measured window. One 100ms reconciler runs in every measured mode. The API-only mode sends 8 concurrent in-process Axum router clients × 12 POST start/GET status/POST cancel cycles: 96 of each, 288 requests; there is no executing worker. The worker-only mode prequeues **256 checkpointing tasks of 15ms each** and runs the worker (limit 32, 5000ms lease, 100ms outer cadence), with no measured API requests; prequeue uses control only for setup. This short-task stream repeatedly exercises claims, checkpoint writes, completion and worker maintenance rather than measuring a long sleep. Combined retains the existing 8 prequeued 4200ms + 64 prequeued 15ms checkpointing tasks and the same API load and worker settings; every run asserted exactly **72 completed / 72 checkpointed / 0 failed**. Worker-only also asserts **256/256/0**. Data sets are held fixed *within each mode* for later pool-size comparisons; worker-only and combined intentionally differ.

`phase=begin` occurs after setup/reset. API elapsed ends when the last response is received (`phase=api-end`), before an immediate SQL snapshot; worker-only has no API window or snapshot. `work_elapsed_ms` measures begin through the observer's first 50ms-cadence poll finding no pending/running/waiting/retry-waiting instances, **not** worker task-body or SQL-only time; 30s ceiling. The full SQL snapshot follows the worker assertions (or immediately follows the API snapshot in API-only), then `phase=end`. `pg_stat_statements` sums server SQL execution times across all workload queries, not wall time or per-operation latency; its snapshot includes observer queries and can include statements finishing after API-end (2.009–2.633ms API snapshot lag). In-process router requests exclude socket/network/TLS overhead. API percentiles use sorted samples at index `floor(n*p/100)`. The uninstrumented runs in the table below do not contain client mutex waits or worker persistence operation latency. A separate **temporarily instrumented** repeat on the same committed harness and unchanged store behavior follows, using `tools/dev-harness/worker-persistence-diagnostic.patch`. It records real per-operation persistence latency and acquisition wait without logging under the mutex; it is not a pooled checkout-wait comparison. The patch was fully reversed; task 4.1 must use a consistently scoped pool diagnostic and label unlike windows. Single-client server session count (including observer) was asserted at begin: API 2, worker/combined 3; no per-run peak-session observer was run, and each baseline store has only its single session.

## Raw summaries

Units: API operation p50/p95 **milliseconds** (converted from integer microseconds printed by harness); SQL exec and elapsed **milliseconds**; rates completed API requests or completed work items per second. Each API operation has 96 observations. `SQL API` is the immediate API-end snapshot; `SQL full` is after completion. Worker-only has no API measurements. Each row's `sessions` includes the observer; all runs have one API/reconciler store connection, plus a separate worker connection in worker/combined.

| run | mode | sessions | API req/s | start p50/p95 | status p50/p95 | cancel p50/p95 | SQL API | work ms | work/s | SQL full | completion/checkpoint/fail |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | API | 2 | 185.6 | 44.409/48.094 | 29.123/42.504 | 56.070/70.260 | 10.773 | — | — | 10.831 | — |
| 1 | worker | 3 | — | — | — | — | — | 7070 | 36.2 | 113.218 | 256/256/0 |
| 1 | combined | 3 | 182.9 | 43.318/47.475 | 28.072/40.683 | 59.780/74.459 | 29.589 | 6641 | 10.8 | 67.063 | 72/72/0 |
| 2 | API | 2 | 145.5 | 42.176/44.991 | 28.174/510.844 | 54.726/67.458 | 11.125 | — | — | 11.197 | — |
| 2 | worker | 3 | — | — | — | — | — | 6906 | 37.1 | 114.419 | 256/256/0 |
| 2 | combined | 3 | 165.7 | 49.760/53.163 | 31.064/47.076 | 63.801/82.395 | 29.194 | 6485 | 11.1 | 65.241 | 72/72/0 |
| 3 | API | 2 | 190.1 | 42.829/45.784 | 27.936/41.108 | 55.273/69.282 | 14.029 | — | — | 14.089 | — |
| 3 | worker | 3 | — | — | — | — | — | 6911 | 37.0 | 112.280 | 256/256/0 |
| 3 | combined | 3 | 165.8 | 49.397/54.457 | 31.349/47.567 | 63.762/82.664 | 28.565 | 6216 | 11.6 | 62.703 | 72/72/0 |
| 4 | API | 2 | 189.7 | 42.756/45.296 | 28.187/41.080 | 56.014/69.362 | 10.966 | — | — | 11.027 | — |
| 4 | worker | 3 | — | — | — | — | — | 6960 | 36.8 | 111.553 | 256/256/0 |
| 4 | combined | 3 | 166.6 | 49.339/57.979 | 30.419/46.375 | 63.536/83.854 | 28.938 | 4469 | 16.1 | 55.896 | 72/72/0 |
| 5 | API | 2 | 186.9 | 43.794/46.145 | 28.543/42.011 | 56.174/69.369 | 11.201 | — | — | 11.259 | — |
| 5 | worker | 3 | — | — | — | — | — | 7019 | 36.5 | 110.126 | 256/256/0 |
| 5 | combined | 3 | 163.3 | 51.210/59.204 | 30.115/47.131 | 63.213/82.945 | 28.187 | 6510 | 11.1 | 66.624 | 72/72/0 |

Run 2 API status p95 510.844ms/145.5 req/s is an outlier, not discarded; run 4 combined completed markedly sooner with the same correctness result. API throughput median 186.9 req/s; worker median 36.8 work/s; combined API median 165.8 req/s, work median 11.1 work/s. The workload/observer/scheduling and separate mode-specific task sets preclude interpreting these as comparable per-query SQL costs or a promise that pooling helps. Re-run the same mode workloads, boundaries and observer method when comparing pool sizes.

## Scoped diagnostic: worker persistence operation latency (task 1.1 review fix)

Five **additional** successive runs on 2026-10-17 UTC used the exact same committed harness workloads, database/server configuration, opt-in, API/worker/combined order and sample percentile rule as above, on a new privately provisioned disposable `postgres:17.6-alpine` server with its own generated password and randomly mapped port. Source was the same base single-client `PostgresStore`; only the reversible `tools/dev-harness/worker-persistence-diagnostic.patch` was applied for these five runs and reversed immediately afterward. Logs are `/tmp/blkit-task11-fix-{1..5}.log` on the measurement host; `CARGO_TARGET_DIR=/tmp/blkit-pg-role-pools-target.PEwRyP` reached 7.1 GiB, with >=812 GiB overlay and ~93 GiB worktree free. All 15 mode windows passed; each worker-only window had 256/256/0, each combined window had **72/72/0**.

The patch times **real** `PostgresStore::commit_initial_checkpoint`, `commit_checkpoint`, and `finish_owned` calls on worker task execution, from immediately before the existing single-client mutex acquisition through completion of their actual PostgreSQL `execute` call. `total_us` includes mutex wait, client/network scheduling and SQL execution; `wait_us` ends at mutex acquisition. It records affected-row count and prints one `PERSIST_OP` line **after explicitly dropping the client guard**, never while holding the client mutex. Each full worker window has 256 successful writes of each type; each full combined window has 72. The combined mode had one extra unsuccessful initial checkpoint (`changed=0`) in runs 2, 3 and 5, consistent with a claimed/cancelled concurrent API instance; these three are excluded from successful-write percentiles and disclosed rather than reported as successes. No task-body sleep, `run_once` batch duration, prequeue write, claim transaction, heartbeat or reconcile SQL is mislabelled as a persistence operation. SQL execution totals in the original harness still include *all* operations, not just these three write types.

To reproduce this historical diagnostic, first copy `tools/dev-harness/worker-persistence-diagnostic.patch` from the current feature checkout into the **separate historical checkout** at the same relative path; the patch was added after commit `22e6359` and does not apply to the current pooled store. Create only your own disposable container and export private URL, `BLKIT_MEASURE_DISPOSABLE=yes`, and `CARGO_TARGET_DIR` exactly as in the historical reproduction block above. In that historical checkout, replace the original container-only EXIT trap with a trap that reverses **only** this patch and removes **only** the container you created; execute (with `set -e` still active):

```sh
git apply --check tools/dev-harness/worker-persistence-diagnostic.patch
git apply tools/dev-harness/worker-persistence-diagnostic.patch
trap 'git apply -R tools/dev-harness/worker-persistence-diagnostic.patch 2>/dev/null || :; docker rm -f "$name" >/dev/null 2>&1 || :; unset BLKIT_MEASURE_DATABASE_URL BLKIT_MEASURE_DISPOSABLE password' EXIT
for i in 1 2 3 4 5; do
  cargo test -q -p blkit-dev-harness --test postgres_contention -- --nocapture >"/tmp/blkit-task11-fix-$i.log" 2>&1
  grep -E '^mode=.*(phase=end|completed=)|test result:' "/tmp/blkit-task11-fix-$i.log"
  du -sh "$CARGO_TARGET_DIR"; df -h /tmp .
done
git apply -R tools/dev-harness/worker-persistence-diagnostic.patch
```

The actual run generated the patch from the temporary source diff, verified its reverse application, ran five successful invocations, then reversed it; `git status` confirmed no production source diff. Never apply to a real database: the harness truncates tables. Do not apply the archived 23-site `LOCK_WAIT_US` patch alongside this diagnostic; its print occurs *under* the mutex and alters the critical section. To re-extract the values below from the logs with only the Python standard library (run after the five invocations), use this exact marker/percentile rule:

```sh
python3 - <<'PY'
from pathlib import Path
from collections import defaultdict
for run in range(1, 6):
    mode = window = None
    samples = defaultdict(list)
    for line in Path(f'/tmp/blkit-task11-fix-{run}.log').read_text().splitlines():
        if line.startswith('mode=') and ' phase=begin' in line:
            mode = line.split()[0][5:]
            window = 'api' if mode == 'combined' else 'full'
        elif line.startswith('mode=combined phase=api-end'):
            window = 'full'
        elif line.startswith('mode=') and ' phase=end' in line:
            mode = window = None
        elif line.startswith('PERSIST_OP '):
            assert mode in ('worker', 'combined'), (run, line)
            fields = dict(item.split('=', 1) for item in line.split()[1:])
            if fields['changed'] != '1':
                continue
            for key in ('total_us', 'wait_us'):
                samples[mode, 'full', fields['kind'], key].append(int(fields[key]))
                if mode == 'combined' and window == 'api':
                    samples[mode, 'api', fields['kind'], key].append(int(fields[key]))
    for role in ('worker', 'combined'):
        for kind in ('initial_checkpoint', 'checkpoint', 'finish_owned'):
            total = sorted(samples[role, 'full', kind, 'total_us'])
            wait = sorted(samples[role, 'full', kind, 'wait_us'])
            assert len(total) == len(wait) == (256 if role == 'worker' else 72)
            print(run, role, kind, len(total), *(f'{v[len(v)//2]/1000:.3f}/{v[len(v)*95//100]/1000:.3f}' for v in (total, wait)))
PY
```

**Raw full-work window**: each `n` counts successful worker persistence operations, not task batches; `total` and `mutex wait` are p50/p95 in **ms**, sorted index `floor(n*p/100)`. The full window is from `phase=begin` until `phase=end` (including the observer query/assertion interval after work finishes). In practice all 72/256 successful writes precede the completion observer; an acquisition started before a marker but logged after it would be assigned to the later window. For API-window subsets, use `phase=api-end` as the cutoff; observer lag and stderr/stdout ordering mean the boundary is approximate, not an atomic timestamp.

| run | mode | operation | n | total p50/p95 ms | mutex wait p50/p95 ms |
| --- | --- | --- | ---: | ---: | ---: |
| 1 | worker | initial checkpoint | 256 | 19.877/109.797 | 14.907/104.603 |
| 1 | worker | checkpoint | 256 | 16.527/163.146 | 11.530/157.974 |
| 1 | worker | finish | 256 | 6.727/173.339 | 1.732/168.104 |
| 1 | combined | initial checkpoint | 72 | 29.185/173.886 | 23.295/169.022 |
| 1 | combined | checkpoint | 72 | 30.267/154.305 | 25.214/149.281 |
| 1 | combined | finish | 72 | 23.385/164.608 | 18.379/158.254 |
| 2 | worker | initial checkpoint | 256 | 19.732/109.164 | 14.805/104.150 |
| 2 | worker | checkpoint | 256 | 16.327/154.109 | 11.405/149.101 |
| 2 | worker | finish | 256 | 6.664/165.133 | 1.699/160.176 |
| 2 | combined | initial checkpoint | 72 | 27.654/170.271 | 21.780/165.487 |
| 2 | combined | checkpoint | 72 | 31.463/187.276 | 25.038/180.799 |
| 2 | combined | finish | 72 | 19.271/197.545 | 14.106/191.088 |
| 3 | worker | initial checkpoint | 256 | 20.039/109.044 | 15.005/103.997 |
| 3 | worker | checkpoint | 256 | 16.787/156.009 | 11.757/150.914 |
| 3 | worker | finish | 256 | 6.742/170.205 | 1.704/165.067 |
| 3 | combined | initial checkpoint | 72 | 30.373/164.664 | 24.536/159.677 |
| 3 | combined | checkpoint | 72 | 28.848/145.442 | 23.896/138.929 |
| 3 | combined | finish | 72 | 22.875/160.652 | 17.763/154.531 |
| 4 | worker | initial checkpoint | 256 | 19.495/99.907 | 14.658/95.192 |
| 4 | worker | checkpoint | 256 | 15.909/148.937 | 11.065/144.035 |
| 4 | worker | finish | 256 | 6.633/164.050 | 1.730/159.116 |
| 4 | combined | initial checkpoint | 72 | 27.382/182.169 | 21.098/175.949 |
| 4 | combined | checkpoint | 72 | 29.098/185.363 | 22.156/180.066 |
| 4 | combined | finish | 72 | 16.380/186.422 | 11.115/181.061 |
| 5 | worker | initial checkpoint | 256 | 19.468/105.668 | 14.618/100.750 |
| 5 | worker | checkpoint | 256 | 15.951/154.371 | 11.061/149.519 |
| 5 | worker | finish | 256 | 6.570/163.464 | 1.715/158.459 |
| 5 | combined | initial checkpoint | 72 | 29.539/167.429 | 24.185/162.675 |
| 5 | combined | checkpoint | 72 | 28.867/161.082 | 24.064/154.654 |
| 5 | combined | finish | 72 | 20.866/172.965 | 14.436/166.829 |

Combined **API-window** subsets for finish writes, taken from begin through API-end (n varies with concurrent worker progress), show the same boundary type as `db_api_exec_ms` above. Units ms; no API-only worker write exists.

| run | n | finish total p50/p95 | finish mutex wait p50/p95 | SQL API ms (this diagnostic run) | SQL full ms (this diagnostic run) |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 47 | 157.873/164.613 | 151.490/158.341 | 29.441 | 60.973 |
| 2 | 52 | 188.578/197.663 | 182.571/191.150 | 28.485 | 65.402 |
| 3 | 43 | 144.338/160.664 | 138.009/155.706 | 29.235 | 56.999 |
| 4 | 54 | 180.018/187.448 | 174.952/181.336 | 29.411 | 67.713 |
| 5 | 49 | 166.272/173.143 | 159.937/166.862 | 28.651 | 64.813 |

Worker-only SQL full totals in these five diagnostic runs were 121.674, 112.688, 111.424, 110.646, 111.663ms, with work elapsed 7168, 6920, 7073, 6860, 6802ms respectively. SQL server totals cannot be subtracted from an individual persistence latency percentile: they sum *all* server SQL. This temporary diagnostic's per-write `eprintln!` is outside the lock but still consumes CPU/I/O and may perturb scheduling; the original uninstrumented throughput/SQL table must not be spliced into this instrumented wait distribution as if from a single run. `total_us` contains more than wait+server execution (client response/network/scheduling) and is not a server SQL percentile. Baseline mutex waits are **not** numerically interchangeable with later pool checkout waits; compare equivalent write types/windows with a consistent post-pool diagnostic rather than claiming exact same-population percentiles. No default-size conclusion follows from these five baseline runs alone.

# Bounded-pool sweep — task 4.1 (default decision: four per role)

**The operator selected the conservative default of four per role to conserve PostgreSQL session capacity, not because four is the throughput optimum.** Three repeated, successful instrumented runs at every requested maximum (1, 4, 8, 16) show that four markedly improves on one but eight improves API throughput further. Defaulting to eight would double the worst-case per-role session budget; eight remains available as an explicit, capacity-budgeted setting after later deployment-specific stress testing.

## Reproduction and measurement scope

Measured 2026-10-17 UTC on the same shared WSL2/Ryzen 9 5900X/24-logical-CPU/31-GiB-RAM workspace as the original baseline, Rust/Cargo 1.96.1 and Docker 28.5.1. A **new, private** `postgres:17.6-alpine` container with generated password, Docker-selected localhost-mapped port reached from the workspace through `host.docker.internal`, `shared_preload_libraries=pg_stat_statements`, `max_connections=100`, `superuser_reserved_connections=3`, `reserved_connections=0` (97 ordinary connection slots), `shared_buffers=128MB`, `track_io_timing=off`, no CPU/memory quotas. No other database/container was used or removed. Combined worst-case is 16 API + 16 worker + two observer sessions = 34, below 97; observed maximum was 27. The cargo target reached ~108 GiB, `/tmp` overlay retained >=711 GiB free and worktree mount ~92 GiB free. No CPU isolation or discarded warm-up run.

These commands use **only a new disposable server**. The harness truncates tables and resets SQL statistics: never point it at a persistent/shared database. Do not print the password or the URL. The diagnostic patch is applied once and reversed; the pre-pool `worker-persistence-diagnostic.patch` above is **not** applied alongside it. Raw logs are ephemeral `/tmp/blkit-task41-pool-{1,4,8,16}-{1,2,3}.log`, not committed. The run command used the test filter shown below to avoid the separate focused budget test perturbing the mode windows.

```sh
set -e
name="blkit-task41-$(date +%s)-$$"
password="$(openssl rand -hex 24)"
docker run -d --rm --name "$name" -e "POSTGRES_PASSWORD=$password" \
  -p 127.0.0.1::5432 postgres:17.6-alpine \
  -c shared_preload_libraries=pg_stat_statements
trap 'git apply -R tools/dev-harness/pool-persistence-diagnostic.patch 2>/dev/null || :; docker rm -f "$name" >/dev/null 2>&1 || :; unset BLKIT_MEASURE_DATABASE_URL BLKIT_MEASURE_DISPOSABLE BLKIT_MEASURE_POOL_MAX password' EXIT
until docker exec "$name" pg_isready -h 127.0.0.1 -U postgres >/dev/null 2>&1; do sleep 1; done
port="$(docker port "$name" 5432/tcp | sed 's/.*://')"
export BLKIT_MEASURE_DATABASE_URL="postgres://postgres:$password@host.docker.internal:$port/postgres"
export BLKIT_MEASURE_DISPOSABLE=yes
export CARGO_TARGET_DIR=/tmp/blkit-pg-role-pools-target.PEwRyP
git apply --check tools/dev-harness/pool-persistence-diagnostic.patch
git apply tools/dev-harness/pool-persistence-diagnostic.patch
for max in 1 4 8 16; do
  export BLKIT_MEASURE_POOL_MAX="$max"
  for run in 1 2 3; do
    cargo test -q -p blkit-dev-harness --test postgres_contention \
      measure_api_worker_and_combined_connections -- --nocapture --test-threads=1 \
      >"/tmp/blkit-task41-pool-$max-$run.log" 2>&1
    grep -E '^mode=.*(pool_max=.*peak_sessions|completed=)|test result:' \
      "/tmp/blkit-task41-pool-$max-$run.log"
    du -sh "$CARGO_TARGET_DIR"; df -h /tmp .
  done
done
git apply -R tools/dev-harness/pool-persistence-diagnostic.patch
```

Each invocation runs API-only, worker-only and combined in that order; the mode-specific task sets, 8 × 12 API start/status/cancel cycles, worker limit 32, 100-ms worker/reconciler cadence, 5,000-ms lease and 30-s work ceiling are exactly those defined above for the original baseline. Each mode creates fresh API/reconciler and (when applicable) worker stores at the **same** selected maximum; setup and reset precede `phase=begin`. Mode and repeat do not change concurrency or request count. Pooled stores and the sampling observer use mode-scoped `application_name` tags for counting, with the untagged main observer counted separately; `begin/end` include the main observer and role sessions, while `peak` adds the continuously running second observer and polls every 5 ms from begin until after work/requests and assertions. A poll can miss a short-lived transient peak, but the observed peaks did not exceed max-per-role + two observers. Lazy pool creation accounts for peak below configured 16 in API/combined. Every worker-only window asserted 256 completed / 256 checkpointed / 0 failed; all **12 combined windows asserted 72 / 72 / 0**.

API rate spans `phase=begin` through the last in-process router response (`phase=api-end`); each start/status/cancel percentile times the corresponding router request/response (96 samples per operation). There is no API window for worker-only. Work rate spans begin through the first 50-ms-cadence observer poll finding no active work, not task-only execution; combined work may outlast its API window. The API SQL snapshot is queried just after api-end; the full SQL snapshot is queried after completion/assertions and after stopping the session sampler. `pg_stat_statements` execution milliseconds sum **all** statements (including observer SQL), not an individual operation's SQL latency. Its snapshots include possible late finishes after the API marker and are not a client wall-clock timer. The sampler itself adds SQL, extra sessions and scheduling overhead absent from the original uninstrumented baseline; diagnostic `eprintln!` adds worker I/O. All pooled rates below therefore belong to these **instrumented** repeats, not the baseline's uninstrumented rate table.

The reversible pooled patch times only the real `commit_initial_checkpoint`, `commit_checkpoint` and `finish_owned` calls: `total_us` starts immediately before `Pool::get()` and ends after the SQL `execute`, while `wait_us` stops on successful checkout. Logging is after dropping the checked-out client. The patch retains the actual write and affected-row checks; unsuccessful writes (`changed=0`) are listed separately and excluded from successful-write percentiles. It does not measure worker claim/renewal, reconciliation, prequeue, API store checkout, task-body sleep, or SQL-server-only operation latency. Full worker windows have 256 successful samples **per write type** and full combined windows have 72 per type. Percentiles sort microsecond samples and take index `floor(n*p/100)`, converted to milliseconds below; values rounded to three decimals. Unlike the baseline's original mutex lock, pooled checkout may include connection creation/recycling and bounded waiting. The baseline diagnostic and this patch instrument similarly named worker writes, but mutex and checkout wait distributions are **not** interchangeable and were gathered on separate containers/runs. In-process router requests omit network/socket/TLS transport, shared WSL scheduling is uncontrolled, and other PostgreSQL deployments may behave differently.

## Raw repeated pool results

Sessions are actual sampled PostgreSQL sessions (including observer(s)); `polls` gives sampling coverage. API operation columns are p50/p95 **ms**; SQL times **ms**; rates API requests or completed work items per second. The per-run API, worker and combined result is kept rather than replacing the repeats by means.

| max | repeat | mode | begin sessions | end sessions | peak sessions | polls | API req/s | work/s | SQL API ms | SQL full ms | start p50/p95 ms | status p50/p95 ms | cancel p50/p95 ms |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | 1 | api | 2 | 2 | 3 | 189 | 180.6 | — | 20.832 | 20.950 | 44.025/71.373 | 28.966/42.093 | 56.845/70.395 |
| 1 | 1 | worker | 3 | 2 | 4 | 856 | — | 36.2 | — | 177.488 | — | — | — |
| 1 | 1 | combined | 3 | 2 | 4 | 713 | 158.5 | 12.3 | 42.419 | 103.216 | 50.459/66.646 | 31.256/48.274 | 66.487/84.563 |
| 1 | 2 | api | 2 | 2 | 3 | 187 | 183.2 | — | 20.971 | 21.091 | 43.647/66.501 | 28.669/41.741 | 56.275/68.750 |
| 1 | 2 | worker | 3 | 2 | 4 | 875 | — | 35.4 | — | 181.004 | — | — | — |
| 1 | 2 | combined | 3 | 2 | 4 | 832 | 156.9 | 10.5 | 43.287 | 110.922 | 51.372/65.811 | 30.811/47.054 | 66.677/86.565 |
| 1 | 3 | api | 2 | 2 | 3 | 192 | 178.5 | — | 21.363 | 21.504 | 44.707/71.487 | 29.047/43.069 | 56.434/70.443 |
| 1 | 3 | worker | 3 | 2 | 4 | 918 | — | 32.4 | — | 233.838 | — | — | — |
| 1 | 3 | combined | 3 | 2 | 4 | 831 | 142.8 | 10.5 | 48.463 | 122.828 | 51.930/92.816 | 34.935/85.253 | 70.391/88.262 |
| 4 | 1 | api | 2 | 5 | 6 | 71 | 426.8 | — | 20.225 | 20.376 | 13.719/115.689 | 5.779/10.771 | 21.938/61.260 |
| 4 | 1 | worker | 3 | 2 | 7 | 432 | — | 71.1 | — | 160.395 | — | — | — |
| 4 | 1 | combined | 3 | 5 | 10 | 581 | 449.9 | 14.7 | 34.659 | 124.251 | 13.892/64.639 | 6.081/12.621 | 23.104/63.747 |
| 4 | 2 | api | 2 | 5 | 6 | 78 | 422.1 | — | 18.632 | 18.758 | 15.204/72.172 | 6.725/13.102 | 28.035/39.702 |
| 4 | 2 | worker | 3 | 2 | 7 | 439 | — | 70.1 | — | 159.780 | — | — | — |
| 4 | 2 | combined | 3 | 5 | 10 | 632 | 440.1 | 13.6 | 35.123 | 128.261 | 11.445/66.947 | 5.907/20.012 | 23.470/38.742 |
| 4 | 3 | api | 2 | 5 | 6 | 61 | 493.5 | — | 17.186 | 17.311 | 13.442/111.027 | 5.498/9.127 | 21.719/28.762 |
| 4 | 3 | worker | 3 | 2 | 7 | 435 | — | 70.1 | — | 164.625 | — | — | — |
| 4 | 3 | combined | 3 | 5 | 10 | 616 | 467.5 | 14.0 | 33.560 | 129.786 | 12.984/65.227 | 5.859/9.403 | 24.364/61.379 |
| 8 | 1 | api | 2 | 9 | 10 | 37 | 728.1 | — | 18.638 | 18.806 | 6.604/156.351 | 2.113/2.691 | 11.042/12.129 |
| 8 | 1 | worker | 3 | 2 | 11 | 424 | — | 71.9 | — | 167.766 | — | — | — |
| 8 | 1 | combined | 3 | 9 | 18 | 609 | 577.1 | 13.9 | 32.966 | 145.863 | 6.504/110.472 | 2.390/46.841 | 11.143/14.702 |
| 8 | 2 | api | 2 | 9 | 10 | 37 | 736.7 | — | 18.182 | 18.320 | 6.542/153.702 | 2.038/2.599 | 10.874/11.461 |
| 8 | 2 | worker | 3 | 2 | 11 | 414 | — | 74.4 | — | 164.213 | — | — | — |
| 8 | 2 | combined | 3 | 9 | 18 | 555 | 622.0 | 15.2 | 35.713 | 145.315 | 6.649/110.883 | 2.350/46.932 | 11.532/14.801 |
| 8 | 3 | api | 2 | 9 | 10 | 37 | 735.0 | — | 18.825 | 18.983 | 6.475/157.276 | 2.020/2.574 | 10.710/11.391 |
| 8 | 3 | worker | 3 | 2 | 11 | 425 | — | 71.8 | — | 176.157 | — | — | — |
| 8 | 3 | combined | 3 | 9 | 18 | 598 | 585.0 | 14.2 | 34.930 | 150.817 | 6.763/112.350 | 2.354/13.560 | 11.221/24.957 |
| 16 | 1 | api | 2 | 10 | 11 | 38 | 787.9 | — | 19.276 | 19.411 | 6.519/114.488 | 2.092/2.543 | 10.904/12.594 |
| 16 | 1 | worker | 3 | 2 | 19 | 418 | — | 72.2 | — | 186.607 | — | — | — |
| 16 | 1 | combined | 3 | 10 | 27 | 563 | 568.6 | 14.9 | 45.446 | 178.073 | 6.770/156.563 | 2.346/4.353 | 11.174/58.495 |
| 16 | 2 | api | 2 | 10 | 11 | 37 | 828.6 | — | 19.385 | 19.601 | 6.591/114.401 | 2.088/2.472 | 11.032/12.265 |
| 16 | 2 | worker | 3 | 2 | 19 | 495 | — | 60.9 | — | 212.835 | — | — | — |
| 16 | 2 | combined | 3 | 10 | 27 | 602 | 567.9 | 14.0 | 41.605 | 179.528 | 6.589/115.536 | 2.269/3.242 | 11.059/60.870 |
| 16 | 3 | api | 2 | 10 | 11 | 37 | 727.1 | — | 19.123 | 19.270 | 6.520/162.570 | 2.057/2.338 | 10.931/11.404 |
| 16 | 3 | worker | 3 | 2 | 19 | 418 | — | 72.8 | — | 197.855 | — | — | — |
| 16 | 3 | combined | 3 | 10 | 27 | 596 | 545.9 | 14.0 | 48.554 | 190.714 | 6.868/111.307 | 2.308/3.133 | 11.326/55.222 |

Worker persistence operation latency and scoped checkout wait (p50/p95 ms):

| max | repeat | mode | write | successful n | total p50/p95 | checkout p50/p95 | changed=0 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | 1 | worker | initial_checkpoint | 256 | 20.099/103.440 | 15.088/98.389 | 0 |
| 1 | 1 | worker | checkpoint | 256 | 16.855/154.765 | 11.862/149.713 | 0 |
| 1 | 1 | worker | finish_owned | 256 | 6.782/168.874 | 1.725/163.769 | 0 |
| 1 | 1 | combined | initial_checkpoint | 72 | 29.361/173.345 | 23.705/168.341 | 0 |
| 1 | 1 | combined | checkpoint | 72 | 29.786/167.309 | 24.770/162.516 | 0 |
| 1 | 1 | combined | finish_owned | 72 | 23.409/181.577 | 18.499/175.018 | 0 |
| 1 | 2 | worker | initial_checkpoint | 256 | 20.372/104.934 | 15.330/99.884 | 0 |
| 1 | 2 | worker | checkpoint | 256 | 17.318/157.585 | 12.239/152.381 | 0 |
| 1 | 2 | worker | finish_owned | 256 | 6.908/176.296 | 1.763/171.045 | 0 |
| 1 | 2 | combined | initial_checkpoint | 72 | 29.578/180.556 | 23.924/175.479 | 1 |
| 1 | 2 | combined | checkpoint | 72 | 28.841/158.975 | 23.368/152.291 | 0 |
| 1 | 2 | combined | finish_owned | 72 | 23.493/170.994 | 18.337/164.345 | 0 |
| 1 | 3 | worker | initial_checkpoint | 256 | 20.683/104.031 | 15.562/98.928 | 0 |
| 1 | 3 | worker | checkpoint | 256 | 17.863/155.365 | 12.689/150.315 | 0 |
| 1 | 3 | worker | finish_owned | 256 | 7.168/169.104 | 1.903/163.975 | 0 |
| 1 | 3 | combined | initial_checkpoint | 72 | 31.355/220.575 | 24.129/215.344 | 1 |
| 1 | 3 | combined | checkpoint | 72 | 29.217/216.081 | 23.786/210.552 | 0 |
| 1 | 3 | combined | finish_owned | 72 | 25.485/219.528 | 20.403/213.151 | 0 |
| 4 | 1 | worker | initial_checkpoint | 256 | 5.235/89.946 | 0.008/83.455 | 0 |
| 4 | 1 | worker | checkpoint | 256 | 6.832/45.210 | 0.009/38.638 | 0 |
| 4 | 1 | worker | finish_owned | 256 | 6.717/54.160 | 0.007/47.464 | 0 |
| 4 | 1 | combined | initial_checkpoint | 72 | 8.908/119.680 | 0.796/112.959 | 0 |
| 4 | 1 | combined | checkpoint | 72 | 7.710/63.029 | 0.012/56.000 | 0 |
| 4 | 1 | combined | finish_owned | 72 | 8.278/67.160 | 0.008/60.132 | 0 |
| 4 | 2 | worker | initial_checkpoint | 256 | 5.370/111.758 | 0.008/104.978 | 0 |
| 4 | 2 | worker | checkpoint | 256 | 6.678/58.678 | 0.008/50.422 | 0 |
| 4 | 2 | worker | finish_owned | 256 | 6.776/58.438 | 0.007/51.604 | 0 |
| 4 | 2 | combined | initial_checkpoint | 72 | 8.519/117.992 | 0.078/108.564 | 0 |
| 4 | 2 | combined | checkpoint | 72 | 7.197/47.537 | 0.011/40.570 | 0 |
| 4 | 2 | combined | finish_owned | 72 | 7.147/49.070 | 0.007/41.969 | 0 |
| 4 | 3 | worker | initial_checkpoint | 256 | 5.219/116.085 | 0.008/109.308 | 0 |
| 4 | 3 | worker | checkpoint | 256 | 7.585/58.103 | 0.009/51.297 | 0 |
| 4 | 3 | worker | finish_owned | 256 | 6.776/59.019 | 0.007/51.899 | 0 |
| 4 | 3 | combined | initial_checkpoint | 72 | 8.295/131.183 | 0.010/124.647 | 0 |
| 4 | 3 | combined | checkpoint | 72 | 7.959/60.028 | 0.010/52.730 | 0 |
| 4 | 3 | combined | finish_owned | 72 | 6.846/56.362 | 0.008/49.780 | 0 |
| 8 | 1 | worker | initial_checkpoint | 256 | 5.040/116.332 | 0.009/109.576 | 0 |
| 8 | 1 | worker | checkpoint | 256 | 7.426/26.471 | 0.009/20.009 | 0 |
| 8 | 1 | worker | finish_owned | 256 | 6.727/25.458 | 0.007/18.923 | 0 |
| 8 | 1 | combined | initial_checkpoint | 72 | 5.306/170.638 | 0.010/159.947 | 0 |
| 8 | 1 | combined | checkpoint | 72 | 7.383/34.261 | 0.010/27.471 | 0 |
| 8 | 1 | combined | finish_owned | 72 | 6.867/32.532 | 0.008/23.427 | 0 |
| 8 | 2 | worker | initial_checkpoint | 256 | 4.984/108.090 | 0.009/100.465 | 0 |
| 8 | 2 | worker | checkpoint | 256 | 7.485/30.014 | 0.009/23.656 | 0 |
| 8 | 2 | worker | finish_owned | 256 | 6.694/25.576 | 0.007/19.195 | 0 |
| 8 | 2 | combined | initial_checkpoint | 72 | 6.179/98.581 | 0.012/91.729 | 0 |
| 8 | 2 | combined | checkpoint | 72 | 7.911/24.878 | 0.009/18.312 | 0 |
| 8 | 2 | combined | finish_owned | 72 | 7.033/26.300 | 0.007/19.660 | 0 |
| 8 | 3 | worker | initial_checkpoint | 256 | 5.017/120.741 | 0.009/114.274 | 0 |
| 8 | 3 | worker | checkpoint | 256 | 7.453/27.203 | 0.009/20.772 | 0 |
| 8 | 3 | worker | finish_owned | 256 | 6.748/25.947 | 0.007/19.541 | 0 |
| 8 | 3 | combined | initial_checkpoint | 72 | 6.290/142.155 | 0.011/135.080 | 0 |
| 8 | 3 | combined | checkpoint | 72 | 7.666/57.854 | 0.011/48.192 | 0 |
| 8 | 3 | combined | finish_owned | 72 | 6.821/36.356 | 0.009/23.400 | 0 |
| 16 | 1 | worker | initial_checkpoint | 256 | 5.094/123.381 | 0.009/113.979 | 0 |
| 16 | 1 | worker | checkpoint | 256 | 7.435/8.421 | 0.009/1.306 | 0 |
| 16 | 1 | worker | finish_owned | 256 | 6.743/9.704 | 0.007/3.009 | 0 |
| 16 | 1 | combined | initial_checkpoint | 72 | 7.608/141.465 | 0.012/130.382 | 0 |
| 16 | 1 | combined | checkpoint | 72 | 7.622/17.834 | 0.010/10.331 | 0 |
| 16 | 1 | combined | finish_owned | 72 | 6.905/17.099 | 0.008/10.011 | 0 |
| 16 | 2 | worker | initial_checkpoint | 256 | 9.208/131.601 | 0.009/125.102 | 0 |
| 16 | 2 | worker | checkpoint | 256 | 8.083/14.189 | 0.009/1.944 | 0 |
| 16 | 2 | worker | finish_owned | 256 | 6.843/36.596 | 0.007/7.831 | 0 |
| 16 | 2 | combined | initial_checkpoint | 72 | 6.830/179.749 | 0.011/167.403 | 0 |
| 16 | 2 | combined | checkpoint | 72 | 7.368/13.608 | 0.010/3.439 | 0 |
| 16 | 2 | combined | finish_owned | 72 | 6.801/14.130 | 0.008/7.476 | 0 |
| 16 | 3 | worker | initial_checkpoint | 256 | 5.087/98.818 | 0.009/91.728 | 0 |
| 16 | 3 | worker | checkpoint | 256 | 7.690/13.300 | 0.009/5.271 | 0 |
| 16 | 3 | worker | finish_owned | 256 | 6.797/13.289 | 0.007/6.708 | 0 |
| 16 | 3 | combined | initial_checkpoint | 72 | 7.592/153.364 | 0.013/141.693 | 0 |
| 16 | 3 | combined | checkpoint | 72 | 7.880/12.438 | 0.010/4.672 | 0 |
| 16 | 3 | combined | finish_owned | 72 | 6.807/13.279 | 0.008/3.674 | 0 |

Only combined maximum 1 repeats 2 and 3 had one additional `changed=0` initial checkpoint each (a concurrent API cancellation); all successful writes and completed/checkpointed/failed assertions still passed. A marker and an `eprintln!` from another task are not an atomic timestamp, so the full-window classification is robust after task completion but any narrower API-window write subset would have an approximate boundary. SQL execution totals, observer polls and checkout samples must not be added/subtracted to derive per-operation server time.

## Comparison and selected default

Original **uninstrumented pre-pool** single-client medians (five runs): API-only 186.9 req/s, worker-only 36.8 work/s, combined API 165.8 req/s and combined work 11.1 work/s. The pooled, **diagnostic + observer** three-run medians are respectively 180.6 / 35.4 / 156.9 / 10.5 at max 1; 426.8 / 70.1 / 449.9 / 14.0 at max 4; 735.0 / 71.9 / 585.0 / 14.2 at max 8; and 787.9 / 72.2 / 567.9 / 14.0 at max 16. The max-1 shape is broadly consistent with the old client bottleneck but is **not** an exact controlled baseline (separate container, observer and instrumentation). The original temporary baseline `worker-persistence-diagnostic.patch` recorded worker finish mutex-wait p95 around 159–168 ms in worker-only runs and 154–191 ms combined, whereas this pooled patch recorded worker-only finish checkout p95 47–52 ms at 4 and 19–20 ms at 8, and combined 42–60 ms at 4 and 19–23 ms at 8. Those are unlike acquisition windows, **not** claimed direct wait reductions; the total write latency, rates and server SQL totals must each be interpreted in their own window. See the complete original five-run diagnostic above, not an inferred zero-wait baseline.

The evidence challenges treating four as the performance optimum: eight improves these in-process API rates substantially at API-only and combined load, while using up to 18 rather than 10 sessions (two observers included). Sixteen adds API-only throughput in two repeats but not consistent combined/worker throughput and uses up to 27 sessions; worker-only run 16/2 dipped to 60.9 work/s. **The operator chose four as the default per role to conserve deployment-wide PostgreSQL sessions despite the measured API gains at eight.** An operator can later explicitly set `BLKIT_API_DB_POOL_MAX=8` and/or `BLKIT_WORKER_DB_POOL_MAX=8` for active remote roles only after stress testing their deployment and budgeting the sum of role limits across processes plus observers/admin clients against PostgreSQL/proxy capacity. This choice trades demonstrated API throughput on this workload for a lower default connection budget; it is not evidence that four is optimal everywhere or that eight universally improves performance.
