# Deterministic Reasoning Engine (DRE) — MVP Plan

## 1. Objective

Build a **domain-agnostic deterministic reasoning engine** that can evaluate structured problems using symbolic, geometric, and causal reasoning while guaranteeing reproducible outputs.

The MVP is not intended to be generally intelligent.

Its purpose is to prove that a single deterministic runtime can:

1. ingest a domain model,
2. construct a canonical state,
3. distinguish known, unknown, inferred, and conflicting facts,
4. reason using multiple methods,
5. generate candidate actions or conclusions,
6. select a result deterministically,
7. explain exactly how that result was reached,
8. replay the same problem and produce the identical result.

The primary success invariant is:

\[
SameInput + SameModel + SameEngine = SameDecision
\]

Note that what is actually *verified* is narrower — cross-machine
determinism is out of scope, so the tested property carries a
`+ SameMachine` term. See section 21.

---

# 2. MVP Scope

The MVP should contain three layers:

```text
DRE Lab
   │
   ▼
Model Packs
   │
   ▼
DRE Core
```

## DRE Core

The core implements reasoning mechanics.

It must not contain domain-specific assumptions.

## Model Packs

Model packs define individual reasoning environments.

Examples:

- logic puzzles,
- mechanical diagnosis,
- navigation,
- game-state reasoning,
- synthetic causal systems.

## DRE Lab

DRE Lab runs experiments, benchmarks, replay tests, and comparisons between reasoning strategies.

---

# 3. Core MVP Capabilities

The first implementation should support:

- typed entities,
- typed properties,
- relationships,
- canonical state,
- evidence,
- unknown values,
- deterministic rules,
- geometric state representation,
- causal transitions,
- constraints,
- goals,
- candidate generation,
- deterministic scoring,
- deterministic tie-breaking,
- proof generation,
- replay,
- benchmark execution.

Do not implement machine learning in the MVP.

---

# 4. Proposed Architecture

```text
                   Problem / Experiment
                           │
                           ▼
                  ┌─────────────────┐
                  │ Model Pack      │
                  └────────┬────────┘
                           │
                           ▼
                  ┌─────────────────┐
                  │ State Builder   │
                  └────────┬────────┘
                           │
                           ▼
                ┌─────────────────────┐
                │ Canonical State     │
                │ + Evidence          │
                └──────────┬──────────┘
                           │
            ┌──────────────┼──────────────┐
            ▼              ▼              ▼
       Symbolic        Geometric        Causal
       Reasoner         Reasoner        Reasoner
            │              │              │
            └──────────────┼──────────────┘
                           ▼
                  Candidate Results
                           │
                           ▼
                  Constraint Engine
                           │
                           ▼
                  Deterministic Planner
                           │
                           ▼
                      Decision
                           │
               ┌───────────┴───────────┐
               ▼                       ▼
          Proof Graph               Replay Hash
```

---

# 5. Technology Choice

## Runtime

Use **Rust**.

Recommended reasons:

- deterministic execution behavior,
- strong typing,
- explicit data structures,
- good performance,
- precise control over ordering,
- suitable for future scaling.

**Pin the toolchain to an exact version** in `rust-toolchain.toml`, not
`stable`. The compiler is part of the engine identity: a bump can change
optimisation decisions and, in principle, observable behaviour, so a
replay record produced under one compiler is not strictly comparable to
one produced under another. Tracking `stable` means the toolchain moves
with no record of having moved. Currently pinned to **1.97.1**.

## Experimentation

Use **Python** as an optional external tool for:

- dataset generation,
- visualization,
- benchmark analysis,
- model-pack generation.

Python should not determine authoritative runtime decisions.

## Storage

For the MVP:

- files for model packs,
- SQLite or PostgreSQL for experiment results,
- JSON or MessagePack for snapshots.

Avoid a graph database initially.

The graph should first exist as deterministic in-memory structures.

---

# 6. Repository Structure

```text
dre/
│
├── crates/
│   ├── dre-core/         phase 1  — deterministic runtime foundation
│   ├── dre-state/        phase 2  — canonical state
│   ├── dre-model/        phase 3  — model pack format
│   ├── dre-symbolic/     phase 4  — symbolic reasoning
│   ├── dre-evidence/     phase 5  — evidence reasoning
│   ├── dre-geometry/     phase 6  — geometric reasoner
│   ├── dre-causal/       phase 7  — causal reasoner
│   ├── dre-planner/      phases 8-9 — constraints, goals, search
│   ├── dre-proof/        phase 10 — proof engine
│   ├── dre-replay/       phase 11 — replay
│   ├── dre-lab/          phase 12 — the `dre` CLI
│   └── dre-conformance/  determinism conformance suite
│
├── model-packs/
│   ├── logic/
│   ├── diagnosis/
│   └── navigation/
│
├── experiments/
│
├── benchmarks/
│
├── schemas/
│
├── tools/
│
└── docs/
```

## Notes on the layout

**`dre-lab` is a crate, not just a layer.** Section 18 requires a CLI, and
a binary needs somewhere to live. It is also the one crate where the
determinism rules deliberately do not apply: it is the harness *around*
the engine, so it may read the clock and the filesystem. Keeping that
boundary at a crate edge makes it enforceable rather than a convention.

**The conformance suite is `crates/dre-conformance`.** As a workspace
member it runs under `cargo test --workspace`, which matters: a
conformance test that has to be remembered is one that stops getting run.
Keep it a workspace member — tests outside a package belong to no Cargo
target and would silently never execute.

**Constraints and goals live in `dre-planner`.** The architecture diagram
in section 4 shows the constraint engine as its own stage, but the planner
is its only consumer. Split it out if constraint evaluation ever becomes
independently useful.

**`dre-model` holds pack sections as unparsed bytes.** Sections are owned
by different reasoners — `rules.yaml` by `dre-symbolic`, `geometry.yaml`
by `dre-geometry` — so a `dre-model` that knew all of those types while
every reasoner needed the pack would be a dependency cycle. Each reasoner
parses its own section. A second benefit: `ModelHash` covers the raw bytes
that were on disk, so a parser upgrade cannot silently change stored
hashes.

---

# 7. Phase 1 — Deterministic Runtime Foundation

## Goal

Establish the execution environment and determinism guarantees.

## Build

Implement:

- deterministic collections,
- immutable IDs,
- canonical serialization,
- fixed-point numeric types,
- deterministic hashing,
- model versioning,
- execution context,
- runtime configuration.

