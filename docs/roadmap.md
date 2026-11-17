# Roadmap

What exists, what is planned, and which designs are ruled out. Design reasoning is in [decisions.md](decisions.md); the build plan is [mvp-plan.md](mvp-plan.md).

## Build order

State, evidence, determinism and replay come before geometry, because those four are what the MVP tests. The order follows plan section 24.

| Phase | Subject |
|---|---|
| 1 | Runtime determinism and the acceptance test |
| 2 | Canonical state and the commutative merge rule |
| 3 | Model pack loader and validator |
| 4 | Symbolic reasoning: three-valued, forward chaining |
| 5 | Evidence resolution: reliability, independence groups, freshness, corroboration |
| 6 | Proof graph and rendering |
| 7 | Replay verification and the experiment runner |
| 8 | Geometry: distance, containment, ranking, boundary, direction |
| 9 | Causal actions |
| 10 | Planner: BFS, Dijkstra and A* with a total frontier order |
| 11 | `dre benchmark` and replay storage |
| 12 | Domain packs and the `dre` CLI |

## What exists beyond the MVP

| Capability | Where it is described |
|---|---|
| Derived features (`features.yaml`) | plan section on features |
| Soft and hard constraints, with a declared meaning for undecidable | [decisions.md](decisions.md) |
| Explanation ranking over declared hypotheses (`dre-explain`) | [decisions.md](decisions.md) |
| Counterfactual reasoning and the observation survey (`dre survey`) | plan section 26 |
| Declared traversal over relationships (`reach`) with a mandatory `collapse` | [decisions.md](decisions.md) |
| Declared value precedence for conflicting inferences | [decisions.md](decisions.md) |
| Failed reads recorded separately from evidence | [decisions.md](decisions.md) |
| Rule, feature and constraint subjects that name an entity type | [decisions.md](decisions.md) |
| Machine-restart and cross-machine determinism checks | [verification.md](verification.md) |

## Planned work

Each item waits on a domain that needs it. The rule for the engine is that no new reasoning abstraction arrives without a domain that fails without it.

- Lazy evidence resolution: resolve only the properties some rule reads. It matters at a scale the MVP does not target.
- `dre validate` over `given.evidence`, so a defective reading is reported before a run as well as rejected during one.
- Measurement tolerance bands, so a reading can carry an error bar distinct from a reliability weight.
- Cost of information, so the survey can weigh what gathering an observation costs.
- More than one geometric space per pack.
- `at_least: N` as a collapse for traversals.
- Explanations deeper than two levels, and scoring that encodes specificity.

## The direction: observe, reason, act

The engine is a pure function of a recorded input. The long-term direction is a loop that gathers external information and acts on it.

- Acquisition freezes into a recorded input before the engine sees it. An engine that read a live feed mid-run would have no meaningful input hash and could not be replayed. The harness gathers, records and then runs.
- `dre-lab` is where the determinism rules stop, so acquisition adapters belong there and never in an engine crate.
- The evidence layer (sources, reliability, independence groups, freshness windows, `STALE`) is the acquisition model; `given.now` is a logical tick, never a clock reading.
- `dre survey` already ranks which observation is worth acquiring next, and which would settle a diagnosis where a pack declares hypotheses.
- Acting needs an action's outcome kept distinct from its declared effect, with re-observation to close the gap. That is the first time the engine would represent the model being wrong about the world, which is a change to the epistemic core.
- `Action.reversible` is declared, hashed and read by no engine code yet. It becomes the difference between a step that can be backed out and one that commits, once anything executes.
- "The planner plans; it does not execute" is stated in several documents, including the external pack brief. Changing it is a recorded decision.

## Ruled out

These break invariants, so no domain unlocks them.

- Non-monotonic reasoning and answer-set programming. Retraction destroys the termination argument, which rests on a monotone merge rule in which `CONFLICTING` absorbs. Several stable interpretations have no total declared order, so the same input could not give one decision.
- Truth maintenance. It makes the result a function of the edit sequence, and the frozen input stops being the whole input.
- A pack-selectable reasoning mode. Anything that changes what the engine decides belongs in `EngineHash`, and a mode chosen by the pack would sit in `ModelHash`.
- A general join between entity types. A rule binds exactly one instance of one type; reach reads across declared relationships without binding a second type.
