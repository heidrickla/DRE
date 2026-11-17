# Model packs

A model pack defines a reasoning domain. It contains no engine code.

This directory carries the MVP's central claim: adding a reasoning domain
must require a new pack and **no change to the DRE runtime**. If a domain
ever needs a runtime change, that is a finding about the architecture, not
a task — record it in `docs/decisions.md` before making the change.

## Structure

Per plan section 9, a pack may declare:

```text
model.yaml        identity, version, engine requirement
ontology.yaml     entity and relationship types
features.yaml     derived quantities: arithmetic over properties
rules.yaml        symbolic inference rules
geometry.yaml     dimensions and regions
explanations.yaml competing hypotheses, ranked against the observed state
causal.yaml       actions, preconditions, effects
constraints.yaml  constraints on candidate results
goals.yaml        target states
tests/            expected results, used by `dre validate`
```

Sections are optional. A pure logic pack declares no geometry; a
navigation pack declares no evidence sources.

## The packs

Chosen to exercise genuinely different engine paths. Variations of one
problem shape would not test domain isolation at all.

| Pack | Exercises |
|---|---|
| `logic/` | Symbolic reasoning only. No geometry, no planning. |
| `diagnosis/` | Evidence, inference, geometric classification, missing observations. |
| `navigation/` | Geometry, constraints, planning, deterministic path selection. |
| `process/` | Derived features, arithmetic, computed geometric axes, undefined versus unmeasured. |
| `haulage/` | Constraints as prices and as prohibitions: a soft penalty that is paid, and hard constraints that go undecidable and disagree about what that means. |
| `device-cascade-abduction/` | Declared hypotheses ranked against the observed state, where two correct rules could only conflict. Correlated symptoms discounted rather than counted. |

Three were planned. `process/` was added when `features.yaml` turned out to
be a capability no existing pack needed — and, more to the point, one no
pack could have supplied for itself. See below.

`haulage/` was added for the same reason one step along: the constraint
layer had two behaviours that only its own unit tests had ever triggered.
No shipped pack had ever *paid* a soft penalty — `navigation/`'s hazard is
routed around for free, because an equal-length detour exists — and none
had a hard constraint that could go undecidable, so `on_undecidable` was a
setting nothing but a fixture had exercised. Implemented is not reachable,
and this repository keeps relearning it.

`device-cascade-abduction/` was added for the third instance of the same
pattern, and the sharpest one. `device-cascade/` — below — was written to
break the engine and succeeded: two correct rules concluded two different
root causes and the property merged to `CONFLICTING`, which no arrangement
of rules could have improved on. Ranking competing accounts is a capability
no rule can supply, and it took a pack built to fail to show it.

Both packs are kept, deliberately. `device-cascade/` is the evidence that
rules cannot rank; overwriting it with the fix would have left only the fix.

### The other fourteen, which are not design packs

They are here for different reasons and are worth keeping separate from the
table above, because none of them was written to demonstrate a capability.