## Requirements

Prohibit:

- random iteration order,
- uncontrolled floating point,
- wall-clock dependency,
- random number generation,
- mutable models during execution,
- nondeterministic thread result ordering.

## Deliverable

A runtime that can load an input object and produce a deterministic output hash.

## Acceptance Test

Run the same workload:

```text
100,000 times
```

and verify:

```text
unique output hashes = 1
```

---

# 8. Phase 2 — Canonical State Engine

## Goal

Create the universal representation of a reasoning problem.

## Primitive Types

Implement:

```text
Entity
Property
Value
Relationship
Observation
Evidence
Goal
Constraint
Action
```

## State Representation

Example:

```yaml
entities:
  - id: object-a
    type: generic-object

properties:
  object-a.temperature:
    value: 750
    status: KNOWN

  object-a.internal_state:
    value: null
    status: UNKNOWN
```

## Epistemic States

Support:

```text
KNOWN
UNKNOWN
INFERRED
CONFLICTING
STALE
UNOBSERVABLE
INVALID
NOT_APPLICABLE
```

## Deliverable

Canonical state schema plus deterministic state builder.

---

# 9. Phase 3 — Model Pack Format

## Goal

Allow reasoning domains to be defined without modifying the engine.

A model pack should contain:

```text
model.yaml
ontology.yaml
features.yaml
rules.yaml
geometry.yaml
causal.yaml
constraints.yaml
goals.yaml
tests/
```

## `ontology.yaml` — declared types and domains

Entity types, their properties, and the domain each property is defined
over:

```yaml
entities:
  machine:
    properties:
      temperature:
        type: integer
        min: 0
        max: 1000
      condition:
        type: symbol
        values: [degraded, healthy]
```

Absent `min`/`max` means unbounded and absent `values` means any symbol, so
declaring nothing constrains nothing.

Checked by `dre validate`, **and enforced during a run since 0.7.0** for
incoming observations. This paragraph used to say enforcement was a
hypothetical deserving its own decision; that decision was taken, by a domain.

`device-cascade` submitted a temperature of 9999 against a declared max of
200. It was admitted, disagreed with a sound reading, the subject resolved
`CONFLICTING`, the overheating rule could not fire — and the engine reached
the right diagnosis *because a sensor was lying*. Repairing the sensor broke
it. So a reading whose kind, domain or vocabulary is wrong is now stated as
rejected and excluded from resolution, and a subject whose every reading was
defective becomes `INVALID` rather than `UNKNOWN`: "we looked and what came
back was bad" is strictly more informative than "nobody looked".

The checks matter because their failures are silent. A condition on a
property no type declares evaluates to `Unknown`, so the rule quietly never
fires; a symbol outside its vocabulary compares equal to nothing, with the
same result. Both produce a run that looks entirely ordinary and has left a
step out.

Region identifiers are the symbols written to a geometry's `classify_into`
property, so the two declarations are checked against each other. That is
the one place they have to agree, and nothing else would notice them
drifting.

## Two literal grammars, at two typing levels

A design rule rather than an implementation quirk, and worth stating
because an independent author generalised the wrong way from the brief.

```text
Value position — the kind is ambiguous
    integer                    5
    boolean                    true
    symbol                     SERVICE_NOW
    fixed-point                {fixed: "44.0"}
    reference                  {ref: pump-1}

Fixed position — the kind is already known
    whole number               44
    decimal string             "44.0"
```

The tag exists to disambiguate, and only where there is ambiguity. In a
value position `44` could be an integer and `"44.0"` could be a symbol, so
fixed-point has to say so. Inside an arithmetic expression, an action's
cost or a constraint's penalty, the position is typed `Fixed` already and
the tag would be answering a question nobody asked.

`{fixed: ...}` in a `Fixed` position is **refused**, not accepted as a
synonym — with an error that gives the rule rather than just the rejection.
Two spellings for one concept would add no expressive power and would raise
a canonical-form question: which one should documentation, formatters and
generated packs emit?

**Falsifiable.** One author, a missing example, and a
recovery inside a minute is a documentation gap. Two or more independent
authors reaching for the tagged form *after* the brief carries the example
would be evidence that the distinction is semantically real and
ergonomically poor, and accepting it everywhere would then rest on evidence
rather than on taste.

## `features.yaml` — derived quantities

The section that makes the goal above true rather than nearly true.

A rule concludes a **literal**: `Inference` carries a `Value`, not a
computation. So a pack can say "if temperature exceeds 800 then
bearing_fault is true" and cannot say "efficiency is output over input" at
all. Any domain whose reasoning starts from a ratio, a margin, or a
difference from a set point needed an engine change to be expressible —
which is precisely what a pack format exists to avoid.

Features are arithmetic over properties, evaluated after evidence and
before rules, so a rule then reasons about a derived quantity as though it
had been measured:

```yaml
features:
  - id: F002
    subject: pump.efficiency
    expression:
      divide:
        - subject: pump.output_power
        - subject: pump.input_power
```

Operations: `add`, `subtract`, `multiply`, `divide`, `min`, `max`, `abs`,
`negate`, over `subject` references and `literal` constants, nested freely.
An optional `when:` guard carries the same conditions rules use.

### Four outcomes, not two

A derivation writes one of four epistemic statuses, and the distinctions
are the point:

| Outcome | Status | Means |
|---|---|---|
| a value | `INFERRED` | derived, and marked as derived rather than observed |
| an operand was not available | `UNKNOWN` | never measured, still unknown, or conflicting |
| operands present, operation undefined | `INVALID` | division by zero, or overflow |
| a guard did not hold | `NOT_APPLICABLE` | the model says the quantity is meaningless here |

Computing from a substituted default is the failure this prevents: once a
number is in state it is indistinguishable from a measured one, so a
default launders a guess into a fact. `INVALID` and `NOT_APPLICABLE` had
no producer in the engine before this section existed.

### Evaluation order

Features may read other features, and run to a fixpoint in identifier
order rather than declaration order, so a pack never has to be
topologically sorted by hand — requiring that would make the file's
ordering semantic, and orderings get edited by people thinking about
something else. A dependency cycle leaves its members `UNKNOWN` and
terminates rather than spinning.

Example:

```yaml
model:
  id: simple-diagnosis
  version: 0.1.0

entities:
  machine:
    properties:
      temperature:
        type: integer
        min: 0
        max: 1000

      vibration:
        type: integer
        min: 0
        max: 1000
```

## Deliverable

Model-pack schema and validator.

---

# 10. Phase 4 — Symbolic Reasoning

## Goal

Implement deterministic logical inference.

Example rule:

```yaml
rule:
  id: R001

  if:
    - temperature > 800
    - vibration > 700

  infer:
    bearing_fault: true

  priority: 100
```

## Required Operators

Implement:

```text
EQ
NE
GT
GTE
LT
LTE
EXISTS
KNOWN
UNKNOWN
```

`AND`, `OR` and `NOT` were on this list and are not operators. They are
*condition* forms — `all:`, `any:` and `not:` — which is where they have
to be, because they combine conditions rather than compare a subject
against an operand. Having them in both places meant `Operator::And`
existed, parsed, and evaluated to `Unknown` in every context, so a
condition using it could never fire.

`IN` was on this list too, and needs a set-valued operand. `Value` has no
set form and adding one is not MVP scope. Membership is expressible today
as `any:` over `EQ` conditions.

Both were removed from `Operator` on 2026-08-15 rather than left declared
and unimplemented — see `decisions.md`. This section is the specification
and the specification was wrong.

## Rule Ordering

Use:

```text
priority ASC
specificity DESC
rule_id ASC
```

## Deliverable

Forward-chaining symbolic inference engine.

---

# 11. Phase 5 — Evidence Reasoning

## Goal

Allow the system to reason when information is incomplete.

Evidence should contain:

```yaml
evidence:
  id: E001
  subject: object-a
  predicate: temperature_high
  value: true
  source: sensor-a
  reliability: 900
```

## MVP Evidence Logic

Support:

- evidence provenance,
- source reliability,
- evidence freshness,
- conflicting evidence,
- independent evidence count,
- inferred values.

Do not implement complex probabilistic inference initially.

Use deterministic thresholds and precedence rules.

Example:

```text
IF two independent high-reliability sources agree
THEN inferred state = KNOWN
```

---

# 12. Phase 6 — Geometric Reasoner

## Goal

Implement deterministic state-space reasoning.

A model defines dimensions:

```yaml
space:
  dimensions:
    - temperature
    - vibration
    - pressure
```

Current state:

\[
x = [750, 830, 410]
\]

## Initial Operators

Implement:

```text
DISTANCE
NEAREST_REGION
INSIDE_REGION
BOUNDARY_DISTANCE
DIRECTION
```

All five exist in `dre-geometry`. Four are reachable from a model pack:
`classify_into` writes the nearest region, `inside_region_into` writes
containment, `boundary_distance_into` writes the distance to the nearest
edge, and distance is what the ranking is computed from.

`DIRECTION` is **implemented and deliberately unreachable**. It returns one
ordering per dimension and no property kind holds a vector, so exposing it
needs a naming convention for per-axis properties — a format decision
rather than a gap to fill in passing. Tested as a library function.

Worth recording how that was found, since the list above says "implement"
and all five were implemented: three of the five had **zero call sites
anywhere in the workspace**, tests included, until 2026-08-15. Implemented
is not the same as reachable, and this section could not tell the
difference.

Use fixed-point integer mathematics.

Start with:

- rectangular regions,
- centroids,
- weighted squared distance.

Do not begin with manifolds or learned embeddings.

## Example

```yaml
region:
  id: FAILURE_A
  centroid:
    temperature: 900
    vibration: 850
    pressure: 400
```

---

# 13. Phase 7 — Causal Reasoner

## Goal

Represent actions as deterministic state transformations.

Example:

```yaml
action:
  id: COOL_OBJECT

  preconditions:
    - temperature > 700

  effects:
    temperature:
      add: -200
```

Conceptually:

\[
S' = T_a(S)
\]

## MVP Capabilities

Support:

- deterministic preconditions,
- state transformations,
- action cost,
- reversibility,
- constraints.

Avoid probabilistic effects initially.

---

# 14. Phase 8 — Planner

## Goal

Search possible action sequences.

Start with:

- breadth-first search,
- Dijkstra,
- A*.

The planner must use deterministic:

- node expansion,
- candidate ordering,
- scoring,
- tie-breaking.

## A\* admissibility is checked, not assumed

A\* returns optimal plans only while its heuristic never overestimates.
The Manhattan estimate here is admissible exactly when no action moves the
goal dimensions further, in total, than it charges:

```text
for each action:  sum over goal dimensions |delta| <= cost
```

The **sum** matters, not the maximum: an action moving `x` and `y` by one
each at unit cost buys two units of Manhattan distance for one unit of
cost. An `add` is compared against cost; a `set` on a goal dimension can
close any distance at all, so no finite cost makes it safe and it is
rejected outright; a `clear` returns the property to `UNKNOWN`, which the
heuristic skips, so it can only lower the estimate and is always safe.

`dre_planner::check_admissible` runs before any A\* search and refuses the
model rather than searching it. This is checked rather than documented
because the failure is invisible by construction: an inadmissible heuristic
does not crash or warn, it returns a plan that is valid, reaches the goal,
and simply is not the cheapest — and "optimal" is not a property anything
downstream can read off a plan. Dijkstra has no heuristic and is
unaffected, so an inadmissible model is still perfectly searchable.

## Which strategy runs is the experiment's choice, and part of the engine

All three were implemented from the start, and for most of the MVP only one
of them was ever reachable: the lab runner named `Dijkstra` at the call
site, so every stored record was produced by it and A\* ran only in unit
tests. An experiment now declares `strategy`, defaulting to `dijkstra` so
that no existing record changed meaning.

`Strategy` lives in `dre_core::config::RuntimeConfig` beside the ceilings
and the proof mode, which puts it in the `EngineHash`. It is there for a
reason worth stating, because the obvious reason does not apply: the
strategy does **not** change the decision. Measured on `grid-40-mid`, A\*
and Dijkstra return the same 38-step plan.

What differs is the proof. A\* reaches that plan having expanded 383 states
against Dijkstra's 727, so the two record different rejected candidates and
their `ProofHash`es differ. A replay verifies five hashes, and a knob that
moves any of them belongs in the engine identity — otherwise two
legitimately different searches report as a determinism failure. See
`docs/determinism.md`.

