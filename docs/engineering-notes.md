# Engineering notes

Build practice, measured constraints and design traps, each with its fix. These are the details most likely to be rediscovered painfully. Design reasoning is in [decisions.md](decisions.md); verification procedures are in [verification.md](verification.md).

## Build and toolchain

- The toolchain is pinned to an exact version in `rust-toolchain.toml`. The compiler is part of the engine identity, so a bump is a recorded decision and stored replay records may need re-recording.
- A full workspace release build takes about a minute cold and seconds warm. Worktrees can share artifacts with a common `CARGO_TARGET_DIR`.
- Run `cargo build --release` after pulling. A release binary built from older source keeps working, and its refusal of a newer experiment (`unknown field`) looks like a malformed experiment.
- Anything that records or reports a result takes the engine version from `dre engine` on the binary that did the reasoning. `dre --version` reports the CLI crate's own version, which does not track the engine. An engine hash that does not match the binary voids the determinism claim for that result.
- Run an experiment from another branch in a `git worktree`, never over the main checkout. Replay baselines live in the working tree, and `--record` from the wrong branch overwrites them.
- When syncing a working tree to a build machine, do not overwrite generated files (`experiments/records`, `Cargo.lock`). Sync source out, pull the generated files back, and run `cargo build --locked` before committing.
- Drive a test run from the exit code. `cargo test --workspace | grep "test result: ok"` hides failures: a failing target aborts the run, later crates never report, and the summed count drops instead of turning red.

```bash
if cargo test --workspace >/tmp/t.log 2>&1; then echo PASS; else echo FAIL; fi
```

## Determinism enforcement

- `.gitattributes` forces LF. `ModelHash` is taken over the raw bytes of a model pack, so a CRLF checkout is a different model, and every replay fails while every file looks identical in a diff.
- Banned types are enforced mechanically: `HashMap`, `HashSet`, `Instant` and `SystemTime` through `clippy.toml`; floats and panicking accessors through workspace lints. When a lint fights the code, the code is usually wrong.
- Test modules lift the panic lints with a scoped `#![allow(...)]`, since a panic in a test is the failure report.
- `unsafe_code` is `forbid`, which cannot be lifted locally. That rules out a counting global allocator, so peak memory is read from `/proc/self/status` (`VmHWM`) by the harness. Read `VmRSS` and `VmHWM` in one snapshot, or the process can grow between two reads and a current value can exceed the peak.
- `dre-lab` allows `clippy::disallowed_types` at its crate root. It is the harness and must time execution. The rule it keeps is that no reading reaches a decision.
- A stray carriage return inside a line makes git classify the file as `-text`, which line-ending checkers skip. `tools/check-line-endings.py` checks for it. When repairing one, refer to the byte as `chr(13)` from a script file, never through a shell escape.
- Parse every pack section through `dre_model::yaml::from_str`, never `serde_saphyr::from_str` directly. The wrapper configures the parser strictly: merge keys rejected, strict booleans, bounded alias expansion.
- `dre-model` holds pack sections as unparsed bytes. That breaks a dependency cycle and makes `ModelHash` cover exactly what was on disk.
- Keep the conformance suite in `crates/dre-conformance` as a workspace member. A test outside a package is not a Cargo target and would never run.

## Hashes and replay

