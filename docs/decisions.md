# Decisions

Design decisions and their reasoning, plus the ones still open. Newest
first.

## Open

### Constraint engine placement — provisional

The architecture diagram (plan section 4) shows the constraint engine as
its own stage, but section 6 lists ten crates without one.

`Constraint` currently lives in `dre-planner`, its only consumer. If
constraint evaluation grows independently useful — used by the symbolic
reasoner, or exposed to model packs directly — split it out then.

## Decided

### Cross-machine determinism is in scope, and not yet falsified

The determinism claim is `SameInput + SameModel + SameEngine = SameDecision`, and it is meant to hold across machines. It was first narrowed to a single machine because only one host was available; a second host for the restart check removed that reason, so cross-machine determinism was put back in scope and tested.

Every committed experiment reproduces on the second host from the records committed on the first. No new tooling was needed: the committed baselines are the first host's, so `dre replay` run anywhere else is already the cross-machine comparison, and `stored_replay.rs` makes it part of the suite.

What it does not establish. Both hosts were the same CPU model running the same operating system build and the same rustc. The result rules out anything seeded from the machine (hostname, MAC address, per-boot address layout, an iteration order derived from an allocator address) and says nothing about CPU-feature divergence, because there was no divergent CPU to test against. `SameMachine` is unfalsified across the two hosts, not retired, and `determinism.md` states it that way.

It also depends on the two hosts being different physical machines. If both guests land on one physical host, the replay keeps passing, because same-host runs agree at least as readily as cross-host ones, and the evidence degrades into a weaker claim with the same green output. No in-guest check can detect that, so confirming placement is a procedure, not a test.

The next step is heterogeneous hardware: a CPU of a different model or vendor, and a different libc or kernel.

### A geometric space names an entity type, not an entity

`SpaceDecl.subject` was an entity identifier, so a pack classified exactly
one entity whatever else it declared. It now names an entity **type**, and
every instance of that type is classified, writing its own result onto
itself.

Found by writing `fault-network`, which has sixty components and could not
use the section at all. Recorded first as a limitation and fixed in the
following pass, which is the order `model-packs/README.md` asks for.

**Why nothing had noticed.** Both geometric packs called their single
entity the same thing as its type — `machine` of type `machine`, `pump` of
type `pump` — so a lookup by identifier and a lookup by type returned the
same entity. No pack could distinguish the two readings, because no pack
had two of anything. That is also why the change is transparent: neither
pack's decision or proof hash moved.

**Which is the hazard, not the reassurance.** Every stored baseline
verified against the changed engine without complaint, and the behaviour
had plainly changed — a pack with two entities of the type now yields two
classifications, and a pack whose subject names an entity that is not a
type now classifies nothing where it used to classify that entity. Both
halves of the accepts/decides rule, and neither visible in a single stored
record. `ENGINE_VERSION` moved to 0.6.0 for exactly that reason, and the
re-record diff is the shape `determinism.md` describes: thirteen records
moved `engine` and `engine_version` alone.

**What it changed elsewhere.** `active_dimensions` accumulates across
subjects instead of being set. It was documented as "set rather than
accumulated: a run classifies against one declared space", which was true
of the old semantics and would have reported the last component's readable
dimensions as though it were the run's — 1 instead of 60, a plausible
small number rather than an obvious fault. A single-instance pack is
unaffected, one call either way, which is why no benchmark row moved.

**The alternative, and why not.** A list of spaces would also have solved
`fault-network`'s problem, by letting it declare sixty of them. That is the
same limitation with more typing: the interesting property of a domain
with sixty components is that they are all the same *kind* of thing, and a
format that makes you enumerate instances cannot express that.

Both remain possible — a pack still declares one space, so a domain
needing two different spaces over two different types is still stuck. That
one has not been hit by a real domain yet, and on this repository's record
it should not be built until it is.

The narrower observation worth keeping: this is the third time a section
has turned out to be scoped to what existing packs happened to need
(`features.yaml` was inexpressible, the ontology was unread, geometry was
single-subject). Each was found by writing a domain rather than by reading
the code.

### ~~`requires_engine` is decorative on every pack~~ no longer true

**Fourteen of twenty-six packs now declare a real minimum** and a pack that
declares one genuinely cannot run below it. The entry is kept because the
reasoning that made it true is still what keeps it honest: a field every pack
sets identically is a field nothing exercises. What follows is the original,
from when all nine packs declared `0.1.0`.

All nine packs declare `requires_engine: 0.1.0`, and the loader checks it
honestly — `requires_engine > ENGINE_VERSION` refuses the pack. The check
works. Nothing has ever exercised it, because no pack claims to need
anything.

`fault-network` makes the gap concrete. It cannot run on an engine without
the feature layer: `deficit_percent` would be absent, every threshold rule
undecidable, and a network with an obvious fault in it would be diagnosed
as healthy — quietly, since an absent feature is not an error. It declares
`0.1.0` anyway, matching its eight predecessors, several of which have the
same problem.

Left as it is, deliberately. Assigning honest minimums means deciding which
engine version first made each section *reachable*, and the answer for
several of them is "later than the version that first parsed it" — which is
the same accepts/decides distinction `determinism.md` records, applied
backwards through history. Worth doing as its own pass, with the versions
established from the record rather than guessed at now.

Noted here because a field that is checked, satisfied by everything, and
believed to be doing work is the exact shape this repository keeps finding:
the ceilings that enforced nothing, the operators with no evaluator, the
ontology nothing parsed.

### The planner's strategy is in the `EngineHash`, for the proof's sake

An experiment may now declare `strategy`, and the value lands in
`RuntimeConfig` beside the ceilings and the proof mode — which puts it in
the `EngineHash`. The reasoning is worth recording because the obvious
argument for it is the one that does not apply.

`astar_matches_dijkstra.rs` had already written the rule down: several
routes are equally optimal on a grid, so two strategies could each return a
shortest path and disagree about *which*, and then the strategy would be
part of the decision. If that test ever failed, it said, record that
`Strategy` belongs in the `EngineHash`.

It does not fail. Measured on `grid-40-mid` — a 40-by-40 grid with the goal
at (19, 19) — A\* and Dijkstra return the identical 38-step plan. By the
rule as written, `Strategy` stays out.