The measurement also settles what the far corner is worth: at (39, 39)
every cell lies on some shortest path, `g + h` is constant, and the two
strategies expand the *identical* 1518 states. The heuristic is not weak
there, it is inert, which is why the mid-grid pack exists.

## Search Limits

Define:

```text
max_depth
max_states
max_actions
max_memory
```

Do not allow unbounded search.

---

# 15. Phase 9 — Goal Model

A model pack should define desired states.

Example:

```yaml
goal:
  id: SAFE_STATE

  conditions:
    - temperature < 500
    - vibration < 400
```

The engine searches for:

\[
S_{current} \rightarrow S_{goal}
\]

subject to constraints.

---

# 16. Phase 10 — Proof Engine

Every decision must be explainable.

Example proof:

```text
INPUT S001
   │
   ├── temperature = 850
   ├── vibration = 810
   │
   ▼
RULE R013
   │
   ▼
bearing_fault = INFERRED
   │
   ▼
REGION FAILURE_A
   │
   ▼
ACTION A004 candidate
   │
   ▼
CONSTRAINT C002 passed
   │
   ▼
GOAL distance reduced
   │
   ▼
ACTION A004 selected
```

Store:

- inputs,
- feature derivations, including the ones that failed,
- rule activations,
- inferences,
- geometry calculations,
- candidates,
- rejected candidates,
- constraints,
- scores,
- tie-breaks,
- final result.

## The edges are the point

A node also records the nodes it drew on. Without those the proof is a
list of events in order, and the ordering suggests a causation it does not
actually record — "what happened" is answered, "what did *this* step rest
on" is not, and the second question is the one asked when a result is
disputed.

Edges are built by `dre_proof::Trace`, which is the graph plus a note of
which node most recently established each subject. A rule evaluation links
to whatever established the properties its conditions read; an inference
links to the evaluation that drew it; a planner candidate links to the step
before it and to the preconditions it required. So a conclusion traces past
the rule, to the measurement, to the evidence resolution, and on to the
readings that resolution overruled:

```text
#2  INPUT      machine.temperature
#3  REJECTED   E001 — overruled resolving machine.temperature  <- #2
#7  RULE       R001 (fired)                                    <- #2 #6
#8  INFER      machine.bearing_fault                           <- #7
#11 RULE       R003 (fired)                                    <- #8
#12 INFER      machine.condition                               <- #11
```

`dre run --why` prints only that chain; `--proof` prints the whole trace.

Two rules keep the edges honest. A subject nothing established contributes
no edge at all, because inventing one would assert a dependency the run
never had. And the provenance map is deliberately **not** part of
`ProofGraph`: the graph is what gets hashed, serialised and compared, and
it must not carry build-time scaffolding that vanishes on a round trip
while equality claims the two are the same.

The edges are covered by `ProofHash`. Two runs that agree on every step but
disagree on what those steps depended on are not the same proof.

---

# 17. Phase 11 — Replay

Every experiment should produce:

```text
InputHash
ModelHash
EngineHash
DecisionHash
ProofHash
```

Example:

```text
Experiment: EXP-000421

Input:
0x8D12...

Model:
0x772A...

Engine:
0x192F...

Decision:
0x6CC2...

Proof:
0x918B...
```

Replay must verify:

```text
DecisionHash == OriginalDecisionHash
ProofHash == OriginalProofHash
```

## What each hash has to cover

`InputHash` is taken over the state as given, before feature derivation, so
it describes the question rather than any of the engine's own work.

`EngineHash` covers the version *and the limits*. A run under raised
ceilings can legitimately reach a different answer, having explored states
the default engine would have refused, so it is a different engine and says
so. An experiment may declare its own limits; replay against a record made
under different ones reports NOT COMPARABLE rather than a mismatch, which
is the correct reading.

`DecisionHash` covers the final state **and the plan**. That second half
was missing until the planner was run at a scale where two problems could
obviously differ, and the omission is worth recording because of how
invisible it was. The planner works on a clone of the state and returns a
plan without applying it, so the state after planning is the state before
it — and hashing the state alone captured no part of the plan at all.

A 5-by-5 grid solved in eight moves and a 40-by-40 grid solved in
seventy-eight, from different packs with different obstacles, therefore
produced the *same* `DecisionHash`, because both start with an agent at the
origin.

Replay still caught a changed route, because it compares the proof too.
But it caught it as `ProofMismatch`, whose stated meaning is "same
decision, different reasoning" — which for a plan is precisely backwards.
A different route is a different decision, and the hash named
`DecisionHash` must be able to say so.

---

# 18. Phase 12 — DRE Lab

Build a CLI first.

Example:

```bash
dre run experiments/logic-001.yaml

dre replay EXP-000421

dre benchmark model-packs/diagnosis

dre validate model-packs/navigation
```

Output:

```text
Experiment: logic-001

Result:
C = TRUE

Reasoning steps:
7

Rules evaluated:
12

Rules fired:
3

States expanded:
0

Execution:
0.84 ms

Decision hash:
...

Determinism:
PASS
```

A web UI can come later.

---

# 19. Initial Model Packs

Use very different problems to prove the engine is genuinely
domain-agnostic. Three were planned; a fourth was added when derived
features turned out to be a capability no existing pack needed and no pack
could have supplied for itself.

## Model Pack A — Logic

Example:

```text
A → B
B → C
A = TRUE

Determine C.
```

Tests symbolic reasoning.

---

## Model Pack B — Mechanical Diagnosis

Example observations:

```text
temperature = high
vibration = high
pressure = normal
internal condition = unknown
```

Tests:

- evidence,
- inference,
- geometric classification,
- missing observations.

---

## Model Pack C — Grid Navigation

Example:

```text
start = [1,1]
goal = [20,20]
obstacles = [...]
hazards = [...]
```

Tests:

- geometry,
- constraints,
- planning,
- deterministic path selection.

---

## Model Pack D — Process Performance

Example measurements:

```text
input power  = 1000 W
output power =  820 W
rated output =  900 W
```

Nothing here is a conclusion. Efficiency, load and loss all have to be
computed before a rule can be evaluated, and both geometric axes are
derived rather than observed.

Tests:

- derived features,
- features that read other features,
- rules over fixed-point values,
- classification on computed axes,
- the difference between an unmeasured quantity and an undefined one.