- A green `stored_replay` after a semantic change is not evidence. Replay covers the packs that exist, and `ENGINE_VERSION` protects the ones that do not. Bump it on the reasoning change, never on whether the suite went red.
- A clean `ENGINE_VERSION` bump has a recognizable diff: every record moves only its `engine` and `engine_version` lines. If `decision` or `proof` also moves, the engine changed its answer and re-recording is not the fix. Compare against `git show HEAD:<path>`, not against an earlier copy.
- A proof-only change (new node kind, added provenance edges) shows as `ProofMismatch` with an unchanged decision hash. Confirm the decision hashes before re-recording.
- Correcting a pack so its ontology declares what its rules conclude changes `ModelHash` and leaves decision and proof hashes identical. `NOT COMPARABLE` on `[Model]` alone means a different question, not a different answer.
- The test for membership in `EngineHash` is whether a change moves any hash a replay verifies, not whether it changes the decision. A* and Dijkstra return the same plan on a grid, but A* expands fewer states and the proof records every rejected candidate, so `ProofHash` differs.
- Read the search strategy back from the run configuration at the call site. Naming it twice would let the hash describe a search the run did not perform.
- `DecisionHash` must cover the plan. The planner works on a clone of the state and never applies the plan, so hashing the state alone captured none of it, and a 5x5 and a 40x40 grid hashed identically.
- Emit input proof nodes from the built state (a `BTreeMap`), not the experiment file. File order would let a reordering of entities change `ProofHash`.
- `Trace` wraps `ProofGraph` instead of extending it. A provenance map inside the graph would empty on deserialize while `PartialEq` still called two graphs equal.

## Reasoning semantics

- Causal effects must `set`, never `observe`. The merge rule accumulates evidence about a world assumed to hold still, so two disagreeing observations of one property merge to `CONFLICTING`. An action changes the world; routing effects through `observe` makes every transition conflict with its predecessor and the planner stalls one step in.
- Conditions use three-valued logic, so `NOT unknown` is `unknown`. Mapping "not measured" onto true or false is a lie that propagates silently.
- Inferences apply immediately, so a rule earlier in firing order can enable a later one in the same round.
- `Fixed` and `Value` have hand-written serde. `temperature: 900` means nine hundred, and `{fixed: "0.5"}` and `{ref: pump-1}` cover the ambiguous cases. Floats are rejected with a message pointing at `{fixed: ...}`. A fixed literal inside a feature expression is a bare number or decimal string, because the position is already typed.
- Every feature produces fixed-point. A property a feature writes must be declared `type: fixed`, and rules reading it compare against `{fixed: ...}`. Unlike kinds compare as unknown, so a mistake here makes a rule silently never fire. `dre validate` checks both halves.
- A derivation has four outcomes: inferred, unknown (operand unavailable), invalid (operands present, operation undefined) and not applicable (a guard says so). All four leave the property without a value. `0/0` is invalid, not zero: a model computing zero would classify a stopped pump as catastrophically inefficient.
- Unresolved features are recorded dependencies-first, so a feature that could not derive points at the one beneath it that failed first.
- `Operator::UNKNOWN` on an absent subject is true by design. "Nothing ever concluded this" is a legitimate thing for a rule to test, and a shipped pack relies on it. The hazard is a misspelled entity name satisfying the operator, which `dre validate --experiment` reports.
- `specificity` orders rule firing; it does not override. Two rules that conclude different values merge to `CONFLICTING` unless the value domain declares a `precedence`.
- Validation checks declared domains. The run-time gate rejects a defective reading against the ontology and records a rejection naming source and reason. Nothing is repaired: no clamping, no substitution.
- Absent is not `UNKNOWN`. An expectation of `status: UNKNOWN` on a property no rule mentions fails as absent. Declare the conclusions unknown in the experiment's given state to assert that the engine declined to decide them.
- A propagating pack needs rules that conclude soundness as well as failure. "Every upstream is sound" is unknown, not true, when upstreams were never decided, so without clearing rules a root-cause rule never fires anywhere.
- Write a hard constraint as a disjunction, not a bare requirement. `rating_ok == true` is evaluated in every candidate state, so a missing reading about something the route never touches blocks the whole map; `any[ not(here), rated ]` is satisfied away from it.
- Reach has a mandatory `collapse`. An unresolved anchor is an error, never an empty set, because `every` over an empty set is true. `the` over several targets is refused. A hop naming an undeclared relationship reaches nothing, which `dre validate` reports.

## Search and planning

