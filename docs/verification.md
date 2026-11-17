# Verification

How to check that DRE actually works, including the checks `cargo test`
cannot perform for itself.

The distinction this document exists for: most of the suite proves the
engine agrees with itself, and that is the weaker half. What matters is
whether it still agrees with what it decided before — before a refactor,
before a process restart, before a reboot.

---

## The routine check

```bash
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check
```

The toolchain is pinned in `rust-toolchain.toml`; run these on a Linux host with that toolchain.

### Never summarise a test run by grepping for `ok`

```bash
# WRONG. This reports a smaller green number instead of a red one.
cargo test --workspace | grep "test result: ok"
```

A failing target aborts the run, so the crates after it never report and
the summed pass count silently **drops**. The output still says `ok` on
every line that printed. This produced a confident "85 passed, 0 failed"
report while the suite was red.

Drive it from the exit code:

```bash
if cargo test --workspace >/tmp/t.log 2>&1; then echo PASS; else echo FAIL; fi
```

### And never check clippy without `-D warnings`

```bash
# WRONG. Exits 0 with warnings on the screen.
cargo clippy --workspace --all-targets >/dev/null 2>&1 && echo CLEAN
```

The same trap one step along, and it caught this repository on 2026-08-15:
an exit-code check reported CLIPPY_CLEAN across several commits while
`depth.rs` carried two lint warnings the whole time. Warnings are not
failures unless you say so, and the whole point of the lint set here is
that they are.

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Both traps are the same shape: a command that reports success by a weaker
standard than the one you meant. The test version drops failures from a
sum; this one drops warnings from a verdict.

### What a green run means

A green run is `cargo test --workspace` passing, `cargo build --workspace` with no warnings, and clippy and `cargo fmt --check` clean. Every committed experiment has a stored record that `dre replay` reproduces. The test count printed by `cargo test --workspace` is the authoritative one; a count written in prose goes stale.

### Line endings, which a green suite would not catch either

`ModelHash` is computed over the raw bytes of a model pack, deliberately, so
that a parser change cannot retroactively alter a stored hash. The consequence
is that the **CRLF and LF versions of a pack are different models**.

`.gitattributes` pins `* text=auto eol=lf` and every tracked file is `i/lf
w/lf`. That handles checkout and `git add`. It does not cover a Windows tool
rewriting a file **in place**: the index stays LF, the working tree drifts, and
`git status` is silent because the filter reconciles them on the way past.

That gap matters because the working tree can leave git: a build or experiment run from a tree copied to another machine (for example with `tar` over `ssh`) uses the working tree, not a commit. A drifted pack would be hashed as a different model on the build machine and surface as a replay mismatch with no obvious cause.

```bash
python tools/check-line-endings.py           # all tracked files; exit 1 on CRLF
python tools/check-line-endings.py --staged  # what the pre-commit hook runs
python tools/check-line-endings.py --fix     # rewrite drift back to LF
```

It runs in CI before anything compiles, and as a blocking pre-commit hook once
installed:

```bash
git config core.hooksPath tools/githooks
```

**A stray CR is checked separately, and it is the case every endings check
misses.** A carriage return *inside* a line is not a line-ending problem, so
`git ls-files --eol` does not report `crlf` or `mixed` -- it reclassifies the
file as `-text`, which is one of the tokens meaning "leave my endings alone",
so the check skips it and reports all-LF. True, and useless.

That is not hypothetical: a shell escape `$'\r'` written as a real CR byte,
then round-tripped through a Python read without `newline=''`, silently became
`$'\n'` -- universal newlines translate a lone CR. The strip it performed
stopped stripping, and a header kept its carriage return. Files with NUL bytes are still treated as genuinely
binary and skipped.

**The measurement trap**: on Windows,
`grep -c $'
'` and `git show | ...` both **add** CR to the stream, so
pure-LF files get reported as CRLF. `git ls-files --eol` is the authoritative
diagnostic and stays authoritative through a pipe, because `i/` and `w/` are
git's own verdict on the real bytes. The checker drives off that and never
shells out to grep; anything touching content opens `'rb'`.

### Claims worth checking that a green suite does not make for you