The second experiment on this pack measures a stopped pump: zero watts in,
zero watts out. Efficiency is `0/0`, which is not zero — it is undefined,
and the pack exists partly to hold the engine to saying so. A model
computing `0/0 = 0` would classify an idle pump as catastrophically
inefficient and recommend servicing it, and every step of that
recommendation would look reasonable.

These four domains exercise significantly different engine behavior.

---

# 20. MVP Benchmark Suite

Track:

| Metric | Purpose | Where it comes from |
|---|---|---|
| Correctness | Did it produce the expected answer? | Declared expectations in the experiment |
| Determinism | Were all repeated results identical? | Unique decision and proof hash counts |
| Reasoning steps | How much inference occurred? | `Counters::reasoning_steps` |
| Rules evaluated | Symbolic workload | `Counters::rules_evaluated` |
| States expanded | Planner workload | `Counters::states_expanded` |
| Peak memory | Resource use | `VmHWM` from `/proc/self/status` |
| Execution time | Runtime performance | Harness clock, never the engine's |
| Proof size | Explainability overhead | `ProofGraph::len` |
| Active dimensions | Geometric complexity | `Counters::active_dimensions` |
| Unknown resolution rate | Evidence handling | `Counters::unknown_resolution_permille` |

Reported by `dre run` and `dre benchmark`. Three of them needed a decision
about *what exactly* is being counted, and the answers are load-bearing:

- **Peak memory** is the process high-water mark, harness included, read
  from the kernel rather than from a counting allocator. Implementing
  `GlobalAlloc` would need `unsafe`, which is `forbid` workspace-wide and
  deliberately cannot be lifted locally. On a platform that does not
  publish the figure it reports as unavailable, never as zero.
- **Active dimensions** counts the declared geometric dimensions the
  subject could actually be read on — whether or not classification then
  went ahead. Two of three readable is the interesting case, and reporting
  zero because the classifier declined would hide it.
- **Unknown resolution rate** is measured against the state *as authored*,
  before evidence resolution runs, so resolution counts toward it.
  `UNOBSERVABLE` is excluded from the denominator: it is a deliberate
  statement, not a gap, and counting it would make the rate look worse the
  more honest the pack was. An experiment with no unknowns reports "none
  started unknown" rather than 0%.

The rate is carried as parts per thousand in integer arithmetic. Floating
point is denied workspace-wide and there is no reason for the reporting
path to be the one place it appears.

---

# 21. Determinism Conformance Suite

Create tests specifically intended to break determinism.

Test:

| Attack | How it is covered |
|---|---|
| Randomised input ordering | 100,000 seeds, each varying declaration and insertion order |
| Randomised rule file ordering | 500 permutations of the diagnosis pack |
| Different CPU counts | Thread-count invariance, plus a source scan; see below |
| Parallel execution | 64 concurrent runs with experiments interleaved |
| Repeated execution | 100,000 iterations in one process |
| Process restart | A fresh process against a committed replay record |
| Machine restart | Documented procedure in `docs/verification.md` |
| Serialization round-trip | State through JSON, compared canonically |

Required:

```text
same canonical input
→
same canonical output
```

Every attack has a companion test proving it is not vacuous — that the
shuffle really reorders, that the scan really matches something. A test
that cannot fail is worse than no test, because the checkmark reads as
coverage.

## Two attacks that needed reframing

**Different CPU counts** cannot be varied from inside a test process, and
shelling out to `taskset` would test the platform more than the engine. It
is covered from both ends instead: the same work is spread across 1, 2, 3,
8 and 16 threads, and a source scan proves no engine crate can even *ask*
how many CPUs there are — no `available_parallelism`, no `num_cpus`, no
`rayon`, no `std::thread`, no `std::env`. A result cannot depend on a
number nothing reads. Running under `taskset -c 0` stays a manual check.

**Parallel execution** looked vacuous while nothing in the engine is
parallel, and that reading is wrong. Single-threaded internals say nothing
about several runs existing at once: a `OnceLock` cache shared between
runs, a thread-local memo, or an iteration order derived from an allocator
address would each turn "same input, same decision" into "same input, same
decision *if nothing else was running*". Since the project's whole claim is
about reproducibility, that qualifier has to be disproved rather than
assumed.

**Machine restart** needs infrastructure `cargo test` does not have — it
cannot reboot its own host. The committed replay records make it a real
two-stage procedure rather than an unverifiable claim, because the baseline
outlives the machine.

## Scope limit — single machine

Cross-machine determinism is **out of scope**. Development happens on one
VM and there is no second host with a different CPU to test against.

The verified claim is therefore narrower than the headline invariant:

\[
SameInput + SameModel + SameEngine + SameMachine = SameDecision
\]

The prohibitions in section 7 are still written to make the stronger,
machine-independent property likely — banning floating point and
normalising line endings are exactly the measures that would let a result
survive a change of host. But likely is not verified. A determinism bug
that only appears on different microarchitecture will not be caught here,
and the most plausible candidates are the ones the fixed-point rule exists
to prevent: an `f64` slipping into an engine path, or a dependency using
one internally.

Revisit this if the engine is ever run anywhere but the development VM.

---

# 22. MVP Constraints

Keep the first version intentionally small.

Recommended limits:

```text
Entities:           <= 10,000
Features:           <= 10,000
Rules:              <= 10,000
Geometric dimensions <= 256
Regions:            <= 1,000
Actions:            <= 1,000
Search depth:       <= 20
Expanded states:    <= 100,000
```

These are engineering limits, not theoretical limits.

Remove them later as profiling justifies it.

---

# 23. What Not to Build Yet

Defer:

- neural networks,
- LLM runtime decisioning,
- learned embeddings,
- probabilistic planning,
- distributed reasoning,
- billion-node graphs,
- graph databases,
- GPU acceleration,
- self-modifying rules,
- automatic production learning,
- generalized natural-language understanding.

These can obscure whether the underlying reasoning architecture actually works.

---

# 24. MVP Development Sequence

Recommended build order:

```text
1. Runtime determinism
        ↓
2. Canonical state
        ↓
3. Model pack loader
        ↓
4. Symbolic rules
        ↓
5. Evidence model
        ↓
6. Proof generation
        ↓
7. Replay
        ↓
8. Geometry
        ↓
9. Causal actions
        ↓
10. Planner
        ↓
11. Benchmark framework
        ↓
12. Three domain packs
```

Do not start with geometric reasoning.

Prove the state, evidence, determinism, and replay foundations first.

---

# 25. Definition of MVP Complete