| Pack | Why it exists |
|---|---|
| `cold-chain-release/` | **Externally authored** from `external-pack-brief.md` alone. Found Dijkstra returning plans that were valid, optimal-looking and not the cheapest. |
| `digital-evidence-triage/` | **Externally authored**, the second independent domain. Ten composed steps to a conclusion its author could only reach by tracing them. |
| `grid-40/` | **Generated** by `tools/generate-grid-pack.py`. Scale, where equal-cost paths are the norm and the frontier's total ordering is load-bearing on every expansion. |
| `grid-40-mid/` | **Generated.** The same grid with the goal off the corner, which is the only placement where the A\* heuristic has anything to rank as worse. |
| `fault-network/` | **Generated** by `tools/generate-fault-network.py`. Depth: sixty entities, 451 rules, and a conclusion no reader traces by hand. |
| `industrial-switch-diagnosis/` | **Externally authored**, the third independent domain, from the brief and a binary alone — no checkout, no git history. Found rules bound to an instance rather than a type, and a proof naming only the reading that lost. |
| `opc-ua-access/` | **Externally authored**, and the first commissioned with *no engine binary at all*, so its predictions could not be tuned. Found that a rule reading one entity and concluding about another was refused as a join, and that policy conflicts need dominance rather than CONFLICTING. Kept exactly as written. |
| `opc-ua-access-precedence/` | The pack above plus one declared line. The pair is the demonstration: same evidence, same rules, and `precedence: [deny, manual_review, allow]` turns an undecided access decision into `deny`. |
| `windows-admin-reboot/` | **Externally authored.** Patch and reboot readiness, and the only external pack to exercise the planner. Wrote a type-named subject in a goal, which is how the goal layer's missing type resolution surfaced. |
| `network-forwarding-policy/` | **Externally authored.** Path selection and forwarding state. Collapsed a topology into one entity because the format refuses joins, and said so — the clearest statement of the largest outstanding gap. |
| `linux-admin/` | **Externally authored.** systemd startup diagnosis. Wrote type-named subjects in explanation observations, which read UNKNOWN forever and made the whole ranking decline silently. |
| `device-cascade/` | Written to **break** the engine rather than to demonstrate it. It did: a defective reading was admitted, and once that was fixed two correct rules conflicted with no way to rank them. Kept unchanged as the evidence for both. |
| `bus-independence/` | Three sensors behind one undeclared shared failure. What the independence guard catches, and what it cannot: correlation it is not told about produces a confident wrong action. |
| `fleet/` | One rule, three switches. A rule subject naming an entity *type* and binding once per instance — and the first pack whose `requires_engine` genuinely bites. |

`fault-network` is the only pack here with more than one interesting
entity, and that is how the geometric layer's limit was found — see below.

The two generated packs are a pair and neither is redundant. At the far
corner every cell lies on some shortest path, so `g + h` is constant and
A\* expands the identical states Dijkstra does; the guidance can only be
measured away from it. Both are legitimate input — a generated pack is
hashed like any other, and what the generator must not do is compute a
*decision*. See `tools/README.md`.

## When a domain needs a runtime change

The rule at the top of this file says that is a finding, not a task. It has
happened twice, and recording how is more useful than the rule alone.

### Once it was built: arithmetic

A rule concludes a **literal**: `Inference` carries a `Value`, not a
computation. So a pack could say "if temperature exceeds 800 then
bearing_fault is true" and could not say "efficiency is output over input"
at all. Every domain whose reasoning begins with a ratio, a margin, or a
difference from a set point was therefore inexpressible — not awkward,
inexpressible — and the format had declared `features.yaml` for exactly
that purpose since the beginning without anything reading it.

The resolution was to build the missing layer rather than to work around it
in a pack, because a workaround would have meant precomputing the
arithmetic outside the engine and feeding in the answers, which moves the
reasoning out of the model and defeats the point.

`process/` exists to keep that layer honest: every conclusion it reaches
runs through arithmetic, and both its geometric axes are computed rather
than observed, so a regression in features breaks it loudly rather than
subtly.

### And again: geometry at sixty entities

A pack declares one `space`, and the space named one *entity*. So a pack
classified exactly one thing, however many it had.

Nothing had noticed, because until `fault-network` every pack had exactly
one interesting entity — one pump, one machine, one artifact, one agent.
And each of them called that entity the same thing as its type, `pump` of
type `pump`, so no pack could even tell whether the subject was being read
as an entity or as a type.

Recorded as a finding first and built in the pass after: `subject` names an
entity **type**, and every instance is classified. `fault-network` declares
a space now and bands all sixty components.

The order matters more than the outcome. Writing the finding down forced
the question of whether a list of spaces would do instead — it would have,
and it would have been the same limitation with more typing, since the
interesting property of sixty components is that they are all the same
kind of thing.

The general shape is worth noting: three sections have now turned out to be
scoped to whatever the existing packs happened to need, and all three were
found by writing a new domain rather than by reading the code.
