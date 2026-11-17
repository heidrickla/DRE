# Tools

Offline helpers: verification harnesses, dataset generation, visualisation,
benchmark analysis, model-pack generation.

**Nothing here may determine an authoritative runtime decision** (plan
section 5). Everything is offline: it prepares inputs, drives the engine
from outside, or analyses outputs. The moment a tool's result feeds a
decision, that logic belongs in a Rust crate, where the determinism rules
apply and the result is covered by a hash.

The boundary is worth defending explicitly, because the convenient thing —
"just compute the regions in Python and pass them in" — is exactly how
domain assumptions end up outside the engine's replay guarantees. Passing
in a *generated model pack* is fine: the pack is hashed. Passing in a
computed decision is not.

## `restart-conformance.sh`

The machine-restart attack from plan section 21 — the one item on that list
that cannot be a `cargo test`, because a test cannot reboot the host it is
running on.

```bash
./tools/restart-conformance.sh arm --reboot   # baseline, hook, reboot
./tools/restart-conformance.sh status         # read the verdict
```

`arm` records each experiment's decision and proof hashes, installs a
one-shot `@reboot` hook, and reboots. The hook runs `verify` on the way
back up, which compares the fresh hashes against the baseline.

Two design points are load-bearing:

**It proves the machine actually restarted.** "Run, reboot, run again" is
not self-evidently a restart test: if the reboot silently failed, verify
would pass exactly as it does when it succeeds, and the exercise would be a
slow way of running the suite twice. So the baseline records the kernel's
boot ID, which is regenerated on every boot and cannot be forged from
userspace, and verify refuses to report a pass unless it has changed. The
test proves the restart before it proves anything about the engine.

**It cannot loop.** `verify` removes the boot hook as its very first act,
before anything that could fail. Even if every subsequent line crashes,
nothing reboots and nothing is left armed.

`arm` also refuses to run against a dirty working tree. A baseline that
does not correspond to a commit proves only that two runs of an unknown
tree agreed with each other.

### Running it unattended

```bash
./tools/restart-conformance.sh schedule '0 3 * * 0'   # weekly
./tools/restart-conformance.sh history                # cycles + pass rate
./tools/restart-conformance.sh selftest               # crontab isolation
```

`cycle` is what the schedule runs: archive the outgoing report, re-arm,
reboot. Archiving is not housekeeping — `verify` writes a single report
file, so without it each cycle erases the evidence of the last and a
failure survives until the next firing.

**Only schedule this on a machine you are willing to have reboot itself.**
Scheduling it on a box anyone works on makes the reboot a surprise to whoever is using it.

`selftest` exists because the two crontab entries deleted each other twice.
One marker is a prefix of the other, so any unanchored match takes both
lines, and the symptom is a crontab that is simply shorter — nothing fails,
and the check silently stops running. It asserts the full install/remove
matrix rather than the path most recently edited, which is how the second
instance got through.

Do not chain commands after `cycle` or `arm --reboot`. `systemctl reboot`
returns as soon as the request is queued, so anything after it runs while
the machine is still going down; `arm` blocks until the kernel kills it for
exactly that reason.

## `generate-grid-pack.py`

Emits a navigation pack and experiment of arbitrary size.

```bash
python3 tools/generate-grid-pack.py --size 40 --spacing 4 \
  --out model-packs/grid-40 --experiment experiments/grid-40-001.yaml
```

`--goal X,Y` places the goal somewhere other than the far corner, and
`--strategy` sets what the generated experiment declares:

```bash
python3 tools/generate-grid-pack.py --size 40 --goal 19,19 --strategy astar \
  --out model-packs/grid-40-mid --experiment experiments/grid-40-mid-001.yaml
```

The goal placement is the whole difference between measuring the planner
and measuring the heuristic. At the far corner every cell lies on some
shortest path, so `g + h` is constant and A\* expands the identical states
Dijkstra does — a real property of Manhattan distance on an open grid, and
one that makes a far-corner A\* benchmark a measurement of nothing. Off the
corner, the cells beyond the goal can be ranked worse and there is finally
something to prune.

The pack identifier follows the output directory rather than the size, so
two packs of the same size with different goals do not collide.
Regenerating `model-packs/grid-40` still produces `grid-40`, which is what
keeps the shipped pack byte-identical.

Generating a pack is within the boundary above — the pack is hashed, so
whatever this writes is covered by the `ModelHash` like any hand-written
one. Computing a *decision* here would not be, which is why the generated
experiment asserts a plan **length** rather than a route: length is the
Manhattan distance, which the generator knows because it placed the
obstacles, and the route is the engine's business.

Obstacles are a regular lattice rather than a random scatter, so the pack
is a deterministic function of its arguments and regenerates
byte-identically. A pack whose contents depended on an RNG would have a
`ModelHash` that changed every time it was written, which is the one thing
a model must not do.

The size of the generated `constraints.yaml` is itself a finding: the
constraint language has no way to say "these cells are walls" compactly, so
81 obstacles produce a 584-line file. That is fine generated and would be
unbearable by hand, which is the honest reason this exists.

## `generate-fault-network.py`

Emits a fault-propagation network and the experiment that diagnoses it.

```bash
python3 tools/generate-fault-network.py \
  --out model-packs/fault-network --experiment experiments/fault-network-001.yaml
```

Sixty components in six layers, three upstream edges each, one planted
faulty sensor reading. 451 rules in four families, two features per
component, contested evidence on the planted component, and a geometric
space that bands every component by severity. The engine has to work out
which component's failure explains all the others.

The geometry section is there because this pack is the reason it can be:
a space names an entity *type*, so declaring one classifies all sixty.
Until that changed a space named a single entity and this domain had no
way to use the section at all.

This is the depth counterpart to the grid packs. Those measure search at a
scale nobody follows; this measures *inference* at one — 253 steps and
1,675 proof nodes against the 12 steps of the deepest hand-checkable
experiment.

### The boundary, again

Same rule as above and it bites harder here, because the temptation is
larger. The generator plants the fault; it does not work out the
consequences. So the experiment asserts only what construction knows:

- the planted component is the root cause — known because this script
  planted it;
- a component the *formula* makes a direct dependent is impaired, and is
  not itself the root cause;
- a component in a layer strictly above the fault is sound, because
  nothing upstream of a fault can be downstream of it.

Which sixty-component subset ends up impaired is the engine's business,
exactly as the route across the grid is. It happens to be checkable — the
closure widens by two per layer, giving 1 + 3 + 5 + 7 — and that check
lives in `crates/dre-lab/tests/depth.rs`, where it is written as a test
rather than smuggled into the generated file as an expectation.

### Dependency edges are a formula, not a draw

`upstream(layer, index)` returns `(index + k) % width` for `k < degree`.
Regular for the same reason the grid's obstacles are: a pack whose contents
depended on an RNG would have a `ModelHash` that changed every time it was
written.

The second benefit is the one that matters for verification. A formula can
be inverted by a reader in one line, so "a direct dependent of the planted
component" is something the experiment can name without the generator
searching the graph for it — and searching the graph is the step that would
have made this script compute the answer.
