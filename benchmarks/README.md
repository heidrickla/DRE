# Benchmarks

Benchmark definitions and results, per plan section 20.

| Metric | Purpose | Where it comes from |
|---|---|---|
| Correctness | Did it produce the expected answer? | Declared expectations |
| Determinism | Were all repeated results identical? | Unique hash counts |
| Reasoning steps | How much inference occurred? | `Counters` |
| Rules evaluated | Symbolic workload | `Counters` |
| States expanded | Planner workload | `Counters` |
| Peak memory | Resource use | `VmHWM`, read by the harness |
| Execution time | Runtime performance | Harness clock |
| Proof size | Explainability overhead | `ProofGraph::len` |
| Active dimensions | Geometric complexity | `Counters` |
| Unknown resolution rate | Evidence handling | `Counters` |

All ten are reported by `dre run` and `dre benchmark`.

Execution time is measured by the lab harness, outside the reasoning path.
It is reported, never fed back — see `docs/determinism.md`.

Determinism is measured by repetition: the MVP target is 10,000 runs
yielding exactly one `DecisionHash` and one `ProofHash`, with phase 1's
acceptance test at 100,000. All six experiments meet it.

## Reading the peak-memory figure

Peak RSS never falls, so the absolute number is mostly the binary. What
means something is the growth across the loop, and the way to tell a leak
from allocator warm-up is to vary the iteration count: a leak scales with
it and warm-up does not. See `docs/verification.md`.

## Results are not kept here

`dre benchmark` appends a row to `experiments/benchmarks/<experiment>.jsonl`,
which is **gitignored**, and `dre history` reads it back.

Deliberately not committed, unlike the replay records under
`experiments/records/`. A decision hash is a claim anyone can check; a
timing is a property of one machine on one afternoon, and committing one
would lend it an authority it has not got. Two rows from different hosts
are not comparable, and nothing in the file could tell you so.

The archive's real payload is not the timings. Each benchmark row is
internally deterministic — one decision hash over its own loop — so an
answer that *moved between* runs is invisible to any single run. Only the
history can see it, and that is what `dre history` reports on.