The rule was half of one. A replay verifies five hashes, and A\* reaches
that identical plan having expanded 383 states where Dijkstra expands 727.
Fewer expansions means fewer rejected candidates recorded, so the
`ProofHash` differs while the `DecisionHash` does not. Confirmed by running
the same experiment twice under each strategy: decision `dbd7d1be…` both
times, proofs `adeb2837…` and `75b0321d…`.

Outside the engine identity, that pair of runs agrees on engine, model,
input and decision and differs on proof alone — a shape indistinguishable
from a determinism failure, which is precisely what the five hashes exist
to tell apart. So it goes in the hash, and `determinism.md` now states the
rule as **accepts / decides / any verified hash** rather than the first two.

Two consequences worth naming:

- `Strategy` moved from `dre-planner` to `dre_core::config` and is
  re-exported. Engine identity is computed in `dre-core`, so a knob that
  belongs in the hash has to be visible there. The enum is unchanged.
- `RuntimeConfig::default()` sets `Strategy::Dijkstra`, which is *not*
  `Strategy::default()` — that is breadth-first. The lab runner named
  Dijkstra at the call site for the whole MVP, so every committed record
  was produced under it. Taking the enum's own default would have re-planned
  thirteen experiments by a different search while looking like tidying up.
  A test asserts the two differ, so it cannot be quietly "fixed".

### "Every experiment" must be read from disk, not listed

`mvp_experiments.rs` and `stored_replay.rs` each held a hand-written array
of twelve experiment filenames. Adding `grid-40-mid-001` left it asserted
by neither, and the suite stayed green — each array was still perfectly
consistent with itself.

This is the defect those tests were written to catch, one level up. The
lesson recorded then was that an experiment nobody checks is a comment; the
fix at the time added the loop and left the *list* manual, so the same
failure could recur through the list rather than through the experiment.
It took one commit to recur.

Both now enumerate `experiments/*.yaml` through `dre_lab::experiment_files_in`,
sorted, since `read_dir` order varies by filesystem and a test that
iterated it would fail in a machine-dependent order. Each asserts a floor
on the count first: an enumeration that silently returns nothing passes
every loop it feeds.

### YAML parser: `serde-saphyr`, configured strictly

Chosen 2026-08-15 over the alternatives, measured rather than assumed:

| crate | version | last release | 90-day downloads | verdict |
|---|---|---|---|---|
| `serde_yaml` | 0.9.34+deprecated | 2024-03 | 87.8M | deprecated, 2 RUSTSEC advisories |
| `serde_yml` | 0.0.13 | 2026-05 | 6.6M | deprecated after unsoundness; now a shim |
| `serde_norway` | 0.9.42 | **2024-12** | 2.5M | a fork, but untouched for 20 months |
| `yaml-rust2` | 0.11.0 | 2025-12 | 13.8M | parser only, no serde integration |
| `noyalib` | 0.0.22 | 2026-08 | 216k | promising, but 0.0.x and three months old |
| **`serde-saphyr`** | **1.0.1** | **2026-08** | 3.4M | chosen |

`serde_norway` was the obvious pick — it is the direct fork of
`serde_yaml` — but "maintained fork" did not survive checking: no release
since December 2024. Inheriting a deprecated crate's design without
inheriting active maintenance is the worst of both.

`serde-saphyr` earns it on four specifics, not on being newest:

- **`#![forbid(unsafe_code)]`**, in the crate *and* in its `granit-parser`
  backend. Matches the workspace's own setting, and is exactly what
  `serde_yml` failed at.
- **Duplicate keys error by default.** Two rules sharing an id would break
  the total firing order, since `rule_id` is the final tie-break.
- **Parse budgets and alias limits are first-class.** DRE bounds every
  other resource (plan section 22); an unbounded parser in front of a
  bounded engine is an unbounded engine.
- **Panic-free parsing** is a stated design goal, matching the workspace
  rule that engine paths return errors rather than panicking.

Only `deserialize` is enabled. The engine reads packs but writes snapshots
as JSON, so the serialize half and its four transitive dependencies stay
out. The one required dependency is `serde_core`.

**Two settings are tightened beyond the crate's defaults**, in
`dre-model::yaml`:

`merge_keys: Error` — YAML merge keys (`<<`) let a rule inherit conditions
from an anchor defined elsewhere, so the rule no longer states what it
tests. An engine whose selling point is explainable decisions should not
accept a model format where the model itself is indirect. Revisit if packs
become repetitive enough that the indirection earns its cost.

`strict_booleans: true` — YAML 1.1 reads `yes`/`no`/`on`/`off` as
booleans, so a symbol value of `NO` silently becomes `false`. This is the
"Norway problem", and in a typed property it yields a type error at best
and a wrong value at worst.

`crates/dre-model/tests/yaml_policy.rs` pins all of this as behaviour
rather than as a crate name, so a future swap has a contract to meet.

### The conformance suite is `crates/dre-conformance`

Plan section 6 describes this layout.

As a normal workspace member it runs as part of `cargo test --workspace`.
That is deliberate rather than incidental: a conformance test you have to
remember to run is one that stops getting run.

Keep it a workspace member. Tests that are not part of a package are not
part of any Cargo target, so they would simply never execute — and a test
that never runs is worse than no test, because the absence looks like
coverage.

### Toolchain pinned to 1.97.1

`rust-toolchain.toml` names an exact version rather than `stable`, pinned from the first successful build.

The compiler is part of the engine identity. Bumping it is a deliberate
act, not a background upgrade: record the change here and expect stored
replay records to need re-recording.

### `Fixed` arithmetic is named `checked_*`

`add`/`sub`/`mul`/`div` were renamed to `checked_add` and friends.

Clippy's `should_implement_trait` flagged the originals as confusable with
`std::ops`, and it was right for a reason beyond style: these return
`Result`, and a method named `add` reads as infallible. `checked_*` matches
the std convention for fallible integer arithmetic and makes the failure
mode visible at the call site.

### `dre-lab` is its own crate

Section 18 requires a CLI with `run`, `replay`, `benchmark`, and
`validate`, and a binary needs somewhere to live, so `crates/dre-lab/`
provides the `dre` binary. Plan section 6 has been rewritten to include it.

