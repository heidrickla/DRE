# `cold-chain-release`

A pack written from `docs/external-pack-brief.md` alone, without access to the engine's source or a built binary. It tests the claim at the top of `model-packs/README.md`: that adding a reasoning domain requires a new pack and no change to the engine.

## What it found

**A planner defect that had been there since the planner was written.**
The author's B2 prediction — a five-step remediation plan costing 10 — was
correct, and the engine returned a four-step plan costing **11**. Dijkstra
was returning a valid plan that was not the cheapest.

The cause was that the frontier settled a state when it was first
*generated* rather than when it was *popped*. That is only correct if the
first route to reach a state is the cheapest one, and in a uniform-cost
search it is not: a state can be generated early by one expensive action
and reached later by a cheaper sequence, whose arrival was then discarded
as already seen.

Every pack written alongside the engine used **unit-cost actions**, and
with uniform costs generation order tracks cost order exactly, so the bug
was invisible to all of them. This pack has costs of 1, 2, 3 and 5. It
exposed the defect on its first run.

Fixing it also revealed that the previous version was deterministic partly
*because* of the bug — settling at generation kept second routes off the
frontier, so no two entries could tie on every ordering key. Allowing them
on required the frontier ordering to gain the path as a final tie-break.
See `crates/dre-planner/tests/cheapest_path.rs`.

## Plans and state

The planner returns a sequence of actions without applying it, so a property an action sets still holds its pre-plan value in the final state. `cold-chain-release-002` therefore asserts the plan (the actions and their order) rather than the world after the plan has run.

## Modelling boundaries this pack exercises

- Rules and features do not re-run during planning. Actions change state, but no inference happens over successor states, so a plan cannot depend on a rule firing partway through.
- A hard constraint whose conditions are undecidable does not count as violated unless the constraint declares otherwise with `on_undecidable`. Initializing every boolean the planner reads keeps a safety predicate decidable.
- Constraints are predicates over acceptable states. "Never release while unsafe" is written as an implication encoded as a disjunction.
- Two rules inferring different values for one property merge to `CONFLICTING` unless the value domain declares a `precedence`. This pack uses separate boolean flags.

## The domain

Release authorisation for temperature-controlled freight. Contested
telemetry from three probes resolves a cargo temperature; derived features
compute temperature deviation, weight variance and delay ratio; twelve
rules chain four levels deep to a recovery requirement; and the planner
finds the least-cost remediation sequence subject to a hard safety
constraint and a soft penalty discouraging emergency override.

`cold-chain-release-001` is the clean case: everything within tolerance,
one action to release. `cold-chain-release-002` is the contested one.