Each of these was true of this repository at some point today and none of
them made a test fail. If you are auditing it, these are the questions
that found things:

**Does every experiment pass its own expectations?**

```bash
for e in experiments/*.yaml; do cargo run --quiet --bin dre -- run "$e" >/dev/null || echo "FAIL $e"; done
```

`dre run` exits non-zero when an expectation fails. Until 2026-08-15 only
three experiments of twelve had their expectations asserted by a test, and
one of the other nine had been failing since the day it landed.

**Is anything declared but never read?**

```bash
grep -rn "limits\.max_" crates/ --include=*.rs | grep -v config.rs
```

Four of the eight resource ceilings had no reader while sitting inside
`EngineHash`. The general form of the question — what does this codebase
declare, hash, or document that nothing consumes — has now found six
separate defects, which is why it is written down rather than remembered.

**Can a pack actually invoke what the docs say it can?**

```bash
grep -rn "boundary_distance(" crates/ --include=*.rs
```

Three of the five geometry operators plan section 16 requires had zero
call sites anywhere, tests included. Implemented is not the same as
reachable.

**Does a misspelled key fail?**

```bash
cargo test -p dre-lab --test unknown_keys
```

Serde skips unrecognised fields by default, so `perconditions` gave an
action applicable everywhere and `plan_lenght` gave an experiment that
asserted nothing. Every hand-written pack structure now denies unknown
fields; this is what proves it.

**Does every engine knob reach the `EngineHash` — and is the decision the
only hash you checked it against?**

```bash
cargo run --bin dre -- run experiments/grid-40-mid-001.yaml | grep -E "hash|States"
sed 's/^  strategy: astar$/  strategy: dijkstra/' experiments/grid-40-mid-001.yaml > /tmp/d.yaml
cargo run --bin dre -- run /tmp/d.yaml | grep -E "hash|States"
```

Two runs, same pack, same input. The decision hash is identical and the
proof hash is not, because A\* expanded 383 states where Dijkstra expanded
727 and the proof records what each rejected. The engine hashes differ,
which is what makes that legible.

The check has two halves and the second is the one that gets skipped. A
field can be *in* `RuntimeConfig` and absent from its `write_canonical`,
in which case the hash silently ignores it — the same declared-but-unread
shape as the ceilings above, and `config.rs` now has a per-field test for
each of the three. The subtler half is deciding a knob does not belong in
the hash at all because it does not change the answer. `Strategy` does not
change the answer. It changes a hash that replay verifies, and that is the
test.

---

## Experiment-level checks

```bash
cargo run --bin dre -- run experiments/diagnosis-002.yaml --why
```

`--why` prints only the steps the final conclusion rested on, following
the proof graph's dependency edges backwards. `--proof` prints the whole
trace. On a run of a few hundred nodes the difference is the point: the
full trace is a record, the chain is an answer.

```bash
cargo run --bin dre -- validate model-packs/diagnosis --experiment experiments/diagnosis-001.yaml
```

Checks the pack against itself and against the experiment binding it:
every subject resolves to a declared property, geometry axes exist, region
coordinates name real axes, and declared values match their declared kinds
and domains.

Worth running on any pack you edit, because the faults it catches are the
ones that do not fail. A rule on a property nothing declares evaluates to
unknown, quietly never fires, and the run reports nothing unusual.

```bash
cargo run --bin dre -- counterfactual experiments/haulage-001.yaml \
  --set north_bridge.rating_ok=true --full
```

Asks what the engine would have decided had one property been different.
Two ordinary runs and a diff — nothing here reaches a conclusion `dre run`
could not, which is why it needs no separate determinism story.

The check worth running on it is that it agrees with the experiment it
describes. `haulage-001` with the bridge rated *is* `haulage-002`, so the
decision hash it reports must equal the one committed in
`experiments/records/haulage-002.json`. It does, byte for byte, and
`tests/counterfactual.rs` asserts it.

`--unset <subject>` asks the reverse — what would it have decided without a
reading it has. That direction is the one that matters for deciding what is
worth measuring.