The MVP is complete when all of the following work:

### Experiment 1

Symbolic logic problem solved correctly.

### Experiment 2

Diagnostic problem solved correctly despite missing information.

### Experiment 3

Navigation problem produces the correct deterministic path.

### Reproducibility

Each experiment can be executed:

```text
10,000 times
```

with:

```text
1 unique DecisionHash
1 unique ProofHash
```

### Replay

Any prior experiment can be reconstructed exactly.

### Domain Isolation

Adding a new reasoning domain requires:

```text
new model pack
```

and does **not** require modifying the DRE runtime.

---

# 26. Post-MVP Research Tracks

Once the MVP works, several experiments become worthwhile.

## Higher-Dimensional Geometry

Investigate:

- polytopes,
- manifolds,
- topology,
- curvature,
- trajectory geometry.

## Factorized Reasoning

Divide very high-dimensional states into local subspaces.

## Hierarchical Planning

Represent:

```text
goal
→ strategy
→ subgoal
→ action
```

## Counterfactual Reasoning — built 2026-08-15

Ask:

```text
What would happen if condition X were different?
```

`dre counterfactual <experiment> --set entity.property=value` runs the
experiment twice and reports whether the decision moved, which rules
started or stopped firing, and how the plan changed. `--unset` asks the
reverse: what would it have decided *without* an observation it has.

**It adds no reasoning, deliberately.** Both halves are ordinary runs, so
nothing here can conclude anything `dre run` could not, and a counterfactual
inherits determinism and replayability rather than needing its own. A
counterfactual engine that reasoned over hypothetical states would be a
second engine to keep honest.

The five hashes already described the shape: a counterfactual pair shares a
`ModelHash` and an `EngineHash` and differs in `InputHash`, so any movement
in the `DecisionHash` is attributable to the perturbation and to nothing
else. That is asserted rather than assumed.

The claim that makes it trustworthy is checkable, because the repository
already had the pair by hand: `haulage-001` perturbed into `haulage-002`
reproduces `haulage-002`'s committed decision hash byte for byte. If that
ever stops holding, the counterfactual is computing something the engine
would not.

Next along this track is **active evidence gathering** below, which is this
with a ranking on top: `haulage-001` sends a truck the long way because
nobody rated a bridge, and the question it cannot yet ask is which single
measurement would most improve the decision. **Explanation ranking**, third
below, then turns that into "which measurement would settle the diagnosis" —
a sharper question, and a different one.

## Active Evidence Gathering — built 2026-08-15

Have the engine choose which observation to acquire next.

`dre survey <experiment>` runs a counterfactual for every value the
ontology admits for every unknown, and ranks the unknowns by what an answer
could change. On `haulage-001` it reports that rating the north bridge can
only help — the plan drops from 7 to 2, or stays at 7 — while rating the
ford can only hurt, since the best case leaves the plan alone and the worst
reveals the ford unusable at 9.

Like counterfactuals, it adds no reasoning: every entry is an ordinary run.

Three scoping decisions, each of which cost something to learn:

**The ranking is not the `DecisionHash`.** That hash covers the state and
the state contains the observation just injected, so it moves whenever the
input does — every unknown looked decisive. The comparison is over what the
engine *derived*: the plan, and the properties it inferred.

**Candidate values come from the ontology**, the first thing in the
repository to read that file for something other than validation. A closed
`values:` vocabulary is what makes a symbol surveyable; an open one is
unbounded in the same way an integer is.

**Numerics are probed at the thresholds the pack declares.** Sampling their
bounds would find threshold flips at the extremes and miss every one between
— which was the reason numerics were declined outright, and it was half an
argument. The values between the bounds that matter are the operands the pack
already wrote down, so each declared threshold is probed with its two
neighbours: `GT` and `GTE` differ exactly there. A numeric compared against
nothing is still declined by name with the reason given.

**A threshold on a derived value carries back to the measurement that feeds
it**, across `add`, `subtract` and `negate` only — exact on the raw `i64`, so
`derived = ±reading + offset` holds without rounding. `multiply`, `divide`,
`min`, `max` and `abs` are each declined by name. Soundness rests on the run
rather than on the inversion: every carried probe is executed and the
threshold re-evaluated against the resulting state, and a threshold no probe
put on both sides of is withheld.

**Properties the pack derives are not observations.** Asking what the engine
would conclude if told its own conclusion is not a question, and
`fault-network` — which declares every conclusion UNKNOWN so the resolution
metric has a denominator — produced 240 such entries before this was
handled. Which subjects are derived is read from the pack's own feature
subjects, inference subjects and geometry outputs, so no format change was
needed.

### What is missing is the route, not observability

Observability in this engine means **whether there is a defined way to
obtain or inspect a fact, not whether that fact currently holds a value**,
and `EpistemicStatus` already carries it: `UNKNOWN` is "not observed, but
observable in principle", `UNOBSERVABLE` is "no amount of evidence
gathering will resolve it". The survey's filter on `UNKNOWN` is therefore
the observability question already answered by the model, which is why
`diagnosis-001` correctly offers nothing — its only gap is declared
unobservable.

What the format does not record is **which route** a fact has to a value.
That is a different axis, and there are at least four:

| Route | Example | What a survey should say |
|---|---|---|
| Derived | `comp-3-4.impaired`, from rules | Nothing — the engine obtains it itself |
| Acquired | `north_bridge.rating_ok`, from evidence or declaration | Go and find out |
| Actuated | `shipment.inspection_complete`, from an action's effect | Possibly "do it" rather than "measure it" |
| None | anything `UNOBSERVABLE` | Nothing, and say why |

A derived property is fully observable in the sense above — the engine has
a defined way to obtain it, by computing it — and is still useless to a
survey, because the route is internal. Conflating the two is what produced
240 entries on `fault-network` before `derived_subjects` reconstructed the
distinction from the pack's own declarations.

The actuated row is the one with no good answer yet.
`shipment.inspection_complete` is written by `COMPLETE_INSPECTION`, so it
is both something you could check and something the plan could bring about,
and the survey silently picks the first reading. Verified rather than
assumed: leaving it `UNKNOWN` in a copy of `cold-chain-release-001` puts it
in the survey's output.

Also absent, and arguably the larger gap: **what a measurement costs.** The
survey can say rating the bridge is worth up to five units of detour. It
cannot know whether finding that out is a glance at a plaque or a load
survey with a three-week lead time, and no field exists to say so.

