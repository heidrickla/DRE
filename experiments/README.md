# Experiments

Experiment definitions and their recorded replay records.

An experiment pairs a model pack with an input state and an expected
result. Running one produces the five hashes from plan section 17:

```text
InputHash  ModelHash  EngineHash  DecisionHash  ProofHash
```

Records committed here are the baseline that `dre replay` verifies
against. They are committed deliberately, one at a time — local run output
goes to `out/`, which is gitignored, so a stray run cannot quietly rewrite
a baseline.

A record whose `EngineHash` no longer matches the current engine is not a
failure. It means the engine changed, and the record needs re-recording
with a note about what changed and why the new result is correct.

`dre replay` reports that case as `NOT COMPARABLE`, naming which hash
differs, rather than as a mismatch — the two runs were not asked the same
question. A `DecisionMismatch` is the one that means something is wrong.

## Re-recording after an engine bump

```bash
for e in experiments/*.yaml; do cargo run --bin dre -- run "$e" --record; done
```

Then **read the diff**, which is the step that does the work. A bump that
changed no reasoning shows exactly two moved fields per record, `engine`
and `engine_version`. A `decision` or `proof` line moving means the engine
changed its mind, and re-recording is then not the fix — it is how a real
regression gets committed as a new baseline.

The `0.4.0` → `0.5.0` bump is the worked example: twelve records, two
fields each, plus `model` on `grid-40-001` alone because its pack changed
in the same commit.

## What is here

Twenty-five experiments. Every one is asserted by `stored_replay.rs` against
its committed record and by `mvp_experiments.rs` against its own `expect`
block — both, because for most of the MVP only three experiments had their
expectations checked by anything and one had been failing since the day it
landed.

Both tests enumerate this directory rather than listing it. Dropping a
`.yaml` in here is enough to put it under both; there is no list to
remember to update, because the list is what went stale the last time.

The deepest one is `fault-network-001`, and it is the only experiment here
whose result cannot be checked by reading it. What makes it checkable
anyway is described in `crates/dre-lab/tests/depth.rs`.