Three answers mean different things and the tool keeps them apart.
"CONCLUSION UNCHANGED" is a finding about the engine. "The input did not
change" means the property already held that value, so nothing was asked —
a typo would otherwise report as a real negative result. And the decision
*hash* moving is neither: it covers the state, and the state contains the
observation just injected, so it moves whenever the input does.

That last one is why the headline reads CONCLUSION rather than DECISION.
Reporting the hash made every counterfactual look consequential — including
`--set north_bridge.rating_ok=false`, which returns the identical route at
the identical cost, because a blocked bridge and an unrated one are the
same bridge to a planner.

```bash
cargo run --bin dre -- survey experiments/haulage-001.yaml
```

Asks the question a counterfactual raises but cannot answer: of everything
still unknown, which is worth going out to measure?

```text
  north_bridge.rating_ok
    can only help — as good as 2.000000, no worse than 7.000000
  ford.rating_ok
    can only hurt — no better than 7.000000, as bad as 9.000000
```

Candidates are every property the entity's **type** declares that the run did
not arrive at a value for — not just the ones the experiment happened to list
as unknown. A pack whose facts all arrive as evidence declares no properties
at all, and reading only the experiment reported "there is no observation
left to take" on a case with twenty-six measurable properties.

Candidate values come from the ontology, which is the only place that says
what a property may hold — so a closed `values:` vocabulary is what makes a
symbol surveyable at all.

Numerics are probed at the thresholds the pack declares — each operand and its
two neighbours, since `GT` and `GTE` differ exactly there. Sampling the bounds
would miss every flip between them, which is why numerics were once declined
outright; the thresholds are where the middle actually is. A numeric compared
against nothing is declined **by name** with the reason, rather than skipped.

Where a pack thresholds a value it *derives* from a measurement — subtracting
a baseline and comparing the difference, which is the common IT shape — the
threshold is carried back across the arithmetic, but only through `add`,
`subtract` and `negate`. Those are exact on the raw `i64`. `multiply` and
`divide` truncate toward zero, `min` and `max` are many-to-one, and `abs`
names two readings for one threshold; each is declined by name, saying which
operation blocked it. Every carried probe is then confirmed by running it and
re-evaluating the threshold against the resulting state, so a bug in the
inversion yields a false decline rather than a false probe.

Properties the pack derives are not offered, because asking what the engine
would conclude if told its own conclusion is not a question. That is read
from the pack's own feature subjects, inference subjects, geometry outputs
and the explanation layer's `rank_into` — no format change, the information
was already declared. Without it `fault-network` reports 240 entries; with
it, one line.

That list grew by one the hard way, and the way it grew is the point.
`rank_into` was added, the survey went on reading the four kinds of derived
subject it already knew about, and
`dre survey experiments/device-cascade-abduction-004.yaml` duly offered
`boiler-1.root_cause` as something to go and measure. Nothing failed: the
survey ran, sorted, and produced a confident entry that was nonsense. Every
new section carrying a subject has to be added here, and there is nothing in
the type system that says so.

Two distinct filters are at work and it is worth not confusing them, as
this document briefly did. **Observability** — does the engine have a
defined way to obtain or inspect this fact at all — is already answered by
the epistemic status, `UNKNOWN` meaning observable-in-principle and
`UNOBSERVABLE` meaning nothing will ever resolve it. That is why
`diagnosis-001` offers nothing. **Route** — derived, acquired, or actuated —
is the second filter, and the only one this tool has to reconstruct. A
derived property is perfectly observable and still not worth a trip.

```bash
cargo run --bin dre -- explain experiments/device-cascade-abduction-001.yaml
```

Ranks the pack's declared hypotheses against the observed state, and reports
every one of them with the components of its score rather than only the
winner:

```text
  1. POWER  2.400000
     coverage 2.750000, 0 contradicted, 1 unexplained, 2 mechanisms, 0 assumed
       rail_collapse — supported (1 of 1 observed)
       bus_degradation — supported (4 of 4 observed)
       unexplained: overheating
  2. THERMAL  0.900000
     ...
Best explanation: POWER (by 1.500000)
```

The number to read is **2.75, not 5.00**. `POWER` accounts for five
observations, four of which hang off one mechanism — so they are one thing
reported four times and score `1 + 3 × 0.25`. A symptom count would give the
same winner for the wrong reason, which is invisible until the case where it
gives the wrong answer.

