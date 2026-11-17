# Determinism rules

The engine's central claim is that the same input, model, and engine
produce the same decision — and the same reasoning, not merely the same
answer. Everything here exists to protect that claim.

The rules apply to **engine crates**: `dre-core`, `dre-state`,
`dre-evidence`, `dre-model`, `dre-symbolic`, `dre-geometry`, `dre-causal`,
`dre-planner`, `dre-proof`, `dre-replay`.

They do **not** apply to `dre-lab`, which is the harness around the engine.
The lab reads the clock, the filesystem, and the environment freely. The
distinction that matters is that nothing the lab observes is ever handed
back into a reasoning path — a timing that cannot be observed cannot change
an answer.

## Prohibitions

### No floating point

`f64` and `f32` are banned in engine paths, enforced by
`clippy::float_arithmetic`.

Not because of precision — because of reproducibility. Results vary with
FMA contraction, x87 excess precision on 32-bit targets, SIMD
vectorisation width, and the compiler's freedom to reassociate. Two
machines running identical source can disagree in the last bit. In a
system where a distance comparison selects an action, the last bit decides
which action fires.

Use `dre_core::fixed::Fixed`: `i64` scaled by 10⁶, with exact addition,
exact comparison, and truncation that is specified rather than incidental.

### No `HashMap` or `HashSet`

Banned via `clippy.toml`'s `disallowed-types`.

Their default hasher is seeded per process, so iteration order differs
between two runs of the same binary on the same input. Any engine step that
iterates a map — rule evaluation, candidate generation, proof emission —
would produce a different result or a different proof each run.

Use `dre_core::collections::DetMap` / `DetSet`, which are `BTreeMap` /
`BTreeSet`. Iteration follows key order.

### No wall clock

`Instant` and `SystemTime` are banned in engine paths.

Evidence freshness is real and necessary, so it is measured in **logical
ticks supplied with the input**. The same experiment run today and next
year must reach the same conclusion about whether an observation was stale.

### No random number generation

There is no RNG in the engine and no seeded RNG either. A seeded generator
is reproducible only if the seed is part of the input and the consumption
order is fixed, which is a constraint that is easy to state and hard to
keep. Nothing currently needs one.

### No environment or filesystem access

Engine crates do not read environment variables, configuration files, or
locale. All configuration arrives through `RuntimeConfig`, which is folded
into the `EngineHash`, so a configuration difference is visible as a
different engine rather than as an inexplicable disagreement.

### No mutable models during execution

A model pack is loaded, hashed, and then immutable. Self-modifying rules
are deferred (plan section 23).

### No nondeterministic thread result ordering

Parallelism is permitted only where results are recombined in an order
fixed by the data rather than by completion time. Nothing is parallel yet.

### No panics in engine paths

`unwrap`, `expect`, and indexing are denied. A panic mid-decision leaves no
proof and no replayable record, which defeats the point. Return an error —
an error is itself a deterministic outcome.

Test modules lift this with a scoped `#![allow(...)]`, because in a test a
panic *is* the failure report.

## Requirements

### Total orderings

Every ordering that affects a result must be total, terminating in a
unique identifier so no comparison falls through to an arbitrary
tie-break:

| Ordering | Key | Defined in |
|---|---|---|
| Rule firing | priority ASC, specificity DESC, rule_id ASC | `dre-symbolic` |
| Planner frontier | cost ASC, depth ASC, action_id ASC | `dre-planner` |
| Region ranking | distance ASC, region_id ASC | `dre-geometry` |

### Canonical encoding before hashing

Hashes are taken over canonical bytes, never over a serde representation.
JSON may reorder keys, vary whitespace, or render a number several ways;
any of those changes the digest without changing the meaning, and replay
would report a mismatch for two runs that actually agreed.

See `dre_core::canonical`.

### `Cargo.lock` is committed

Do not gitignore it. Dependency versions are part of the engine, and
nothing else pins them: `EngineHash` covers the engine version and runtime
config, not the resolved dependency graph. Without the lockfile, two
checkouts of the same commit can build against different versions of a
transitive dependency and — if any of them touches a decision path —
disagree, while `git log` shows nothing changed.

The usual argument for omitting a lockfile applies to libraries meant for
downstream consumption. Nothing here is published.

### Normalised line endings

`.gitattributes` forces LF everywhere, in the repository and in the
working tree.