It is also the crate where the determinism rules deliberately do not
apply — it is the harness around the engine, so it reads the clock and the
filesystem. Keeping that boundary at a crate edge makes it enforceable
rather than a convention.

### Model pack sections stay unparsed in `dre-model`

A model pack has sections owned by different reasoners: `rules.yaml` by
`dre-symbolic`, `geometry.yaml` by `dre-geometry`, and so on.

The obvious design has `dre-model` know all of those types — but then
`dre-model` depends on every reasoner while every reasoner needs the pack,
which is a dependency cycle.

So `dre-model` owns the manifest, the ontology, and the section
*container*, holding each section as unparsed `SectionSource` bytes. The
owning reasoner parses its own section. Adding a reasoner touches that
reasoner and the assembly point, never `dre-model`.

A second benefit: the `ModelHash` is taken over raw bytes, so it reflects
exactly what was on disk. Hashing a parsed representation would let a
parser upgrade change stored hashes without the model having changed.

### Reliability is an integer weight, not a probability

Evidence reliability is `0..=1000` (plan section 11).

Calling it a probability would invite arithmetic the semantics do not
support — multiplying independent reliabilities, say — and would imply a
calibration no model pack can honestly supply. It is a precedence weight
used by threshold rules, and naming it that way keeps the temptation
visible.

### Squared distances are never square-rooted

Ordering is preserved without the root, and a fixed-point square root
would introduce a rounding step whose direction is one more thing to
specify and test. Nothing in the engine needs true distance magnitude.

### Evidence freshness uses logical ticks

Not wall-clock timestamps. The same experiment run today and next year must
reach the same conclusion about staleness. Ticks arrive with the input.

### Errors rather than saturation on fixed-point overflow

`Fixed` arithmetic returns `Err` rather than saturating or wrapping.

Clamping would make results depend on how close inputs were to the
representable boundary — a hidden discontinuity, and exactly the kind of
thing that makes a reasoning result hard to trust when it eventually
matters.

### Both decision and proof hashes are compared on replay

Comparing only `DecisionHash` would pass a run that reached the right
answer by different reasoning. That is still a determinism failure, and
the one most likely to go unnoticed.

### Rejected candidates are recorded in the proof

A proof showing only the winning path answers "what did it conclude" but
not "why not the alternative". The second question is usually the one
being asked when someone distrusts a result.

### Proof provenance lives in `Trace`, not in `ProofGraph`

`ProofGraph` is the artefact that gets hashed, serialised, committed and
diffed. The map of "which node last established each subject" is
scaffolding used only while building it.

Putting the map inside `ProofGraph` would have avoided threading a second
type through `forward_chain` and `plan`, and was rejected because it
introduces a field that is empty after deserialization while `PartialEq`
still reports the two graphs as equal. A hidden field that silently empties
on a round trip is exactly the class of bug this project exists to make
visible.

The edges themselves *are* hashed. Two runs agreeing on every step but
disagreeing on what those steps rested on are not the same proof.

### Peak memory is read from the kernel, not counted by an allocator

Plan section 20 wants peak memory. A counting `GlobalAlloc` is the usual
answer and needs `unsafe impl`, which `unsafe_code = "forbid"` prohibits
workspace-wide — and `forbid` deliberately cannot be lifted by a local
`allow`. Weakening that to gain a benchmark number is a poor trade.

So `dre-lab` reads `VmHWM` from `/proc/self/status`. The measurement then
sits entirely outside the engine, in the crate that is already allowed to
read the clock and the filesystem, and there is no number the engine could
branch on even in principle.

Two consequences are accepted and stated rather than hidden: the figure
covers the whole process including the harness, and it is Linux-only. Off
Linux it reports "unavailable", never `0` — a zero would be a false claim
where unavailable is a true one.

### Unknown-resolution rate excludes `UNOBSERVABLE`

The denominator is properties that were `UNKNOWN` in the state as authored.
`UNOBSERVABLE` is a deliberate statement about the model, not a gap that
evidence could close, so counting it would make the rate look worse the
more honest a pack was about what cannot be measured.

Measured before evidence resolution runs, so resolution counts toward it —
the metric is titled "evidence handling" and resolution is the handling.

An experiment with no unknowns reports "none started unknown" rather than
0%, because those are different statements.

### "Different CPU counts" is tested structurally as well as behaviourally

A process cannot portably reduce its own CPU count from inside a test, and
shelling out to `taskset` would test the platform more than the engine.

Covered from both ends instead: the same work runs across 1, 2, 3, 8 and 16
threads, and a source scan asserts that no engine crate references
`available_parallelism`, `num_cpus`, `rayon`, `std::thread` or `std::env`.
A result cannot depend on a number nothing reads.

The scan is the same tactic `clippy.toml` uses to ban `HashMap` and
`Instant`, applied where no lint exists. It skips comment lines — several
engine modules explain at length why they do *not* read the clock or the
core count, and a scan that could not tell discussion from use would have
to be silenced with markers, which is how a fitness test becomes noise.

Neither half is the same as running on a machine with fewer physical cores,
and the tests say so rather than implying otherwise.

### A\* refuses an inadmissible model rather than searching it

An inadmissible heuristic does not crash, warn, or produce anything that
looks wrong. A\* returns a plan that is valid, reaches the goal, and is not
the cheapest — and "optimal" is not a property anything downstream can read
off a plan. The failure is invisible by construction, so the only place to
catch it is at the boundary.

`check_admissible` therefore runs before the search and returns an error.
Dijkstra is unaffected, so the model is still perfectly searchable; the
refusal says "not with this heuristic", not "not at all".

### Derived features live in `dre-symbolic`, not their own crate

A derived feature is arithmetic over properties; a rule is a conclusion
about them. Different enough to be separate sections in a pack, close
enough to share condition evaluation, three-valued semantics, subject
resolution and the fixpoint shape.

A `dre-features` crate would have duplicated all of that or depended on
`dre-symbolic` anyway, so the module is where it goes. The crate's own
documentation now says it has two ways to derive a fact, which is the
honest description.

Revisit if features grow past arithmetic — aggregation over related
entities, or anything needing its own evaluation strategy. At that point
the shared surface stops being the larger half.

### A failed derivation has four outcomes, not two