Worth a format discussion before either becomes a feature — and worth
waiting for a domain that needs it, on the `requires_engine` precedent: a
field added to ten packs at once, set the same way in all of them, is a
field nothing exercises.

## Explanation Ranking — built 2026-08-16, engine 0.11.0

Ask:

```text
Which declared account best explains everything I observe?
```

The third post-MVP track, and the first that **adds reasoning** rather than
borrowing it. Counterfactuals and surveys are ordinary runs with a
comparison on top, so nothing they conclude is beyond `dre run`. This is a
question the engine could not previously be asked at all.

### The domain demanded it

`device-cascade`. A power rail fails, four subsystems on it complain, and
the device is also hot. Two rules fire — a low rail with degraded sensors
concludes `POWER`, a high temperature concludes `THERMAL` — both correct in
isolation, both written by any reasonable modeller. The property merges to
`CONFLICTING`.

That is the right answer to the wrong question. "Do my rules agree?" is not
"which account of this device is better?", and no arrangement of rules
answers the second, because a rule fires or it does not. Weighing two
conclusions against each other needs a layer that can hold both at once.

### A separate layer, not a planner feature

```text
observations
      ↓
declared hypotheses
      ↓
mechanisms
      ↓
deterministic ranking
      ↓
planner / survey
```

`dre-explain` does not depend on `dre-planner` and `dre-planner` does not
depend on it. The separation is structural rather than conventional, because
the two questions have different failure modes: an explanation can be wrong
while every action in the plan is valid, and folding them together would
leave nowhere to record which one went wrong.

### The format

A hypothesis has mechanisms; a mechanism explains declared observations.
Observations are declared **once and shared**, which is what makes the
comparison fair — if each hypothesis brought its own list, one could improve
its standing by declaring fewer things to explain, and the ranking would
measure ambition rather than fit.

Two levels, and no arbitrary nesting. That is a deliberate stop on the rule
this project runs on: no new reasoning abstraction without a failing domain
that requires it. A deeper tree is easy to add later and impossible to
remove once packs declare it.

### The score is not a count of symptoms

The tempting scheme — `POWER` explains five things, `THERMAL` explains one —
reaches the right answer here and is the wrong rule. Four sensors on one
degraded bus are one thing happening, reported four times.

**A mechanism is an independence group.** This reuses what the evidence
layer already does rather than inventing a second idea: `dre-evidence`
resolves by independence group precisely so three readings off one bus
cannot outvote one reading off another. Observations under one mechanism
corroborate at a discount; observations under different mechanisms count in
full. So the *shape* of the explanation decides the score rather than the
length of the symptom list.

Five components, all fixed-point, with a total tie-break order:

| Component | What it charges for |
|---|---|
| Coverage | Explaining live observations that **want explaining**, corroboration discounted |
| Contradictions | Predicting something the world denies |
| Unexplained | Live observations the account ignores |
| Complexity | Mechanisms declared — Occam, weighted weakly |
| Assumptions | Mechanisms the account needs and nothing supports |

The weights are engine constants, covered by `ENGINE_VERSION` and therefore
by `EngineHash`. A pack-tunable weight would let a modeller who disliked a
ranking adjust it until the intended hypothesis won.

### A symptom and a negative finding are different — 0.19.0

An observation may declare `needs_explaining: false`. It then earns no
coverage and incurs no unexplained penalty, while still **contradicting** a
hypothesis that predicted the opposite.

The distinction was missing and it produced a wrong answer. A *symptom* is
something wrong that wants accounting for; a *negative finding* is something
confirmed healthy, whose entire diagnostic value is that it refutes. With one
list for both, a mechanism that accounted for `dns_ok`, `network_ok` and
`tls_ok` earned 1.75 of coverage while the rival account was charged 1.00 for
leaving those same three "unexplained" — so an account was **rewarded for
explaining non-problems and punished for not explaining why the working parts
work**, and the wrong hypothesis won.

Found by the fifth independently authored pack, in the one experiment where
its author's prediction and the engine disagreed. The author was right.

### What it refuses to conclude

The ordering is total, so there is always a first element — but a *winner*
is declared only when the leader has a mechanism the state supports **and**
strictly outscores the runner-up.

A decline is not silence. The target is still declared `UNKNOWN`, and the
proof records which of the two reasons applied — nothing supports the leader,
or the top two tie. Absence would make a pack that weighed its accounts
indistinguishable from one that declared none, and two independent authors
asserted exactly `status: UNKNOWN` there, having predicted the tie correctly
and failed only on the property being missing. A lead that exists only because one
identifier sorts before another is a stable report, not a finding, and
putting it into state would make an arbitrary answer indistinguishable from
a real one.

### The known lever

The observation set is shared, so adding a **symptom** helps whoever explains
it and charges everybody else in full. Since 0.19.0 that is true only of
observations that need explaining: a negative finding earns nobody coverage
and costs nobody a penalty, so the lever exists over symptoms alone. Correlated symptoms are
worth about 40% of independent ones rather than nothing, so a large enough
group of them can move a ranking.

That asymmetry has a defence and the defence is only half sound.
Correlation is declared *inside* a hypothesis, so an account offering no
grouping for six live facts pays for six — but the format is what chose to
hide correlation inside hypotheses, so this follows from a design decision
rather than from the world. It is also a way to tilt a ranking by writing
the pack a particular way.

The first fix recorded here was to pool declared correlations across
hypotheses by union, and an independent review showed that is gameable in a
new direction: a hypothesis could group unrelated observations purely to cut
every rival's unexplained penalty. Kept as a rejected alternative, because
the reason it fails is the useful part.

The shape that works is correlation groups **first-class in the shared
observation set**, with mechanisms referring to observations or to groups —
shared questions should have shared independence structure. Not built: the
case that exposes it was constructed by hand to break the rule, not found
while modelling anything. Until it is, this layer is **experimental rather
than stable semantics**.

### Discriminating observations follow from it

With hypotheses declared, `dre survey` can ask which measurement best
separates the accounts still in contention, rather than only which one could
change the decision. Those are different questions: an observation can
change the decision without discriminating, and one that leaves the plan
untouched can still settle which account is right.

The standing is held to the hypothesis leading as written, so the sign is
meaningful — an answer that takes the leader negative has overturned the
diagnosis. This is deterministic experimental design, and it is where the
engine starts behaving like an investigator rather than a rule evaluator.