- Goal-test on pop, never on generation. Testing at generation returns the first route that touches the goal, which is optimal only when every action costs the same, and it stops A* from guiding.
- Dijkstra settles states on pop. Settling on generation returned valid, goal-reaching plans that were not the cheapest, which unit-cost actions cannot reveal.
- The frontier order ends in the state hash. Cost, then depth, then action id is not total, so without the hash a `BinaryHeap` resolves ties by heap layout.
- A heuristic needs states it can rank as worse. On an open grid with the goal in the far corner every cell lies on a shortest path, so `g + h` is constant and A* expands exactly the states Dijkstra does. Benchmark A* with the goal elsewhere, and assert equality there rather than `astar <= dijkstra`.
- A* refuses an inadmissible model (`check_admissible`) instead of returning a suboptimal plan.
- A ranking key that is not total makes a deterministic engine report nondeterministically, so rankings end on the subject name.
- `DecisionHash` is the wrong comparator for a counterfactual, since the state contains the observation the perturbation injected. Compare what the engine derived: the plan and properties with status `Inferred`.
- Rank survey entries by what the answer could buy as well as whether it changes anything, with best achievable plan cost as the second key. Report "this measurement cannot help" explicitly.
- A property the pack computes is not worth surveying. Feature subjects, rule inference subjects and geometry outputs are already declared, so they are excluded without a format change. Every new section that carries a subject must be added to that exclusion.
- An explanation is scored by mechanism, and a mechanism is an independence group, so correlated symptoms count at a discount. An exact tie writes nothing. Observations declared `needs_explaining: false` earn no coverage and incur no penalty but still contradict a hypothesis that predicted the opposite.

## Packs and experiments

- Run `dre validate <pack> --experiment <experiment>` after editing a pack. It catches faults that do not fail a run: a rule on a property nothing declares evaluates to unknown and never fires, and the run looks ordinary.
- Keep a pack beside its corrected version when the pack is evidence of a problem (`device-cascade` and `device-cascade-abduction`, `opc-ua-access` and `opc-ua-access-precedence`). Overwriting the old one leaves only the fix.
- Pair every experiment that shows a mechanism with one that shows the same pack reaching the other answer, so a ranking with the answer built in does not look identical to a working one.
- Two experiments that differ in exactly one property and return different decisions, with the same model hash and engine hash, demonstrate the thesis in four hashes.
- A soft constraint that can be routed around for free proves nothing. The test needs a route where avoiding the hazard costs more than crossing it.
- To verify a conclusion nobody can trace by hand, plant the answer instead of computing it. Generators place one fault in a large network and stop, and name dependency edges by a formula a reader can invert in one line. The count of resolved unknowns then acts as a checksum on the whole run.
- Type-named subjects resolve per instance in rules and features. Elsewhere a type naming exactly one instance resolves to it. A type-named subject that resolves to nothing reads `UNKNOWN` forever and the pack concludes nothing, so name the entity or check with `dre validate`.
- A capability is reachable only if a pack can invoke it. When a feature's only evidence is its own unit test, write a domain that uses it.

## The restart check

`tools/restart-conformance.sh` runs a replay of selected experiments across a reboot.

- It compares the kernel boot ID before and after, and reports `INCONCLUSIVE` rather than `PASS` if the machine did not restart.
- `verify` removes the boot hook as its first act, so a later failure cannot loop the machine.
- `arm` blocks until the kernel ends it. `systemctl reboot` returns when the request is queued, so a command chained after it runs while the machine is going down.
- Match a boot-hook marker by its whole tag, anchored. One tag was a prefix of another, so an unanchored `grep -v` removed both entries and the crontab silently got shorter. `selftest` installs and removes both entries in every order.
- Archive the report when the result exists, not when the next run starts. The stamp lives in the report (`archived_at`), so appending is idempotent.
- Do not hang a cleanup `trap ... RETURN` on a `local` variable. It is out of scope when the trap fires.
- A cycle that armed and never verified is recorded as `incomplete`.
- Schedule it only on a machine whose only job is to be rebooted.