`INFERRED`, `UNKNOWN`, `INVALID`, `NOT_APPLICABLE`. Every one of them
leaves the property without a usable value, which is exactly why they have
to stay distinct — nothing downstream would notice them being collapsed:

- `UNKNOWN` — an operand was not available. Nothing was computed.
- `INVALID` — every operand was available and the operation has no result.
  Division by zero, or overflow.
- `NOT_APPLICABLE` — a guard says the quantity is meaningless for this
  entity. No arithmetic was attempted.

"We never measured the input power", "the input power is zero so
efficiency is not a number", and "this machine has no efficiency to speak
of" are three different facts. The middle one is a fact about the machine;
the other two are facts about the model and the observations.

Before features, `INVALID` and `NOT_APPLICABLE` were declared in
`EpistemicStatus`, guarded against in condition evaluation, and produced by
nothing. A status with no producer is a claim the engine makes about itself
and never has to keep.

### Features are evaluated to a fixpoint in identifier order

Not in declaration order. A pack should not have to be topologically
sorted by hand: that makes the *ordering* of the file semantic, and
orderings get edited by people who are thinking about something else.

The shipped `process` pack declares F001 before the F002 it depends on,
deliberately, so the property is exercised rather than merely documented.

A dependency cycle makes no progress and leaves its members `UNKNOWN`,
which terminates quietly. That is better than an error: a cycle is a
modelling mistake the pack author can see from the output, and it does not
prevent the rest of the run from producing an answer.

### Unresolved features are recorded dependencies-first

Ordering that affects the proof rather than the result. Every unresolved
feature writes the same status whatever sequence they go in, but a proof
edge can only point at a node that already exists — so writing
`efficiency_percent` before `efficiency` leaves "why could it not be
derived" with no edge to follow, when the answer is two nodes further down.

The result is unchanged either way, which is precisely why this had to be
decided deliberately: nothing would have failed.

### Features run after evidence and before rules

The only order that works. A feature reads observations; a rule then
reasons about the derived quantity as though it had been measured.

`InputHash` is taken *before* derivation, so it covers the state as given
to the engine. Hashing after features would fold the engine's own work into
the description of the problem, and two engines deriving different features
from identical observations would look like they had been asked different
questions.

### ~~`ontology.yaml` is parsed by `dre-state`, and enforced only at validation~~ the enforcement half was superseded 2026-08-15

The parsing half stands. The ontology describes the shape of state, so
`dre-state` owns the types. That crate depends on `dre-core` alone, so parsing
it there introduces no new edge.

**The enforcement half is now the other way round, and this entry went on
asserting the old arrangement after it stopped being true.** Engine 0.7.0 put
the check in the run: `defect_in` (`crates/dre-lab/src/runner.rs:296`) asks an
incoming reading the same three questions validation asks of a declared value
— kind (`:304`), domain (`:312`), vocabulary (`:319`) — and `apply_evidence`
(`:329`) excludes a defective one from resolution, recording a `Rejected` node
that names the source and the reason (`:392`). Meanwhile
`validate::experiment` (`crates/dre-lab/src/validate.rs:1158`) still does not
walk `given.evidence` at all. So a defective reading is caught when the
experiment **runs** and not when it is **checked**, which is the exact inverse
of the sentence this entry used to carry.

The reasoning here was right; only the conclusion expired. Enforcing at run
time *was* a change to reasoning behaviour, and it did get its own pass with
re-recorded baselines — the bump changed exactly one stored decision, and
deliberately: `device-cascade-001` no longer concludes `POWER`, because the
defect had been suppressing the competing explanation. The gate also lives in
`dre-lab` rather than in the engine crates, so "enforcement is not in the
engine" survives in that narrow sense while "a run does not" does not.

⚠ **Still open, and it is the offline half** — `dre validate` walking
`given.evidence`. Measured 2026-08-16 across the whole corpus, 290 evidence
rows in 58 experiments against 27 ontologies: exactly one row is outside its
declared domain, `device-cascade-001`'s `E020` at 9999 against a declared max
of 200, and that one is a deliberate fixture which the run already rejects and
the experiment already documents. Nothing else fires. Against the bar the
exclusivity check set — every occurrence a real defect, with a test asserting
the silence elsewhere — that is not a check worth building yet, and the
cheaper option ranked above it in `roadmap.md` is to say it in the brief.

An outside review recommended building it as warnings. It found something
better than its own recommendation: `defect_in` opens with two `?`
early-returns, so a lookup miss reads as *no defect*. An unknown entity still
fails downstream at `StateBuilder::draft_mut`, but a declared entity carrying
an **undeclared predicate** is accepted in silence, because `StateBuilder::set`
(`crates/dre-state/src/builder.rs:188`) never consults the ontology. That hole
is real and nothing catches it — and it is a narrower thing to close than the
four checks proposed around it.

Until 2026-08-15 the file was loaded as bytes and parsed by nothing, which
had one instructive consequence: it was already in the `ModelHash`, so
editing a file nothing read still changed the model's identity. When the
`diagnosis` ontology was corrected, its two experiments reported
`NOT COMPARABLE` on `[Model]` with decision and proof hashes unchanged —
the model became a different model without the engine changing its mind,
which is precisely the distinction the five-hash scheme exists to draw.

### Validation has two strengths, and the weaker one is not redundant

A pack's subjects name entity **instances**; the ontology describes
**types**; and which type an instance has is something only an experiment
says. The `logic` pack is why this cannot be waved away — it declares one
type, `proposition`, and reasons about `a.holds`, `b.holds` and `c.holds`.

So `validate::pack` checks that every property named anywhere is declared
on *some* type, and `validate::experiment` binds instances to types and
checks each subject against the type it actually has. The first catches
typos, which is the common case, and is all a pack can support alone.

A fault reported by the pack pass is not repeated by the binding pass. One
mistake gets one voice, or a reader has to work out whether two lines are
two problems.

### A region may declare no bounds

`Region::contains` already documented that a region may bound only some
dimensions and be a slab through the space rather than a box. A region
bounding *none* of them is the limit of that rather than a special case, so
`bounds` is optional and empty means unbounded.

It was required until a test fixture wanted a purely centroid-defined
region and could not express one. Nothing in the design had asked for the
requirement; it was the serde default.