This is a determinism rule, not a style one. `ModelHash` is computed over
the raw bytes of model-pack files, so a CRLF checkout and an LF checkout of
the same commit are different models. A Windows developer with
`core.autocrlf=true` and a Linux CI runner would compute different model
hashes from identical source, and every cross-machine replay would fail
while every file looked the same in a diff.

### Explicit stable tags

Enum variants are encoded with hand-written string tags rather than
derived names, so renaming a variant cannot silently invalidate stored
hashes.

### Bounded search

Every search is bounded (plan section 22). Exceeding a bound is an error,
never a truncated answer: a planner that quietly stopped early returns a
valid-looking plan that is not the plan the model implies, and nothing
downstream can tell the difference.

Bounds only count if something reads them. Four of the eight were parsed,
documented, folded into `EngineHash` and enforced nowhere until
2026-08-15, so the hash asserted a difference between two engines that
would decide identically. A ceiling nothing checks is a comment.

### Changing reasoning behaviour moves `ENGINE_VERSION`

`EngineHash` covers the engine version and the runtime config, and nothing
else. Tracking the engine version separately from the crate version is
what lets a release that changes no reasoning behaviour leave stored
records valid — and the converse is an obligation, not a convenience.

If a change alters what the engine **accepts**, what it **decides**, or
**any other hash a replay verifies**, move `ENGINE_VERSION` and re-record
every baseline. Otherwise the hash claims two engines are the same when
they are not, and records produced under the old behaviour verify against
the new one.

Each of the three is a half that gets missed, and they were learned in that
order.

"Accepts" is the first. An engine that refuses a pack its predecessor
loaded is a different engine, even though no decision changed — the two
answer the same input differently. Both additive capabilities and new
strictness qualify.

The third is the one the rule originally missed, and it is worth stating
plainly because the phrasing invited the mistake: **a replay verifies five
hashes, not one.** A knob that leaves the decision untouched can still move
the `ProofHash`, and that is enough. `Strategy` is the case. A\* and
Dijkstra return the same optimal plan — measured on the 40-by-40 pack, not
assumed — but A\* reaches it having expanded 383 states where Dijkstra
expands 727, so the two record different sets of rejected candidates.

Had the strategy stayed outside the engine identity, those two runs would
have agreed on engine, model, input and decision, and differed on proof:
four hashes equal, one not, with nothing to explain it. That is
indistinguishable at a glance from non-determinism, which is exactly the
confusion the five-hash scheme exists to prevent. The test that had been
guarding this asked only whether the two *decisions* agreed, and would have
gone on passing.

| Change | Which half | Why it moved the version |
|---|---|---|
| Conditions read by explicit dispatch | accepts | a malformed comparison is now an error, not a status test |
| `on_undecidable` on hard constraints | decides | the same pack can now plan differently |
| Geometry writes `boundary_distance` / containment | accepts | an older engine would silently ignore a pack that asks |
| Every hand-written structure denies unknown fields | accepts | a pack that loaded may now refuse to |
| Planner strategy is declarable configuration | proof | same decision, different rejected candidates |
| A geometric space names an entity type | both | a pack with two instances now yields two classifications; one whose subject is not a type now yields none |

All six landed on 2026-08-15, taking the engine from `0.1.0` to `0.6.0`.
`stored_replay.rs` is what enforces this in practice: it compares every
experiment against its committed record and reports the engine as
differing, so forgetting the bump fails the suite rather than going
unnoticed.

The `0.5.0` re-record is worth keeping as the shape a clean bump has. All
twelve existing records changed exactly two fields, `engine` and
`engine_version`; not one `decision` or `proof` line moved. A thirteenth
field moved on `grid-40-001` alone — its `model`, because its `goals.yaml`
gained the measured result of the question that file had been posing. Any
other line moving in a re-record diff is the engine having changed its
mind, and re-recording is then not the fix.

`0.6.0` is the case that shows why the rule cannot be "bump when a record
moves". The geometry change moved **nothing**: both geometric packs named
their one entity the same thing as its type, so old lookup and new lookup
returned the same entity, and all thirteen existing records verified
against the changed engine without complaint. The behaviour had changed
anyway, in both halves — a pack with two instances of the type now
produces two classifications, and a pack whose subject is not a type now
produces none.

A green `stored_replay` says the engine still answers *these* packs the
same way. It is not evidence about packs nobody has written yet, and those
are what the version protects.

## Conformance suite

Plan section 21 requires tests written specifically to *break* determinism,
not merely to confirm it.

Lives in `crates/dre-conformance` and runs as part of
`cargo test --workspace`.