The comparison that makes it mean something is with the pack next door.
`device-cascade-002` and `device-cascade-abduction-001` describe the same
device with the same readings; the first merges two correct rules into
`CONFLICTING` and the second returns `POWER`. Both packs are kept, because
the first is the evidence that rules cannot rank and deleting it would leave
only the fix.

`device-cascade-abduction-003` is the one that stops the others proving
nothing: a healthy rail on a hot device returns `THERMAL` from the same
pack. Without it, a ranking with the answer written into it would look
identical.

```bash
cargo run --bin dre -- survey experiments/device-cascade-abduction-004.yaml
```

With an explanation layer declared, the survey answers a sharper question
than "what could change the decision" — *which measurement would settle the
diagnosis*:

```text
  boiler-1.flow_sensor_error — could overturn the diagnosis (spread 2.500000)
    true -> POWER in front, base leader at 1.000000
    false -> THERMAL in front, base leader at -1.500000
  boiler-1.door_open — separates nothing; every answer leaves the accounts where they are
```

The door is the line worth having. It is an ordinary unknown whose answer
changes the state, so the older ranking has nothing to say against it. Only
a layer holding two accounts at once can report that measuring it cannot
settle which is right.

The standing is held to the hypothesis leading **as written**, not to
whoever leads under each candidate value. Reading the current leader would
change subject exactly when the answer got interesting, and a flip would
report as no movement at all.

```bash
cargo run --bin dre -- replay experiments/diagnosis-002.yaml
```

Compares a fresh run against the committed baseline in
`experiments/records/`. This is the comparison that catches drift.
`--self-only` compares two fresh runs instead, which proves only that the
engine is consistent with itself right now.

```bash
cargo run --release --bin dre -- benchmark experiments/diagnosis-002.yaml --iterations 10000
```

Reports the plan section 20 metrics and a determinism verdict over the
whole loop, and appends a row to the archive under
`experiments/benchmarks/` (gitignored; pass `--no-archive` to skip).

Every experiment: one unique decision hash, one unique proof hash,
Determinism PASS over 10,000 iterations.

```bash
cargo run --bin dre -- history experiments/diagnosis-002.yaml
```

Reads the archive back. Drift against the previous comparable row is the
obvious part; the useful part is the agreement check. Each benchmark row is
internally deterministic — one decision hash over its own loop — so an
answer that *moved between* runs is invisible to any single run and only a
history can see it.

Rows from another build profile, model or engine are listed as set aside
rather than dropped quietly. A debug row among release rows differs by an
order of magnitude and would otherwise read as a catastrophic regression
that never happened.

### Reading the peak-memory figure

Peak RSS never falls, so the number that means something is the growth
across the loop, not the absolute value. Vary the iteration count and
compare — a leak scales with iterations and arena warm-up does not:

```bash
for n in 100 1000 10000 50000; do ./target/release/dre benchmark experiments/diagnosis-002.yaml --iterations $n | grep "Peak memory"; done
```

Measured 2026-08-15: 460, 536, 596 and 600 KiB of growth for 100 through
50,000 iterations. Five hundred times the work for the same half-megabyte,
so the growth is the allocator reaching a steady state, not accumulation.
A leak would have shown roughly 500× more at the last step.

### Checking a traversal

```bash
cargo run --bin dre -- run experiments/unit-reach-001-partial.yaml --proof
```

A traversal records where it went, and that is the thing to read:

```text
#5 REACH  web ->mounts .available (every of srv-cache, srv-data, srv-logs)  <- #0 #1 #2
#6 RULE   R001 (did not fire)
#7 REACH  web ->mounts .available (some of srv-cache, srv-data, srv-logs)   <- #0 #1 #2
#8 RULE   R002 (fired)
```

Two things to check. The **targets are in canonical order**, not the order
the experiment declared its edges — `srv-cache` before `srv-data` although
the experiment lists `srv-data` first. If that ever stops being true, how a
question was written has begun to reach the answer. And the **inputs** are
the properties actually read, so a conclusion drawn across an edge traces
back to the measurements it rests on.