### `DecisionHash` covers the plan, not just the state

The planner works on a clone of the state and returns a plan without
applying it. So the state after planning is the state before it, and a
`DecisionHash` taken over the state alone captured no part of the plan.

The consequence, once the planner ran at a scale where two problems could
obviously differ: a 5-by-5 grid solved in eight moves and a 40-by-40 grid
solved in seventy-eight, from different packs with different obstacles,
hashed to exactly the same `DecisionHash`. Both start with an agent at the
origin.

Replay still caught a changed route, because it compares the proof as well
— but as `ProofMismatch`, "same decision, different reasoning", which for a
plan is backwards. A different route is a different decision.

The encoding is uniform: tag, state, then the plan as an option. Hashing
the bare state when no plan exists would have preserved five of the seven
baselines, and was rejected — a hash function that changes shape depending
on what the run happened to produce is the kind of conditional encoding
that causes trouble later. Every baseline was re-recorded once instead.

Found by scale, not by review. It had been wrong since the planner was
written and no test could see it, because every planning experiment in the
repository was a single 5-by-5 grid.

### An experiment may declare its own limits

Limits are part of the `EngineHash` by an existing decision: two runs under
different ceilings can legitimately reach different answers, so they are
different engines. What was missing was any way for an experiment to say it
needed a different one — the ceilings were global constants, and
`max_depth: 20` cannot cross a 40-cell grid.

`limits:` in an experiment overrides the MVP defaults field by field.
Declaring it is not a way to make a problem fit; it is an admission that
the problem needs a different engine, and the `EngineHash` records that.

Absent means the defaults, which is why adding the field left every
existing experiment's `EngineHash` untouched.

### Optimality is asserted by plan length at scale

`expect.plan` names the route exactly. That is the stronger claim and the
right one on a small pack, but seventy-eight action identifiers transcribed
into YAML is an assertion nobody reads when it fails.

`expect.plan_length` asserts the property that matters — the plan is as
short as the geometry allows, so it is optimal. *Which* of the roughly
10^22 equally short routes comes back is the frontier ordering's business,
and is pinned by the replay record rather than by the experiment file.

### A source may not vouch for itself

`Source::reliability` was declared and never read. Only the per-observation
figure reached the corroboration count, so an observation could claim any
reliability it liked regardless of what the pack said its source was worth.

That is not a cosmetic gap. The consequence is a denial of conclusion:
`diagnosis-003` is a machine whose temperature is backed by two genuinely
independent probes and disputed by four sensors sharing a bus. Add one more
source, declared at reliability 100, submitting a reading that claims 900,
and the disputed value reaches two independent groups. The engine then has
strong support for two incompatible values, returns `CONFLICTING`, and
**every rule downstream stops firing** — no bearing fault, no failure mode,
no condition, no classification. Verified by reverting the fix and running
it: 3 rules fired becomes 0.

So a single misdeclared source could suppress a whole diagnosis, and the
run would look like an honest inability to decide.

Trust is now the **minimum** of the source's declared reliability and the
observation's own. The source sets a ceiling; the observation may lower it,
because a reading can legitimately be worth less than its instrument — a
partial measurement, or a sensor reporting a degraded mode — but never
more.

An observation from a source the pack never declared keeps its own figure.
There is no ceiling to apply, and treating an undeclared source as
worthless would silently discard evidence rather than reporting a model
that is missing a declaration, which is `dre validate`'s job.

Every existing test gave a source and its observations the same figure,
which is exactly why nothing noticed for the whole MVP.

### Corroborated disagreement is not settled by counting

Two values each backed by enough independent groups give `CONFLICTING`,
however lopsided the split. Five independent groups against two is still
`CONFLICTING`, not 850.

This is deliberate and it is worth restating because it is surprising in a
specific way: **adding evidence can make the engine less decisive.** Two
independent probes agreeing gives `KNOWN`; adding two more independent
probes that disagree gives `CONFLICTING`. Gathering more information moved
the result from an answer to a refusal.

The alternative — prefer whichever value has more independent groups, tie
means conflict — is more decisive and is not obviously worse. Independence
is what makes counting meaningful, so counting independent groups is not
the error that counting raw sources is.

It is left as it stands because the conservative reading is the one the
rest of the engine takes everywhere else: decline rather than guess. If it
changes, it should change as a decision about what a reasoning engine owes
its caller, not as a tweak to make an experiment pass.

### The planner settles states on pop, not on generation

Dijkstra was returning plans that were valid, reached the goal, and were
not the cheapest. The frontier marked a state seen the moment it was first
*generated*, which is only correct if the first route to reach a state is
the cheapest one — and in a uniform-cost search it is not. A state can be
generated early by one expensive action and reached later by a cheaper
sequence, whose arrival was then discarded as already seen.

Settling on pop, with duplicates allowed onto the frontier and skipped when
popped, is the standard lazy-deletion form. A decrease-key heap would save
memory rather than buy correctness, and costs a data structure this does
not need.

Found by an independently authored pack, and it could not have been found
any other way here: every pack written alongside the engine used unit-cost
actions, and with uniform costs generation order tracks cost order exactly.
The failure needs varied action costs to appear at all. This is the same
invisible shape as an inadmissible heuristic — a plan nothing downstream
can tell is suboptimal — which is why `check_admissible` exists and why
this went unnoticed beside it.

### The frontier ordering ends in the path

Cost, depth, action identifier, state hash — and now the path.

The first four do not separate two routes that arrive at one state with
equal cost and depth by the same final action, which happens whenever that
action erases the difference between their predecessors. Any `set` effect
can do that: setting `x` to 5 from `x = 1` and from `x = 2` lands in the
same place.

The previous version never met the case, because settling at generation
kept the second route off the frontier entirely. It was deterministic
partly *because* of the bug that made it return suboptimal plans, which is
worth stating plainly: fixing a correctness defect removed the accident
that was covering a determinism one.

### A failed property expectation names the plan/execute distinction

The planner searches over a copy of the state and returns a sequence
without applying it, so a property an action would have changed still holds
its pre-plan value.

Two independent authors — one of them the engine's own — wrote a first
planning experiment expecting otherwise. Twice is a defect in how the
distinction is presented rather than two coincidences, so the failure
message now explains it at the moment it bites, which is the only moment
anyone is reading.