## Declared Traversal — built 2026-08-16, engine 0.18.0, extended 0.20.0

Ask:

```text
Is anything this unit mounts unavailable?
```

**The most-reported limitation in the project's history.** Four
independently authored packs — OPC UA, Windows administration, network
forwarding, Linux systemd — each collapsed a graph into a single entity
because a rule referencing two entity types is refused. Every one of them
reported it. `fault-network`, the only in-repo pack whose
subject is a dependency graph, is 450 rules generated by a script, one per
edge, because the format could not express the propagation directly.

### It does not lift the refusal

A rule still binds **exactly one instance of exactly one type**. What changed
is what the bound instance may *read*: it can follow a declared relationship
and read a property on the entities it arrives at.

The second type is **reached, never bound**. The author never writes the
target type name — it comes from the relationship declaration — so
`bindings_for` still sees one type and a genuine two-type rule is still
refused with the same error. The standing quote survives word for word:
choosing cross-product or pairwise silently *"would make an answer depend on
a decision nobody wrote down."*

```yaml
- reach:
    from: unit           # instance, or a type with exactly one instance
    path:
      - edge: mounts     # a declared relationship type
        direction: out   # or `in`
    read: available
  collapse: every        # mandatory, no default
  operator: EQ
  operand: false
```

### `collapse` is the whole design

A traversal has the same problem a join does — several targets, one truth
needed — and the resolution is that the author writes the decision down.
There is no spelling of a reach that omits it.

| | Holds when |
|---|---|
| `the` | Exactly one entity is reached and satisfies the test. Several is an **error** |
| `every` | Every reached entity satisfies it. Vacuously true over none |
| `some` | At least one does |
| `{at_least: N}` | At least N do, counted three-valued |

There is no `none`, because `not:` around a `some` gives it with correct
Kleene negation, and one spelling is better than two. That decision has since
been explicitly endorsed by an outside author.

`at_least` arrived in 0.20.0 from the first author to use traversal against a
real graph: *"topology often cares about 'attached to at least two
networks'."* It counts targets that **satisfy the test**, not targets
reached — how many mounts a unit has is a property of the topology rather
than a truth about it.

### Two refusals, and the first is load-bearing

**An unresolved anchor is an error, never an empty set.** `every` over an
empty set is true, so an anchor that silently resolved to nothing would make
every universal traversal hold, every rule containing one fire, and the proof
say so in well-formed prose. "Resolved and has no edges of this type" and
"did not resolve" are different results and must not share a representation.

**`the` over several targets is refused, not picked.** `the` asserts the
traversal is functional; where it is not, the pack has said something false
about its own topology. `targets_per_source: one` makes that claim
enforceable in the state rather than in prose.

Carrying those two is why `evaluate` became fallible. Degrading either to
`UNKNOWN` would be the engine guessing precisely where it said it would
refuse.

### Topology is input, not model

An experiment declares which edges exist, for the same reason it declares
evidence: the pack says what kinds of thing exist and how they may relate,
the experiment says which ones actually do. Two sites with the same equipment
are two questions asked of one domain.

That made `RelationshipType`, `StateBuilder::relate` and two `StateError`
variants live for the first time — declared, hashed into every `ModelHash`,
and read by nothing since the ontology was written.

### What it will not do

It **reads**; it does not conclude on what it reached. A rule reaches from a
unit and concludes about that unit. To conclude about the mount, write the
rule from the mount's point of view and reach back with `direction: in`.

That limit is deliberate and was the losing argument in the design review:
one rejected design could conclude on both endpoints, and paid for it with an
undeclared quantifier. The first outside author to use traversal hit the
limit and did not find the workaround, which is a defect in the brief rather
than in the format.

### Transitive closure falls out, and nothing uses it

A rule anchored on a type, reaching one hop, reading a property and inferring
**that same property** on its anchor, computes a least fixed point —
inferences apply immediately and a binding that has not fired is re-evaluated
every round. Verified rather than assumed
(`one_reach_rule_propagates_transitively`).

So `fault-network`'s 450 generated rules are replaceable by a handful, and no
pack does it yet. Built, hashed and unexercised is this repository's
most-repeated defect shape, and it now applies to something days old.

### What is still refused

**Dyadic recursion.** `Inference` writes a property, never an edge, so a
derived *relation* cannot exist — only a marker property per target of
interest. That is the real content of the "relational layer" proposals and it
remains the boundary between traversal and a join.

One real graph domain has now asked for it and it is still refused, for a
reason worth keeping: that domain **succeeded**. All three of its experiments
passed with exact predictions. The rule is no new abstraction without a
*failing* domain, and a succeeding domain that found the encoding tedious is
answered by documentation, not by a new logic.

## Learned Model Compilation

Use machine learning offline to discover:

- regions,
- relationships,
- candidate rules.

Compile these into deterministic runtime artifacts.

## LLM Interface

Use an LLM only to:

```text
Natural language
      ↓
candidate structured problem
      ↓
schema validation
      ↓
DRE
```

The DRE remains authoritative.

---

## The order these are worth doing in

Recorded because the tracks above are not independent, and doing them out of
order builds machinery nothing yet needs:

```text
1. Keep exercising independent domains
        ↓
2. Declared explanation / hypothesis ranking      — done, 0.11.0
        ↓
3. Use survey to identify discriminating observations  — done, 0.11.0
        ↓
4. Declared traversal over relationships                — done, 0.18.0-0.20.0
        ↓
5. Add acquisition cost only when an experiment requires it
        ↓
6. Eventually add an external execution loop
        ↓
7. Explicitly compare predicted effects with observed outcomes
```

Steps 5 through 7 are described in `docs/roadmap.md` under "The direction:
observe, reason, act", including why `Action.reversible` becomes
load-bearing at step 6 and why step 7 is a change to the epistemic core
rather than an addition beside it.

Step 1 stays first and stays permanent. Every capability in this section
exists because a domain demanded it, and the domains that demanded them were
not the ones anybody predicted.

---

# 27. Core MVP Principle

The MVP should test one central hypothesis:

> **Can complex reasoning emerge from deterministic composition of state, evidence, geometry, causality, constraints, and search without requiring a stochastic model to make the final decision?**

If the answer is yes, the architecture can then be scaled and experimentally extended.

The first goal should therefore not be scale.

The first goal should be **proof that the reasoning substrate works**.