```bash
cargo run --bin dre -- validate model-packs/unit-dependency-reach
```

Catches a traversal that would go nowhere: a hop naming an undeclared
relationship, a path whose consecutive hops do not connect, or a property the
arrival type does not declare. Worth running because the runtime failure is
quiet — a reach that reaches nothing is *vacuously true* under `every`, so
the rule fires and the pack concludes something confident from a traversal
that went nowhere.

An unresolved **anchor** is the one case that is an error rather than a
finding, for exactly that reason.

### Reviewing with an independent model

A model reviewing the repository must be checked for having read something: a review that could read nothing still answers, fluently and from the prompt alone, which looks exactly like a fast review. Count the command executions in its event log before accepting any finding.

### When a model hash legitimately changes

`NOT COMPARABLE` naming only `[Model]` means the pack changed. That is not
a failure — the runs describe different problems, so nothing is proven
either way — but it is worth reading before re-recording.

The instructive case: correcting `diagnosis/ontology.yaml` to declare the
properties its rules conclude changed the `ModelHash` and left the decision
and proof hashes byte-identical. The model became a different model without
the engine changing its mind. If a decision hash moves at the same time,
those are two changes wearing one diff.

### When a replay legitimately fails

A `ProofMismatch` with an unchanged `decision` means the engine reasons
differently but concludes the same thing — a proof-format change, a new
node kind, added provenance edges. That is a real event and must not be
waved through, but it is not a wrong answer. Confirm the decision hashes
are identical, then re-record:

```bash
cargo run --bin dre -- run experiments/<name>.yaml --record --note "<why the proof changed>"
```

The diff should be exactly the `proof` line and the `note` line. If
`decision` also moved, the engine changed its mind and the note is not
the fix.

---

## Running an externally authored branch

External packs arrive on a branch. **Do not check it out over the main
working tree** — the baselines under `experiments/records/` live there, and
a `--record` run from the wrong branch writes them silently. Use a
worktree:

```bash
git worktree add ~/DRE-ext experiment/their-branch
cd ~/DRE-ext && cargo run --release --bin dre -- validate model-packs/their-pack
```

Main stays on `main` and untouched, the branch gets its own directory, and
both can be run without switching. Remove it when finished:

```bash
git worktree remove ~/DRE-ext
```

Each worktree gets its own `target/`, so the first build in one is a full
build. Sharing `CARGO_TARGET_DIR` between them avoids that and is safe as
long as builds are sequential.

This arrangement is also why `checkout_location.rs` exists. Two checkouts
at two paths is exactly the situation in which a location dependency would
start mattering, so the suite now asserts that all five hashes are
independent of where the files sit — a pack loaded from `/home/…/DRE` and
the same pack loaded from `/tmp/…` are the same model, and a `ModelHash`
that disagreed would make every replay record valid only in the directory
that produced it.

---

## Checks `cargo test` cannot make

Three items on plan section 21's list cannot be covered from inside a test
process. They are listed here as procedures rather than quietly dropped.

### Reduced CPU count

The suite covers this from two sides — running the same work across 1 to
16 threads, and a source scan proving no engine crate can even ask how
many CPUs exist (`crates/dre-conformance/tests/parallel_and_cpu.rs`).
Neither is the same as actually having fewer cores. To check that
directly:

```bash
taskset -c 0 cargo test --workspace
```

Single core, so any code path that behaved differently under real
parallelism has nowhere to hide. Expect the same 365 passes, slower.

### Machine restart

`cargo test` cannot reboot its own host, so this is a harness rather than a
test:

```bash
./tools/restart-conformance.sh arm --reboot
```

It records each experiment's hashes, installs a one-shot `@reboot` hook,
and reboots. The hook runs the comparison on the way back up. Afterwards:

```bash
./tools/restart-conformance.sh status
```

Nothing about the engine is expected to survive in memory across the
reboot, which is exactly the point: a result that depended on a warm cache,
a tmpfs file, or an already-mapped page would differ here and nowhere else.