### Validation catches conditions that can never be decided

A rule comparing an integer literal against a fixed-point property does not
fail. Unlike kinds compare as `unknown`, so the rule quietly never fires
and the run reports nothing unusual — the same silent failure as a typo'd
subject, arriving through the numeric type system instead.

Two checks, from the same fact that every feature produces fixed-point
whatever the operation:

- A property a feature writes, declared `integer` in the ontology, is the
  pack contradicting itself. The declaration says one thing and the pack's
  own arithmetic writes another.
- A comparison whose literal kind cannot meet the property's effective kind
  is a condition that can never hold, reported even when the ontology is
  right and the rule is wrong.

The "effective kind" is the declared one, overridden to fixed-point when a
feature writes the property, because that is what is actually there at run
time.

Found by the second independently authored pack. Its author declared
`custody_gap: integer` — entirely reasonable for a difference of two record
counts — wrote a feature to compute it, and compared it against `0`. Three
downstream properties then never came into existence and the run looked
ordinary.

The leniency that allowed it was deliberate: `PropertyKind::accepts` treats
integer and fixed-point as interchangeable so that a pack need not
re-declare a property's kind because of *how* it happens to be computed.
That decision is still right for value checking, and it was exactly what
let the contradiction through. The fix is a separate check rather than
tightening the first one.

### The plan/execute hint fires only for properties the plan writes

Its first version fired whenever a run produced a plan at all, and promptly
attributed a numeric type mismatch to the plan/execute distinction — a
confident explanation of the wrong thing, which is worse than no
explanation. It now consults the effects of the actions in the plan.

### Conditions are read by explicit dispatch, not as an untagged enum

`Condition` was `#[serde(untagged)]`, and the comment above it said the
variant order was chosen carefully so `Compare` would be tried before
`Status`. That was true and it was not enough.

An untagged enum tries each variant and **discards the error** from every
one that fails. `Compare` and `Status` differ only in that `Compare` also
carries an `operand`, and serde ignores unknown fields by default. So a
`Compare` whose operand failed to parse did not fail — it fell through to
`Status`, which accepted the identical mapping and dropped the operand.

The condition changed meaning during parsing. An author writing

```yaml
- subject: artifact.custody_gap
  operator: GT
  operand: 0.0
```

got a status test on `custody_gap`, not a comparison. It loaded. It
validated clean — there was no operand left for a validator to object to.
Every rule built on it was silently dead.

Found in the second independently authored pack, where it cost two rules
and three conclusions. The pack's author had followed the brief exactly;
the brief documented that `Value` rejects floats, and it does. The
rejection simply never reached anyone.

Dispatching on which keys are present means a malformed `Compare` is a
malformed `Compare`. `deny_unknown_fields` on the field set is part of the
fix rather than tidiness: it is what turns a misspelled key into an error
instead of a condition quietly missing a clause.

Serialization is still untagged, so a condition round-trips to the plain
mapping an author wrote. `condition_parsing.rs` checks the hand-written
reader against the derived writer, because they can now disagree.

### `IN`, `AND`, `OR` and `NOT` are gone from `Operator`

Four of the thirteen operators had no evaluator. `compare` and `status`
both fell through to `Truth::Unknown` for them, so a condition using one
could never fire, and nothing said so — the same silence as every other
defect found this day.

Nothing used them: not the engine, not a test, not a pack. `Condition`
already expresses conjunction, disjunction and negation as combining
forms, which is where they belong; `IN` would need a set-valued operand,
and `Value` has none.

Removing them is the honest fix. An enum that offers what the engine
cannot do is a trap with a docstring.

### The four unenforced limits are enforced rather than removed

`max_rules`, `max_dimensions`, `max_regions` and `max_actions` were parsed,
documented, folded into `EngineHash` — and read by nothing. Half the
resource ceilings the engine advertised were decorative, and the hash
asserted a difference between two engines that would in fact decide
identically.

This is the fifth instance of the shape this project keeps finding
(declared, hashed, never read) and the one closest to the identity
primitive itself, which is the argument for enforcing rather than
deleting: plan section 22 says bound every resource, and three of the four
bound something real. `max_dimensions` and `max_regions` are checked per
space rather than per pack, because the arity of the distance computation
is what costs anything.

### A hard constraint declares what undecidable means

Three-valued logic makes the third case unavoidable, and the planner used
to answer it the same way every time: permit. That is right for a
procedural invariant and wrong for a safety one.

A safety invariant exists for the case where information is missing.
Permitting on undecidable means the one situation it was written for is
the one situation it does not cover. But blocking on undecidable is not a
safe default to impose either: early in a search most properties are
undecided, and a blocking constraint prunes every candidate before the
facts that would satisfy it can be established, so the planner reports no
path where one plainly exists.

Neither reading can be chosen once for the engine, so `on_undecidable`
is declared per constraint and defaults to `block`.

**Default `block`** because the alternative derives a permission from an
absence, and this engine's stated position is that nothing is derived from
a default. A pack needing the other reading says so, and the saying is
covered by `ModelHash`.

**In the pack, not in engine configuration**, for the same reason. An
engine-level switch would put a pack's meaning in a setting the pack
cannot see, which is the exact shape of every defect listed above.

Soft constraints are unchanged: they are priced rather than enforced, and
charging for a violation the model never established would bias the search
on missing information.

### Changing reasoning behaviour moves `ENGINE_VERSION`

`EngineHash` covers the engine version and the runtime config, and nothing
else. The version being tracked separately from the crate version is what
lets a release that changes no reasoning behaviour leave stored records
valid — and the converse is an obligation, not a convenience.

Each of the four changes above alters what the engine accepts or decides.
Left at `0.1.0`, `EngineHash` would have claimed that an engine which
blocks on undecidable constraints is the same engine as one that permits,
and every record produced under the old behaviour would have verified
against the new one.

So `ENGINE_VERSION` is `0.2.0` and all ten baselines were re-recorded.
`stored_replay.rs` caught this without being asked to — it compares each
experiment against its committed record, and reported all ten as recorded
under `0.1.0` and not comparable.

### Geometry writes what a pack asks for, and `direction` still does not