- [x] randomised input ordering — entity, property and term order all vary
      per iteration
- [x] repeated execution — 100,000 runs, 1 unique output hash
- [x] randomised rule file ordering — 500 permutations of the diagnosis
      pack, one state hash and one proof hash
- [x] varying CPU counts — thread-count invariance across 1, 2, 3, 8 and
      16 workers, plus a source scan proving no engine crate can ask how
      many CPUs exist. Not the same as fewer physical cores; `taskset -c 0`
      stays a manual check
- [x] parallel execution — 64 concurrent runs with experiments
      interleaved. Worth doing precisely *because* nothing in the engine
      is parallel: single-threaded internals say nothing about several
      runs existing at once, and a shared cache or a thread-local memo
      would turn the invariant into "same decision if nothing else was
      running"
- [x] process restart — a fresh process against a committed replay record
- [x] machine restart — cannot be a `cargo test`, because a test cannot
      reboot the host it runs on. Automated by
      `tools/restart-conformance.sh`, which can run on a schedule on a
      machine whose only job is to be rebooted. `history` carries the pass rate;
      a cycle that armed and never verified is recorded rather than left
      as a gap
- [x] cross-machine — every committed experiment reproduces on a second
      host the records committed from the first. Read the caveat in
      `decisions.md` before quoting it: both hosts were the same silicon,
      so this rules out per-boot and per-host state and tests no
      CPU-feature divergence at all
- [x] serialization round-trip — including that `UNOBSERVABLE` and
      `UNKNOWN` survive distinctly rather than both flattening to absent
- [x] checkout location — the same pack loaded from two different
      absolute paths must produce all five hashes identically. Not on plan
      section 21's list; added when external packs began arriving on
      branches checked out beside the main tree, which is the arrangement
      in which a path reaching a hash would first start to matter
- [x] ~~identical workloads across multiple machines~~ — **out of scope**

Every attack has a companion test proving it is not vacuous: that the
shuffle really reorders, that the scan really matches, that the round trip
really loses something if the encoding is careless.

Ordering-independence is also covered at unit level in `dre-core`
(canonical encoding), `dre-symbolic` (rule ordering), and `dre-planner`
(frontier ordering).

### Running it

```bash
cargo test -p dre-conformance
```

The acceptance test takes about 8 seconds in debug. To run it harder:

```bash
DRE_CONFORMANCE_ITERATIONS=1000000 cargo test -p dre-conformance --release
```

Values below the plan's floor of 100,000 are rejected rather than quietly
accepted — a "passing" acceptance test that ran 100 iterations would be
worse than no test at all.

For the cross-process check, run the workload binary more than once and
compare. Any seed must give the same hash as any other:

```bash
cargo run -p dre-conformance --bin workload-hash -- 0
```

### What the acceptance test actually proves

Less than it appears, and it is worth being precise about the gap.

It proves that within one process, on one machine, one binary, the same
problem constructed 100,000 different ways produces one hash. That rules
out insertion-order leakage — the `HashMap` class of bug.

On its own it does **not** rule out anything that varies between
processes: address-space layout reaching a hash, a lazily-initialised
static, or environment-dependent behaviour. Those are covered elsewhere —
the committed replay records in `experiments/records/` are compared by a
fresh process each run, and `parallel_and_cpu.rs` attacks the shared-state
cases from several threads at once. Taken alone, though, this test means
"order-independent", which is a strictly weaker claim than the heading
suggests.

### Scope limit: single machine only

Cross-machine determinism is deliberately **not** tested. Plan section 21
lists it; this project drops it.

The consequence should be stated plainly rather than left implicit. The
verified claim is:

```text
SameInput + SameModel + SameEngine + SameMachine = SameDecision
```

The prohibitions above are still written to make the stronger,
machine-independent property *likely* — banning floating point and
normalising line endings are precisely the measures that would let results
survive a change of host. But likely is not verified. A determinism bug
that only manifests on different microarchitecture would not be caught, and
the most plausible candidates are exactly the ones the fixed-point rule
exists to prevent: any `f64` that slips into an engine path, or any
dependency that uses one internally.

If the engine is ever run somewhere other than the development VM, this is
the first assumption to revisit.

## Adding a dependency

Every dependency is a determinism risk surface. Before adding one, check
that it does not internally use `HashMap` iteration in a way that reaches
output, does not read the clock, does not use floating point on a path that
reaches a decision, and does not parallelise result assembly.

Record the check in `decisions.md`.
