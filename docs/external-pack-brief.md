# Brief: an independently authored model pack

**For an author who has not worked on the DRE engine.**

You are being asked to write a reasoning domain for an engine you did not
build, using only this document. That constraint is the experiment. Please
read the whole brief before starting — particularly [What to record](#5-what-to-record),
because the notes you keep are the deliverable that matters most.

> **Revision 2.** A previous author worked from revision 1. This version
> spells out several engine behaviours they had to reverse-engineer, and
> **removes the list of known limitations that revision 1 carried**, because
> having it in view before drafting told that author where the walls were.
>
> Nothing about what they found is repeated here, deliberately. If you hit
> something they also hit, that is **replication**, and two authors
> independently running into the same wall is stronger evidence than one.
> Do not assume anything you struggle with is already known — write it down.

---

## 1. What is being tested

DRE is a deterministic reasoning engine. Its central architectural claim
is:

> Adding a reasoning domain requires a new **model pack** and **no change
> to the engine**.

Most packs in the repository were written alongside the engine by its
author, so wherever the format could not express something, the engine
grew a feature and nobody recorded it as a failure.

So the question is not whether you can write a pack. It is **where the
format stops you**, and how you find out.

A second question rides along. Whether *interesting* reasoning emerges from
deterministic composition is untested — most conclusions the engine has
reached are ones a reader could have reached unaided. Deliverable B
addresses that.

### The two things that would make this fail as an experiment

- **Asking the engine's author how to model something.** Ask about
  *syntax* freely, ask what an error message means, ask whether something
  is a bug. Do not ask how to represent your domain. That is the variable
  under test.
- **Reading the existing packs before you are done.** They are in
  `model-packs/`. Everything you need is in this brief, and reading them
  transfers exactly the assumptions this exercise exists to detect. If you
  do look, note it in the friction log — that is data, not a
  disqualification.

---

## 2. What you need before starting

Revision 1's author could not run anything, which cost the exercise half
its value. Please confirm you have all of these before you begin:

- A clone of the DRE repository.
- A working Rust toolchain. The version is pinned in
  `rust-toolchain.toml`; `rustup` will fetch it.
- A successful `cargo build --release --bin dre`.

If you cannot run the engine, say so immediately rather than writing a pack
that cannot be checked. **Do not report results you did not observe.**

---

## 3. Deliverables

### A — A model pack, in a domain of your choosing

Pick a domain **you** know something about and the engine's author does
not. Existing packs cover propositional logic, mechanical diagnosis, grid
navigation, pump performance and freight release; anything meaningfully
unlike those is better than something adjacent. Medicine, law, agriculture,
finance, sports officiating, building regulations, education — the
criterion is that you can say what a right answer looks like and why.

It must involve reasoning, not lookup. If the answer is a table you could
index into, the pack proves nothing.

**Requirements**

- Uses at least three of: `rules`, `features`, `geometry`, `causal` +
  `goals` + `constraints`, and evidence (declared in the experiment).
- At least eight rules, or an equivalent amount of derivation. Small packs
  cannot produce non-obvious conclusions.
- Every property a rule or feature touches is declared in `ontology.yaml`.
- Passes `dre validate` (see §6).

### B — At least two experiments, one of which you cannot answer by eye

An experiment is a question asked of the pack: starting facts in, a
conclusion out.

- **B1 — a straightforward case.** You know the answer. It checks that the
  pack does the obvious thing.
- **B2 — a case you genuinely cannot resolve by inspection.** This is the
  important one. It should need at least four chained steps — evidence
  resolving, a derived quantity, one rule enabling another, a
  classification or a plan — such that a competent reader holding the pack
  and the inputs still could not state the conclusion without working
  through it.

**The protocol for B2, which is the point of the whole exercise:**

1. Write the experiment file, including the `expect:` block, with what you
   **predict** the answer to be.
2. Write down your prediction and your reasoning in the friction log
   **before you run it**.
3. Run it.
4. Record what happened.

There are three outcomes and all of them are results:

| Outcome | What it means |
|---|---|
| You predicted right | The engine composed correctly over a chain long enough to be worth composing. |
| You predicted wrong, the engine is right | The strongest available evidence for the whole thesis. Say what you missed. |
| You predicted wrong, the engine is wrong | A defect, found by the only method that can find this class. Say why the engine's answer is unacceptable. |

**Do not adjust the experiment until it passes.** If your prediction and
the engine disagree, that disagreement *is* the finding. Leaving a failing
experiment in place with a note explaining the conflict is a better
deliverable than a passing one you tuned into agreement.

### C — A friction log

See §5. Deliverables A and B are artefacts; C is the actual data.

---

## 4. What a pack is

A directory of YAML files. Every section is optional except `model.yaml`.
The engine does not know your domain; the pack is the whole of what it
knows.

```text
your-pack/
  model.yaml        identity and version               (required)
  ontology.yaml     entity types, properties, domains
  features.yaml     derived quantities: arithmetic
  rules.yaml        symbolic inference
  geometry.yaml     a space and named regions
  explanations.yaml competing accounts of what is going on
  causal.yaml       actions with preconditions and effects
  constraints.yaml  what a plan must or should avoid
  goals.yaml        target states
```

### The order things happen in

```text
declared facts + evidence
  → evidence resolution      contested observations settled by source
  → features                 arithmetic over properties
  → rules                    forward chaining to a fixpoint
  → geometry                 classification into the nearest region
  → explanations             declared accounts ranked against the state
  → planner                  search for an action sequence to a goal
```

Each stage can read what the earlier ones wrote. A rule can test a derived
feature; geometry can classify on a property a rule inferred.

**Each stage runs once, in that order.** Features and rules do **not**
re-run over the states the planner explores. The planner searches by
applying action effects only, so a plan cannot depend on a rule firing part
way through it. If your domain needs "do X, then re-evaluate", the
re-evaluation has to be something an action's effects express directly.

**The planner returns a sequence; it does not execute it.** It searches
over a copy of the state, so after a run the properties an action *would*
have changed still hold their pre-plan values. Assert the plan, not the
world it would have produced.

### Three-valued logic — the thing most likely to surprise you

Conditions do not evaluate to true or false. They evaluate to **true**,
**false**, or **unknown**, and `NOT unknown` is `unknown`, not true.

A rule whose conditions are undecidable does not fire. This is deliberate
and it is the engine's central commitment: it will decline to conclude
rather than conclude from ignorance. If a rule of yours mysteriously never
fires, the usual cause is that something it reads is not `KNOWN` or
`INFERRED`.

The same applies to constraints. A constraint the engine cannot decide does
**not** count as violated.

### Epistemic status

Every property carries one. They are not interchangeable and the engine
treats them differently:

| Status | Meaning |
|---|---|
| `KNOWN` | Observed. |
| `INFERRED` | Derived by a rule or feature, not observed. |
| `UNKNOWN` | Not observed, but observable in principle. |
| `CONFLICTING` | Sources disagree, or two rules concluded differently. No value. |
| `STALE` | Observed, but older than the freshness window. |
| `UNOBSERVABLE` | Cannot be observed in this domain, ever. |
| `INVALID` | Present but outside its declared domain, or the result of an undefined operation. |
| `NOT_APPLICABLE` | Meaningless for this entity. |

**Two limits on statuses, worth knowing before you design around them.** A
rule can test only `EXISTS`, `KNOWN` and `UNKNOWN` — no operator branches on
`STALE`, `CONFLICTING`, `INVALID`, `UNOBSERVABLE` or `NOT_APPLICABLE`, so
those five can stop a rule firing but cannot themselves become a reasoned
finding. And a probe that produced no answer is recorded in
`given.failed_reads`, not as a status and not as a domain value — see the
experiment file below.

Only `KNOWN` and `INFERRED` are readable by comparisons and arithmetic.
Everything else makes a condition `unknown`.

**Two rules concluding different values for one property give
`CONFLICTING`**, and the value is discarded — priority does not decide it,
and anything reading that property afterwards declines. If you want one of
several outcomes to win, the rules have to be written so that only one can
fire.

---

## 5. What to record

Keep a running log, `FRICTION.md`, alongside your pack. It is the primary
deliverable. Nothing in it is too small.

Record, with a timestamp or at least an ordering:

1. **Anything you wanted to express and could not.** The exact thing, in
   your own words, before you worked around it. This is the highest-value
   entry in the whole exercise.
2. **What you worked around it with**, and whether the workaround changed
   what the model means.
3. **Every time the engine did something you did not expect**, including
   when it turned out to be right.
4. **Every error message that did not tell you what to do**, quoted.
5. **How long each part took**, roughly. Where the time went matters more
   than the total.
6. **Anything you had to guess** because this brief did not say.
7. **Your B2 prediction, written before running it**, and what actually
   happened.
8. **Anything you looked at that this brief told you not to**, and why.

You do not need to be diplomatic. "This is a stupid way to express a
threshold" is a more useful log entry than a neutral paraphrase.

---

## 6. Running it

```bash
cargo build --release --bin dre
```

**A misspelled key is now a load error**, so you will hear about it the
moment you run anything. This was not true before 2026-08-15: an unknown
key was silently dropped, which meant `perconditions:` gave you an action
with no preconditions — applicable in every state — and `hard: ture` gave
you a soft constraint. Both loaded, ran, and produced a confident answer to
a question you had not asked. If you have written packs for this engine
before, that is the trap you no longer have to watch for.

**Check your pack is internally consistent.** Do this early and often — it
catches typos in property *names*, which are still silent: a condition on a
property nothing declares is `unknown`, so the rule simply never fires:

```bash
cargo run --release --bin dre -- validate model-packs/your-pack
cargo run --release --bin dre -- validate model-packs/your-pack --experiment experiments/your-001.yaml
```

**Run an experiment:**

```bash
cargo run --release --bin dre -- run experiments/your-001.yaml
```

**See the reasoning.** `--proof` prints every step; `--why` prints only the
steps the conclusion actually rested on. `--why` is usually what you want
when a result surprises you:

```bash
cargo run --release --bin dre -- run experiments/your-001.yaml --why
```

**If your pack declares hypotheses**, two more commands are worth knowing.
`explain` ranks the accounts and shows the components of each score;
`survey` reports which of your unknowns would actually settle the diagnosis,
and which cannot settle anything however interesting they are otherwise:

```bash
cargo run --release --bin dre -- explain experiments/your-001.yaml
cargo run --release --bin dre -- survey experiments/your-001.yaml
```

**Check it is reproducible:**

```bash
cargo run --release --bin dre -- benchmark experiments/your-001.yaml --iterations 1000
```

Anything other than one unique decision hash and one unique proof hash is a
defect in the engine, not in your pack. Report it immediately.

---

## 7. Syntax reference

Complete. You should not need anything outside this section. Every form was
checked against the engine by building a throwaway pack that uses all of
them, so if something here does not parse it is a bug or a regression —
report it rather than working around it.

### Identifiers

ASCII letters, digits, `-`, `_`, `.`, `:`. A **subject** is a property
reference written `entity.property`, split at the last dot.

**The first half may name an entity *instance* or an entity *type*.**

If your experiment declares an entity with `id: batch-7` of `type: batch`,
you may write `batch-7.temperature` to mean that one batch, or
`batch.temperature` to mean every batch. A rule whose subject names a type
is evaluated **once per instance of it**, binding each in turn, so one rule
covers a fleet without being duplicated per member.

Prefer the type form when the rule is a general statement about the domain,
which is usually what you mean. Reach for the instance form when a rule is
genuinely about one named thing.

**Where the type form works.** Rules and features bind it **once per
instance**. Everywhere else a subject can appear — a goal, a constraint, an
action's effect or precondition, an explanation's observation, `rank_into` —
a type name resolves only when that type has **exactly one instance**, which
is unambiguous. With several instances those sections do not yet have an
agreed meaning for a type name and will not guess at one.

If your domain needs "every instance" in a goal or an observation, say so in
the friction log. That is a real gap, not a misreading, and it is the sort of
thing this brief exists to collect.

**A rule referencing two different types is still refused.** That is a join,
and choosing cross-product or pairwise silently would make an answer depend
on a decision nobody wrote down.

**But you can traverse.** If your domain is a graph — routers to links,
units to mounts, endpoints to sessions — declare the relationship and reach
across it. See "Traversing a relationship" below. Where that is still not
enough, do **not** collapse the graph into one entity quietly: model the
domain as it really is, record in the friction log exactly what you could not
express, and let the experiment show the mismatch.

### Values

Written as plain scalars wherever unambiguous:

```yaml
some_boolean:  true          # boolean
some_integer:  750           # integer
some_symbol:   grade-a       # symbol (an identifier)
some_decimal:  {fixed: "0.5"}  # fixed-point — note the quotes
some_entity:   {ref: pump-1}   # an entity's name — not a pointer, see below
```

**Floating point is rejected.** A bare `0.5` is an error telling you to
write `{fixed: "0.5"}`. Numbers are exact fixed-point with six decimal
places; there is no rounding drift, and `0.1 + 0.2` is exactly `0.3`.

Integers and fixed-point values do **not** compare against each other. A
comparison between unlike kinds is `unknown`, not false — so a rule
comparing across the boundary never fires, and the run reports nothing
unusual.

**Every feature produces fixed-point, whatever the operation.** Subtracting
two integers yields fixed-point, not an integer. So any property a feature
writes must be declared `type: fixed` in the ontology, and rules reading it
must compare against `{fixed: ...}` operands. `dre validate` reports both
mistakes.

### `model.yaml` (required)

```yaml
model:
  id: your-pack-id
  version: 0.1.0
  requires_engine: 0.1.0
  description: >-
    What this domain is.
```

### `ontology.yaml`

Entity **types** and what properties instances of them may carry.

```yaml
entities:
  patient:                    # a type name
    properties:
      age:
        type: integer
        min: 0                # optional, inclusive
        max: 130
      temperature:
        type: fixed
      admitted:
        type: boolean
      triage_category:
        type: symbol
        values: [red, amber, green]   # optional; absent means any symbol
      referred_by:
        type: reference

relationships:                # optional; endpoint types and cardinality enforced
  treats:
    from: clinician
    to: patient
```

Types: `integer`, `fixed`, `boolean`, `symbol`, `reference`.

#### `reference` names an entity; it does not follow one

A `reference` holds an entity's identifier and can be compared for equality.
**It cannot be dereferenced.** There is no way to write "read `healthy` on
whatever `primary_db` points at" — nothing in the engine follows a reference,
and a rule that looks like it does is testing the identifier itself.

To follow something, declare a **relationship** and traverse it with `reach`:

```yaml
relationships:
  primary_db:
    from: server
    to: database
    targets_per_source: one     # what makes `collapse: the` safe
```

```yaml
    if:
      - reach:
          from: server
          path:
            - edge: primary_db
          read: healthy
        collapse: the
        operator: EQ
        operand: false
```

`dre validate --experiment` refuses a reference naming an entity the
experiment does not declare, so a typo in one is caught rather than left
looking like an ordinary pointer at something that is not there. Nothing else
about a reference is checked, because there is nothing else it does.

Declare **every** property, including ones only ever concluded by a rule.
`dre validate` will tell you if you miss one.

Bounds and vocabularies are checked by `dre validate`, not enforced during
a run.

#### An observation can only refute if something can make it false

The single most common modelling mistake in this format, and it is silent.

An observation contributes to an account in two directions: it **corroborates**
when it holds, and it **contradicts** when it is false — and contradiction is
weighted heaviest of anything in the scorer. So if nothing your pack declares
can ever make a property false, an observation on it can only ever add support.
It can never rule anything out.

The trap is writing two booleans for two states:

```yaml
      # DON'T
      datastore_write_latency_high:   { type: boolean }
      datastore_write_latency_normal: { type: boolean }
```

with one rule asserting `..._high = true` above a threshold and another
asserting `..._normal = true` below one. Concluding `normal` does **not** make
`high` false. It leaves it `UNKNOWN`, and an unknown neither supports nor
contradicts — so a measurement you took specifically to *rule an account out*
will not rule it out. The pack loads, validates clean, and concludes
confidently.

This is not hypothetical. An author modelling a storage fault predicted a
normal latency reading would leave their storage hypothesis "actively refuted",
chose that measurement on exactly those grounds, and it changed nothing: the
same account led before and after, in all three regimes of the value they
picked. They had flagged their uncertainty about this and had no way to check
it. `dre validate` now warns about this specific shape.

**Two remedies. Pick by how many states there are.**

*One property, both polarities inferred* — best for two states, and it handles
a middle band correctly:

```yaml
      datastore_write_latency_high: { type: boolean }
```

```yaml
  - id: R007
    if: [{ subject: incident.datastore_write_delta_ms, operator: GTE, operand: {fixed: "25"} }]
    infer: [{ subject: incident.datastore_write_latency_high, value: true }]
  - id: R008
    if: [{ subject: incident.datastore_write_delta_ms, operator: LTE, operand: {fixed: "10"} }]
    infer: [{ subject: incident.datastore_write_latency_high, value: false }]
```

Between 10 and 25 the property stays `UNKNOWN`, which is the honest answer:
the measurement genuinely does not settle it. Note what you get for free —
`R008` now *refutes*.

*One symbol with a closed vocabulary* — best for three or more states, or
where "neither" is not meaningful:

```yaml
      channel_state:
        type: symbol
        values: [open, renewing, expired]
```

The shipped `opc-ua-access` pack is the worked example: it asserts
`value: false` in ten places, and its accounts can be refuted rather than only
supported.

### When two rules disagree

Two rules concluding different values for one property give `CONFLICTING`,
and the value is discarded. That is right for a **belief** — two sources
disagreeing about a temperature does not make either correct, and letting
rule order decide would manufacture facts out of how the file was written.

It is wrong for a **policy**. An access decision where one rule says `deny`
and another says `manual_review` is not undecided: `deny` dominates, and it
dominates whichever rule fired first. So declare the order on the **value
domain**, in `ontology.yaml`:

```yaml
access_decision:
  type: symbol
  values: [allow, deny, manual_review]
  precedence: [deny, manual_review, allow]   # most dominant first
```

Both values must appear in the order. A vocabulary that does not mention one
of them has said nothing about how it compares, and the engine will not
guess — it conflicts as before. Applies to **inference only**: sources
disagreeing about an observation is an evidence question and is untouched.

An earlier author, modelling access control, predicted `CONFLICTING`, said
plainly that the domain answer should be `deny`, and left the clash in their
experiment rather than fudge it. This is the mechanism that answers them; you
should not have to write your rules so that only one can ever fire.

### Conditions

Used by rules, feature guards, action preconditions, constraints and goals.
Four forms, freely nested:

```yaml
# compare a property against a literal
- subject: patient-1.age
  operator: GTE
  operand: 65

# test a property's epistemic status
- subject: patient-1.temperature
  operator: KNOWN

# all of
- all:
    - subject: patient-1.age
      operator: GTE
      operand: 65
    - subject: patient-1.admitted
      operator: EQ
      operand: true

# any of
- any:
    - subject: patient-1.triage_category
      operator: EQ
      operand: red
    - subject: patient-1.triage_category
      operator: EQ
      operand: amber

# not
- not:
    subject: patient-1.admitted
    operator: EQ
    operand: true
```

Comparison operators: `EQ`, `NE`, `GT`, `GTE`, `LT`, `LTE`.
Status operators: `EXISTS`, `KNOWN`, `UNKNOWN`.

A list of conditions at the top of a rule is an implicit `all`.

### `rules.yaml`

```yaml
rules:
  - id: R001
    description: What this rule says, in words.
    if:
      - subject: patient-1.age
        operator: GTE
        operand: 65
    infer:
      - subject: patient-1.risk_band
        value: elevated
    priority: 100
```

`infer` asserts a **literal**. A rule cannot compute. Arithmetic belongs in
`features.yaml`.

Inferred values land as `INFERRED`, never `KNOWN`, and an inference cannot
overwrite something that was observed.

**Firing order** is `priority` ascending, then specificity descending
(number of leaf conditions — a more specific rule wins at equal priority),
then rule id.

**Specificity orders *firing*, not *winning*.** Both rules fire, and two
rules concluding different values for one property give `CONFLICTING`
whatever their specificity or priority. If you want one value to dominate,
declare `precedence:` on that property's value domain in `ontology.yaml` —
see below.

Rules run to a fixpoint, and inferences apply immediately, so a rule
earlier in the order can enable a later one in the same pass.

### `features.yaml`

Arithmetic over properties. This is the only place computation happens.

```yaml
features:
  - id: F001
    subject: patient-1.bmi
    description: Weight over height squared.
    expression:
      divide:
        - subject: patient-1.weight_kg
        - multiply:
            - subject: patient-1.height_m
            - subject: patient-1.height_m
    when:                     # optional guard, same condition syntax
      - subject: patient-1.height_m
        operator: GT
        operand: {fixed: "0"}
```

Operations: `add`, `subtract`, `multiply`, `divide`, `min`, `max` take a
list; `abs` and `negate` take one operand. Operands are `{subject: a.b}`,
`{literal: 5}`, or a nested operation.

**A non-whole literal is a decimal string:**

```yaml
expression:
  subtract:
    - subject: switch-1.input_voltage
    - literal: "44.0"
```

A literal here is already in a numeric position, so write a whole number or
a decimal string. The tagged `{fixed: "44.0"}` spelling used elsewhere in
this brief belongs in general **value** positions — a property's value, a
rule's operand — where the kind has to be told apart from an integer or a
symbol. There is nothing to tell apart inside an arithmetic expression.

Getting this wrong is not silent: the validator says which spelling this
position wants and why.

`subtract` and `divide` fold left: `[a, b, c]` is `a - b - c`.

Features may read other features and are evaluated to a fixpoint in **id
order**, so you do not have to declare them in dependency order.

A feature produces one of four outcomes, and the distinctions matter:

| Situation | Result |
|---|---|
| Computed | `INFERRED` |
| An operand was not available | `UNKNOWN` |
| Operands present, operation undefined (÷0, overflow) | `INVALID` |
| A `when:` guard did not hold | `NOT_APPLICABLE` |

### `geometry.yaml`

Classifies an entity into the nearest of several named regions, by weighted
squared distance.

```yaml
space:
  subject: patient-1            # whose properties form the point
  classify_into: nearest_region # property the result is written to
  dimensions:
    - id: bmi
      weight: 1
    - id: age
      weight: 2                 # weights matter: axes have different units

regions:
  - id: HIGH_RISK
    centroid:
      bmi: 35
      age: 70
    bounds:                     # optional
      bmi: { min: 30, max: 100 }
  - id: LOW_RISK
    centroid:
      bmi: 22
      age: 30
```

Dimensions name **properties of the subject** — bare names, not dotted.
Region ids become the symbol written to `classify_into`, so if you give
that property a `values:` vocabulary it must list every region.

If any dimension is not readable, the engine **declines to classify**
rather than guessing.

**`classify_into` is a ranking, not a membership test.** It names the
nearest region whether or not the subject is in it, so a patient far
outside every declared region is still classified `HIGH_RISK`. If your
domain needs the difference, ask for it:

```yaml
space:
  subject: patient-1
  classify_into: nearest_region
  inside_region_into: within_band     # boolean: is it actually in there
  boundary_distance_into: margin      # fixed: how far outside, 0 if inside
  dimensions: ...
```

Both are optional and both write properties, so declare them in the
ontology — `boolean` and `fixed` respectively. A pack that does not ask
does not get them.

### `explanations.yaml`

Optional, and the newest section — engine 0.11.0. Use it when your domain
has **competing accounts of the same situation** and a rule cannot choose
between them, which is the case a rule system genuinely cannot handle: two
correct rules concluding different things merge to `CONFLICTING`, and that
is the right answer to "do my rules agree" rather than to "which account is
better".

```yaml
# The question set, declared once and shared by every hypothesis. This is
# what makes the accounts comparable: if each brought its own list, one
# could score better by declaring less to explain.
observations:
  - id: undervoltage
    description: The rail is below its working range.
    when:                          # an ordinary condition
      subject: boiler-1.rail_voltage
      operator: LT
      operand: 200
  - id: comms_error
    when:
      subject: boiler-1.comms_error
      operator: EQ
      operand: true

hypotheses:
  - id: POWER                      # normally a symbol `rank_into` admits
    description: Rail A has collapsed.
    mechanisms:
      - id: rail_collapse
        explains: [undervoltage]
      - id: bus_degradation
        description: Everything on rail A fails together.
        explains: [comms_error]    # and any other symptoms it causes

# Optional. Where to write the winner, as entity.property. Leave it out and
# the ranking is computed and reported without touching the state.
rank_into: boiler-1.root_cause
```

**How a hypothesis is scored.** Given so you can predict a ranking rather
than guess at one. All arithmetic is fixed-point.

```text
coverage       per mechanism the state supports, counting only
               observations that need explaining:
                 1.00 + (creditable observations under it - 1) x 0.25
contradicted   observations it explains that are definitely FALSE
unexplained    observations that HOLD, need explaining, and that it
               does not account for
complexity     mechanisms declared
assumptions    mechanisms with no supported observation

score = coverage - 2.00 x contradicted
                 - 0.25 x unexplained
                 - 0.05 x complexity
                 - 0.50 x assumptions
```

Order is total: score descending, then fewer contradictions, then fewer
mechanisms, then identifier. The weights are engine constants, not pack
fields — a pack cannot tune them.

#### Check that each account can actually be argued against

Note what the formula implies: `contradicted` is the only term that takes
score *away* from an account, and it counts observations that are definitely
**FALSE**. So if nothing in your pack can make any of an account's
observations false, that account can gain score and never lose it. No
measurement will rule it out, and the ranking becomes a contest nobody can
lose — only win by more.

`dre validate` reports this per account, and it is worth reading before you
decide the pack is finished. It is not a rare mistake: of the packs in this
repository when the check was added, **one could not refute a single one of
the four accounts it declared.** Its author had chosen a measurement
specifically to rule an account out, and it could not.

The fix is usually one rule. For every account, find at least one observation
it explains, and make sure some rule can conclude the negative — see the
ontology section on modelling exclusive states.

These weights are a **scoring policy, not derived quantities**, and the
layer is **experimental rather than stable semantics**. If your domain makes
them produce an answer you think is wrong, that is a finding worth the
friction log.

**An observation may say it is not something to explain.** Set
`needs_explaining: false` on a *negative finding* — something confirmed
healthy, whose diagnostic value is that it **refutes**:

```yaml
observations:
  - id: dns_ok
    needs_explaining: false
    when: { subject: host.dns_resolved, operator: EQ, operand: true }
```

It then earns no coverage and levies no unexplained penalty on anybody, while
still contradicting a hypothesis that predicted the opposite. Default is
`true`.

Reach for it whenever an observation records that something is **fine**.
Without it, an account that mentions three healthy findings outscores one
that explains the actual fault — which is not a hypothetical: it is how an
earlier author's pack returned the wrong hypothesis, and they predicted the
right one.

**A mechanism is an independence group, and that is the point.** Symptoms
listed under one mechanism are declared correlated, so the second and later
ones corroborate at a discount rather than counting as separate support —
four sensors on one failed bus are one thing happening reported four times.
Split symptoms across mechanisms only when they really are independent
consequences.

A mechanism that explains nothing the state supports is an **assumption**,
and is charged for: it is a step your account needs that nobody has
confirmed.

Two levels only. A hypothesis has mechanisms and a mechanism has
observations; there is no nesting below that. If your domain needs a
sub-mechanism, **say so in the friction log** rather than working around it
— that is exactly the kind of report this brief exists to collect.

Nothing is written into `rank_into` unless the leader has a supported
mechanism and **strictly** outscores the runner-up. An exact tie reports as
"no explanation is preferred", which is a real result rather than a failure.

```bash
dre explain experiments/your-experiment.yaml
```

reports every hypothesis with the components of its score, not only the
winner.

### Traversing a relationship

When your domain is a graph, declare the edges and read across them.

**In `ontology.yaml`**, say what may relate to what:

```yaml
relationships:
  mounts:
    from: unit
    to: mount
    targets_per_source: many   # a unit may mount several; default
    sources_per_target: many   # a mount may serve several; default
```

`targets_per_source: one` is **enforced**, not documentation — a second
outgoing edge of that type from the same entity is an error. Declare it when
the edge really is functional, and `collapse: the` below becomes safe.

**In the experiment**, say which edges actually exist. Topology is input, not
model, for the same reason evidence is: the pack says what may relate, the
experiment says what does:

```yaml
given:
  entities:
    - { id: web, type: unit }
    - { id: srv-data, type: mount }
    - { id: srv-logs, type: mount }
  relationships:
    - { id: M1, type: mounts, from: web, to: srv-data }
    - { id: M2, type: mounts, from: web, to: srv-logs }
```

**In a rule**, follow the edge and read a property on what you arrive at:

```yaml
- reach:
    from: unit           # anchor: an instance, or a type with one instance
    path:
      - edge: mounts
        direction: out   # or `in` to follow it backwards
    read: available
  collapse: some         # REQUIRED. `the`, `every` or `some`
  operator: EQ
  operand: false
```

**`collapse` has no default and you cannot leave it out.** A traversal
usually arrives at several entities, and one truth is needed. Saying which
you mean is the entire point: the engine refuses to pick for you, exactly as
it refuses a two-type join.

| `collapse` | Holds when |
|---|---|
| `the` | Exactly one entity is reached, and it satisfies the test. Several is an **error** |
| `every` | Every reached entity satisfies it. Vacuously true if none is reached |
| `some` | At least one does |
| `{at_least: N}` | At least N do. Three-valued: `unknown` while undecided targets could still get you there |

There is no `none`: `not:` around a `some` reach gives it, with correct
three-valued negation, and one spelling is better than two. `{at_least: 1}`
is `some`; prefer `some`, which says it in a word.

**`also` — when one reached entity must satisfy several things at once.**

```yaml
# At least one scoped VM that is BOTH degraded AND host-exposed.
- reach:
    from: incident
    path:
      - edge: incident_scope
        direction: out
    read: host_cpu_exposure
    also:
      - read: app_degraded
        operator: EQ
        operand: true
  collapse: some
  operator: EQ
  operand: true
```

Each entry in `also` is `{read, operator, operand}` — read on the **same**
reached entity, conjoined with the main comparison before `collapse` sees the
result. An empty `also` is the default and means what a reach always meant.

**This is not a filter, and the distinction matters.** It does not select a
subset of targets and then test them; every reached entity contributes
`main AND every entry in also`, and the collapse still decides what a set of
those means. A filter would introduce a set you never declared, and `every`
over an empty set is *true* — so a filter matching nothing would make every
universal traversal hold.

**Why you cannot get this from two conditions.** Writing

```yaml
    if:
      - reach: { ... read: app_degraded }        # some VM is degraded
        collapse: some
        operator: EQ
        operand: true
      - reach: { ... read: host_cpu_exposure }   # some VM is exposed
        collapse: some
        operator: EQ
        operand: true
```

does **not** say what it looks like it says. Those are two independent
existentials over the same set: one VM may satisfy the first and a completely
different VM the second, and the rule fires having established nothing about
either. A pack shipped exactly that shape, its author spotted it in their own
model — *"It does not prove those are the same VM"* — and the engine
concluded a confident wrong diagnosis from it before `also` existed.

If you find yourself writing two reaches over one path in one rule, that is
the signal.

**What `also` still cannot do:** read anything but a property on the entity
reached, conclude *about* that entity, or bind it for use elsewhere. It pairs
predicates on one traversal; it is not a join.

**`path` is a list, and more than one hop is the interesting case.**

```yaml
# Every service sharing a network with this one: out to the networks it is
# attached to, then back in to everything else attached to them.
- reach:
    from: service
    path:
      - edge: attached_to
        direction: out
      - edge: attached_to
        direction: in
    read: degraded
  collapse: some
  operator: EQ
  operand: true
```

This is worth spelling out because the first author to use traversal against
a real graph reported two-hop reachability as *missing* while writing fifteen
single-hop paths. It was not missing; this section only ever showed one hop.
Consecutive hops are checked at validation — a path that cannot connect,
because the second edge does not run from where the first arrives, is an
error rather than a traversal that quietly reaches nothing.

**The second type is reached, never bound.** You never write the target type
name — it comes from the relationship. So the rule still binds one instance
of one type and the two-type refusal is untouched.

**What a traversal will not do.** It reads; it does not conclude on what it
reached. A rule reaches from a unit and concludes about that unit. If you
want to conclude something about the *mount*, write the rule from the mount's
point of view and reach back with `direction: in`.

A `reach` in a **goal** is refused at load. The planner's heuristic cannot
read one, and accepting it would silently degrade the search. Express the
traversal as a rule and give the goal the property that rule concludes.

### `causal.yaml`, `goals.yaml`, `constraints.yaml`

Only needed if your domain involves choosing a sequence of actions.

```yaml
# causal.yaml
actions:
  - id: ADMINISTER_FLUIDS
    description: What this does.
    preconditions:
      - subject: patient-1.hydration
        operator: LT
        operand: 30
    effects:
      patient-1.hydration:
        add: 20             # or  {set: some_value}  or  clear
    cost: 1
    reversible: false
```

```yaml
# goals.yaml — the first goal is the one planned toward
goals:
  - id: STABILISED
    conditions:
      - subject: patient-1.hydration
        operator: GTE
        operand: 60
```

```yaml
# constraints.yaml
constraints:
  - id: C001
    description: Never exceed the daily dose.
    hard: true
    conditions:
      - subject: patient-1.dose_today
        operator: LTE
        operand: 4

  - id: C002
    description: Prefer to avoid the invasive route.
    hard: false
    penalty: 5
    conditions:
      - not:
          subject: patient-1.route
          operator: EQ
          operand: invasive
```

**A constraint's conditions describe what must hold in an acceptable
state** — they are not a description of the situation you want to forbid.
A state in which the conditions evaluate false is the violation. So "never
release while unsafe" is written as "either it is not released, or it is
safe", which reads backwards the first time.

A **hard** constraint eliminates any state violating it. A **soft** one
adds its penalty to the cost of entering a violating state — once per
entry, so a route passing through two violating states pays twice. The
planner therefore takes the discouraged route when avoiding it would cost
more than the penalty.

**A hard constraint decides for itself what an undecidable condition
means.** Three-valued logic makes the case unavoidable, and neither answer
suits every constraint:

```yaml
  - id: C003
    description: Never release without a confirmed interlock.
    hard: true
    on_undecidable: block      # the default; omit it and you get this
    conditions:
      - subject: reactor-1.interlock_engaged
        operator: EQ
        operand: true

  - id: C004
    description: Reviewers should be assigned, once we know who they are.
    hard: true
    on_undecidable: permit
    conditions:
      - subject: case-1.reviewer
        operator: KNOWN
```

Use `block` — the default — for anything safety-shaped. "I cannot tell
whether this is safe" is not permission, and permitting there means the one
case the constraint exists for is the one it does not cover.

Use `permit` when the constraint describes something that becomes true
during the plan rather than before it. Early in a search most properties
are undecided, and a blocking constraint on one of them prunes every
candidate before the facts that would satisfy it are established — the
planner then reports no path where one plainly exists.

Soft constraints ignore the setting. They are priced rather than enforced,
and an undecidable one is never charged for.

### The experiment file

Lives in `experiments/`, not in the pack. The pack is the domain; the
experiment is a question asked of it.

```yaml
experiment:
  id: your-001
  description: What is being asked.
  pack: model-packs/your-pack

  given:
    now: 1000                 # logical tick, not a clock; for freshness

    entities:
      - id: patient-1         # the instance rules refer to
        type: patient         # its type, from the ontology
        properties:
          - name: age
            value: 71
            status: KNOWN
          - name: genetic_marker
            status: UNOBSERVABLE    # no value: a claim about not knowing

    # Optional: contested observations, resolved before anything else.
    policy:
      high_reliability: 800       # weights are integers 0-1000
      corroboration_required: 2   # independent groups needed for KNOWN
      freshness_window: 100       # ticks before an observation is STALE

    sources:
      - id: lab-a
        reliability: 900
        independence_group: lab-network   # optional; absent = independent
      - id: bedside-monitor
        reliability: 850

    evidence:
      - id: E001
        subject: patient-1
        predicate: temperature
        value: {fixed: "39.2"}
        source: lab-a
        reliability: 900
        observed_at: 995

    # Optional: attempts that produced NO value.
    #
    # This is not a reading and does not go in `evidence`. It is a record
    # that somebody looked and nothing came back -- a probe that timed out,
    # an instrument that did not answer, a query that errored. Keeping it in
    # a separate list is what makes its exclusion from resolution structural
    # rather than a filter somebody has to remember.
    #
    # Reach for this whenever you would otherwise be tempted to invent a
    # domain value meaning "we could not tell": a `status: no_answer`, a
    # `-1`, an `unknown` symbol in a vocabulary of real states. Those all
    # smuggle "the engine does not know" into "the world is in this state",
    # and the two must not blur -- a hypothesis can be credited or refuted by
    # a domain value and must not be by a failed probe.
    #
    # The property stays exactly as declared. A failed read does NOT make it
    # INVALID: nothing arrived to be invalid.
    failed_reads:
      - id: E020
        subject: patient-1
        predicate: blood_gas
        source: bedside-monitor
        reason: analyser returned no result after three attempts
        observed_at: 995

  expect:
    properties:
      - subject: patient-1.risk_band
        value: elevated
        status: INFERRED
    rules_fired: 3            # optional
    states_expanded: 0        # optional
    plan_length: 4            # optional
    # plan: [ACTION_A, ACTION_B]   # optional, exact sequence
```

**Evidence resolution.** When several sources report different values for
one property, the engine does not count sources — it counts **independent
groups**, because three sensors on one failed bus are one opinion reported
three times. A source with no declared group is its own group. The trust
placed in an observation is the *lesser* of its source's declared
reliability and its own, so a source cannot vouch for itself.

If two different values each reach the corroboration threshold, the result
is `CONFLICTING` with no value, however lopsided the split.

---

## 8. Definition of done

- [ ] `dre validate <pack>` passes with no errors.
- [ ] `dre validate <pack> --experiment <each>` passes with no errors.
- [ ] B1 runs and reports `Expectations: PASS`.
- [ ] B2 runs. It may **PASS or FAIL** — what matters is that the friction
      log says what you predicted and what happened.
- [ ] `dre benchmark <each> --iterations 1000` reports one unique decision
      hash and one unique proof hash.
- [ ] `FRICTION.md` is written.

Send back: the pack directory, the experiment files, `FRICTION.md`, and the
terminal output of the commands above.

---

## Appendix — if a rule will not fire

In rough order of likelihood:

1. A property name is misspelled. Run `dre validate`.
2. Something the rule reads is `UNKNOWN`, `CONFLICTING`, `INVALID` or
   `NOT_APPLICABLE`, so the condition is undecidable and the rule correctly
   declines.
3. You compared an integer against a fixed-point value, or the reverse.
   Unlike kinds compare as `unknown`.
4. A symbol is misspelled in a value. Nothing compares equal to it. Declare
   a `values:` vocabulary and `dre validate` will catch it.
5. Two rules concluded different values for the property, so it is
   `CONFLICTING` and carries no value.
6. The rule depends on another rule that did not fire.

`--why` shows the chain the conclusion rested on, and `--proof` shows every
rule that was tried and declined.