Plan section 16 requires five geometric operators. All five were
implemented and three — `nearest_region`, `boundary_distance` and
`direction` — had **zero call sites anywhere in the workspace**, tests
included. Two of them no model pack could invoke at all.

Milder than the other findings of the day: nothing hashed them and nothing
claimed they had run, so no result was ever wrong because of them. But an
operator a pack cannot reach is not an implemented operator, and the plan
said otherwise.

`boundary_distance_into` and `inside_region_into` are now optional fields
on the space declaration. Optional because a pack that does not reason
about margins should not gain properties it never mentions, and absent by
default means no existing pack changed.

The pair earns its keep by being distinguishable: `classify_into` names
the *nearest* region whether or not the point is in it, so a reading can
be classified `FAILURE_A` while sitting outside every declared region. A
pack asserting only the classification cannot tell membership from
ranking. `diagnosis` now declares all three, which is what makes the path
exercised rather than merely present.

`direction` is deliberately still unreachable. It returns one ordering per
dimension and no property kind holds a vector; naming a property per axis
is a format decision rather than an oversight to patch in passing. It is
tested as a library function and recorded here as unreachable, which is
the difference between a decision and an omission.

### Every experiment checks its own expectations

`assert_passes` covered three experiments of twelve. Among the other nine,
`cold-chain-release-001` had been failing since the day it landed:
its author asserted `release_authorized: true`, which is the world *after*
the plan is carried out, and the planner does not carry plans out. `dre
run` exited non-zero on it every time and nothing was watching — the
README meanwhile described it as the clean path.

Both cold-chain experiments now assert `plan:` instead, naming the actions
and their order. That is what the original expectation meant and is the
stronger claim; the superseded version is quoted in the file so the
author's original prediction is still legible.

An experiment is a claim about what the engine concludes. One nobody
checks is a comment, and this repository has now shipped one for long
enough to argue the point.

### Every hand-written pack structure denies unknown fields

The `Condition` fix was applied at one site, and the property it relied on
is general: serde skips fields it does not recognise, so a misspelled key
anywhere in a pack was dropped without complaint.

None of these fail. They narrow or widen the model silently:

| Written | Loaded as |
|---|---|
| `perconditions:` on an action | an action with no preconditions, applicable everywhere |
| `hard: ture` on a constraint | a soft constraint |
| `vaules:` on a property type | a property with no closed vocabulary, so `dre validate` has nothing to check |
| `on_undecideable: permit` | the default, `block` — the opposite of what was asked for |
| `plan_lenght:` in an expectation | an experiment that asserts nothing and passes |

Every downstream artefact is then internally consistent with the model
that loaded rather than the one that was written, which is what makes it
undetectable: the decision, the proof and the replay record all agree.

`deny_unknown_fields` was already on `RuntimeConfig`, `EvidencePolicy` and
the pack manifest, so this was not a new idea — it had simply never been
applied to the sections a pack author spends all their time in. It is now
on all thirty structures written by hand in a pack or experiment.

Deliberately **not** applied to externally tagged enums (`Effect`,
`Expression`, `Operator`): an unknown variant name already errors there,
and the attribute means nothing for newtype variants. Nor to machine
written artefacts — proof nodes, replay records, benchmark rows — where
the writer is this engine and a strict reader would only make old records
unreadable after an additive change.

No shipped pack was relying on a key being ignored. That was checked
rather than assumed, and had one been, it would have been a finding rather
than a reason to relax the check.

## Explanation ranking is a separate layer, and a separate crate

Engine 0.11.0. Decided while closing plan section 26's explanation track,
which an independent review had identified as the largest missing reasoning
capability — correctly, and on the same evidence the repository had already
produced.

### Why not fold it into the planner

The two questions look adjacent and are not:

```text
explanation   what most plausibly explains what I observe?
planning      given that, what action sequence reaches the goal?
```

They fail differently. An explanation can be wrong while every action in the
plan is valid, and a plan can be wrong about a world the explanation
described correctly. Sharing a module makes "the diagnosis was wrong" and
"the route was wrong" the same kind of error, and there is then nowhere to
record which one happened — which is exactly the distinction anybody
debugging a bad outcome needs first.

So `dre-explain` does not depend on `dre-planner`, and `dre-planner` does
not depend on it. Structural rather than conventional, because a convention
survives until the first time somebody needs one number from the other side.

### Why the weights are engine constants

They could have been pack fields. That would have been worse in a specific
way: a modeller who disliked a ranking could adjust the weights until the
intended hypothesis won, and the resulting pack would encode the answer
rather than the reasoning. Two packs would then be reporting one number on
two different scales.

As engine constants they are covered by `ENGINE_VERSION` and therefore by
`EngineHash`, so changing one is an engine change — visible in every record,
arguable in one place, and impossible to do quietly for a single domain.

### Why a mechanism is an independence group

The naive score is a symptom count: `POWER` explains five things, `THERMAL`
explains one. It reaches the right answer on `device-cascade` and it is the
wrong rule, because four sensors on one degraded bus are one thing happening
reported four times.

This is not a new idea in this repository. `dre-evidence` resolves by
independence group rather than by vote precisely so that three readings off
one bus cannot outvote one reading off another, and `bus-independence` is
three experiments about what happens when the grouping is not declared. The
explanation layer reuses it: observations a hypothesis hangs off one
mechanism corroborate at a discount, observations under different mechanisms
count in full.

The consequence worth stating is that the **shape** of an explanation
decides its score rather than the length of its symptom list.

### Why two levels and no deeper

A hypothesis has mechanisms; a mechanism has observations. No `from:`, no
nesting, no general DAG.

`device-cascade` needs exactly two levels and nothing has needed three. The
standing rule — no new reasoning abstraction without a failing domain that
requires it — applies with unusual force here, because a deeper tree is easy
to add later and impossible to remove once packs have declared it. When a
domain arrives whose mechanism genuinely has a sub-mechanism, that domain
should decide the shape of the extension.

### Why an exact tie writes nothing

The ranking's order is total, so there is always a first element. Writing it
into `rank_into` regardless would be wrong: a lead existing only because one
identifier sorts before another is a tie-break, not a finding, and it would
enter the state wearing the same clothes as a real conclusion.

So the write requires a supported mechanism **and** a strict lead. "This
evidence does not separate these accounts" is a genuine result, and it is
the one `dre survey` then acts on by finding an observation that would.