**The harness proves the machine actually restarted.** "Run, reboot, run
again" is not self-evidently a restart test — if the reboot silently
failed, the second run would pass exactly as it does when it succeeds, and
the whole exercise would be a slower way of running the suite twice. The
baseline therefore records the kernel boot ID, which is regenerated on
every boot and cannot be forged from userspace, and the verify stage
reports `INCONCLUSIVE` rather than `PASS` if it has not changed. It also
refuses if `HEAD` moved in between, since that would be comparing different
code.

To do it by hand instead, the two-stage shape is: capture
`dre replay` output for every experiment, `sudo reboot`, capture again,
compare. The harness exists because that is easy to do wrongly.

### It runs itself now

The harness can run on a schedule on a machine whose only job is to be rebooted. Its history reads:

```bash
./tools/restart-conformance.sh history
```

```text
2026-08-15T23:19:09Z cycle=PASS        commit=7875614 ...
2026-08-15T23:20:53Z cycle=PASS        commit=7875614 ...
2026-08-15T23:25:13Z cycle=incomplete  commit=7875614  note=armed-but-never-verified
2026-08-15T23:29:36Z cycle=PASS        commit=c841242 ...

3 of 4 cycles passed
```

Read the pass *rate*, not the last line. A single green cycle says the
machine restarted cleanly once; the claim is about every time. The
`incomplete` row above is real — a cycle armed and never came back to
verify, because a chained `disarm` removed the boot hook while the machine
was still shutting down. It is recorded rather than absent, which is the
difference between a check that failed and one you cannot tell ran.

### Cross-machine

A second host makes this checkable for the first time, and it needs no new
tooling: the committed records were produced on one host, so replaying
them anywhere else *is* the comparison.

```bash
for e in experiments/*.yaml; do cargo run -q --bin dre -- replay "$e"; done
```

Every committed experiment passes on the second host. Re-run it rather than
trusting a count in prose, which goes stale the moment an experiment is added.

**Do not quote that as "determinism is machine-independent".** Both hosts
were the same CPU model, running the same OS build and the same rustc. It rules out anything seeded from the machine — hostname, MAC,
per-boot address layout, iteration order derived from an allocator address
— and tests no CPU-feature divergence, because none is available to test.
The `SameMachine` term is unfalsified across two boxes, not retired.

**Check placement before you quote it at all.** The result assumes the two
machines are different physical hosts. If both guests share a physical
host this command keeps printing PASS while proving strictly less, and there
is no in-guest signal for it. Confirm placement in the hypervisor's
management interface; otherwise the run above is a same-host test with a
cross-machine label on it.

An unattended run across a full reboot produced:

```text
verdict     PASS
boot_before b920e616-a475-4b03-9618-011012f5e18b
boot_after  f50b44b9-80ea-4991-9b7d-e29f1c7a23e2
experiments 6
```

All twelve hashes byte-identical. Corroborated in the journal: boot at
18:08:35, cron ran the `@reboot` job at 18:08:41, verify finished at
18:08:42, and the hook removed itself.

The guard was checked too, by arming and verifying *without* rebooting.
Every hash matched and the harness still refused to report `PASS`,
returning `INCONCLUSIVE` with "the boot ID is unchanged, so the machine
did not restart". A restart test that passes when no restart happened is
worse than no test.

| Experiment | Decision | Proof |
|---|---|---|
| logic-001 | `e700a372…` | `37e4ff65…` |
| diagnosis-001 | `851a12d5…` | `72ae0120…` |
| diagnosis-002 | `851a12d5…` | `3f43bba1…` |
| navigation-001 | `75e6a72f…` | `9d1afca1…` |
| process-001 | `ab6b63a4…` | `e0b086bc…` |
| process-002 | `9873d26e…` | `ce6ff357…` |

`diagnosis-001` and `diagnosis-002` share a decision hash on purpose: they
reach the same conclusion about the same machine, one from direct
observation and one from contested evidence that had to be resolved. The
proof hashes differ because the reasoning did.

### A different machine

Cross-machine determinism is in scope and tested. See "Cross-machine" above: every committed experiment reproduces its stored decision and proof on the second host, and the remaining caveat is that both hosts were the same CPU model, so the run rules out per-boot and per-host state and tests nothing about CPU divergence.