### What was left unfixed, and stated

The observation set is shared across hypotheses, so adding an observation
helps whoever explains it and charges everybody else in full. Correlated
symptoms are worth roughly 40% of independent ones — discounted, not
neutralised — so a large enough correlated group can move a ranking.

The defence is half sound. Correlation is declared *inside* a hypothesis, so
an account offering no grouping for six live facts pays for six — but the
format is what chose to hide correlation inside hypotheses, so this is a
consequence of a design decision rather than a fact about the world. It is
also a lever for tilting a ranking by writing the pack a particular way.

**The first fix recorded here was wrong, and is kept as a rejected
alternative.** It was to pool declared correlations across hypotheses by
union. An independent review pointed out that this is gameable in a new
direction: a hypothesis could group unrelated observations solely to cut
every rival's unexplained penalty.

The shape that works is to make correlation groups first-class in the shared
observation set, with mechanisms referring to observations or to groups —
shared questions should have shared independence structure. Not built,
because the case that exposes the asymmetry was constructed by hand to break
the rule rather than found while modelling anything.

Because that asymmetry is knowingly unresolved, the explanation layer is
**experimental rather than stable semantics**. `requires_engine` is what
makes that safe to change later.


## Declared traversal, and the two designs that lost

Engine 0.18.0. The most-reported limitation in the project's history: four
independently authored packs each collapsed a graph into one entity because a
rule referencing two entity types is refused.

Three designs were produced independently and judged adversarially. This file
exists to record *why* a choice was made, and the version log names only the
winner, so the two that lost are recorded here. The reason they lost is the
whole argument for the shape of the one that won.

### What both losing designs did

Both bound a **second variable**. One traversed a declared relationship and
let a rule conclude on either endpoint; the other put an explicit quantifier
on the rule, naming variables and a `where:` predicate.

Both are more expressive than what shipped. The first can conclude about the
entity it reached, which declared reach deliberately cannot, and that is a
genuine loss -- confirmed later by the first outside author to use traversal,
who wanted exactly that.

### Why they lost anyway

**They converted one silent decision into another.** The standing refusal
exists because choosing cross-product or pairwise silently "would make an
answer depend on a decision nobody wrote down". A bound second variable
replaces *which pairing* with *which quantifier* -- and the trade is not
neutral, because the old undeclared decision **refuses** and the new one
**fires**. An answer that arrives confidently is worse than none. Both
submissions conceded this themselves.

**They cost the termination argument.** Neither could keep `bindings_for`'s
signature or `already_fired`'s key type, and both had to introduce a new
activation ceiling, because a join blows through every existing limit. Reach
adds one ordering level -- target entity id ascending over a deduplicated set
-- and no binding axis, so activations stay `rules x instances` and the
existing argument holds unchanged.

**They were harder to author correctly.** Three of the four preceding
external authors could not write the format correctly on their first attempt.
A sigil grammar like `a:component.impaired` is a *legal* `Id` that resolves to
nothing and reads `UNKNOWN` forever -- a pack that loads, validates clean and
concludes nothing. A structured block under `deny_unknown_fields` makes every
typo a parse error instead.

### What was given up, stated plainly

Reach writes only to its anchor. To conclude about the entity reached, the
rule is written from the other end with `direction: in`. The first author to
hit that limit did not find the workaround, which is a defect in the brief
rather than in the format -- and it is the one place where a losing design
was genuinely better.

### `collapse` has no default, and that is the design

A traversal reaching several entities has the same problem a join does: one
truth is needed. There is no spelling of a reach that omits the answer. `the`
over several targets is an **error**, not a pick -- it asserts the traversal
is functional, and where it is not the pack has said something false about
its own topology.

There is no `none`, because `not:` around a `some` gives it with correct
Kleene negation. That decision has since been endorsed by an outside author
who went looking for a fourth option and reported wanting `at_least: N`
instead, which became 0.20.0.

## Dominance is declared on the value domain, not inferred from rules

Engine 0.14.0. Two rules concluding different values for one property give
`CONFLICTING` and discard the value. Right for a **belief**, wrong for a
**policy** -- and an outside author modelling OPC UA access control hit the
difference exactly: they predicted `CONFLICTING`, said plainly the domain
answer was `deny`, and left the clash in rather than fudge it.

**The rejected alternative was letting rule priority decide the merge.** It
is the obvious fix and it is wrong: priority is an authoring convenience that
orders *firing*, and making it decide *merging* would let the order somebody
typed their rules in manufacture facts. The same objection retires
specificity as a candidate -- and a doc comment claiming specificity already
overrode survived for months precisely because nothing exercised it.

So the order is declared on the **value domain**, in the ontology, where
`ModelHash` covers it. Inference only: sources disagreeing about an
observation is an evidence question with provenance to reason over.

## A symptom and a negative finding are not the same observation

Engine 0.19.0. The explanation layer scored coverage over every live
observation, so an account was credited for explaining that things are
**fine** -- and the fifth external author's pack returned the wrong
hypothesis because of it, while they predicted the right one.

The alternative considered was leaving it to modelling discipline: authors
simply should not declare healthy states as observations. That fails for a
reason worth recording -- a healthy finding has real diagnostic value,
because it **refutes**. `dns_ok` correctly contradicts a DNS hypothesis.
Telling authors not to declare it would remove the refutation along with the
false credit.

So an observation declares which it is. Default `true`, so nothing written
before it moved.

## Relationship cardinality is enforced in the state, not in validation

Engine 0.17.0. `targets_per_source: one` means a `from` entity has at most
one edge of that type, and `StateBuilder::relate` refuses the second.

This looks like it contradicts the standing preference for checking in
`dre validate`, and does not. Validate is a separate pass with **no path into
a decision** -- `dre run` never invokes it, so a constraint that only lives
there is advice. And this one is load-bearing at run time: `collapse: the`
asserts a traversal is functional, and the only thing that can make that
assertion safe is the state refusing to hold a second edge.

Two explicit fields rather than one `one_to_many` spelling, because which
side the "one" refers to in that spelling is precisely the kind of thing this
repository has been burned by. Both default to `many`, the weakest claim, so
packs that already declared relationships gained no guarantee they had not
asked for.
