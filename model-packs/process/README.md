# `process` — derived quantities

The fourth pack, added when `features.yaml` stopped being a section the
format declared and nothing read.

## What it exercises that the other three do not

Every other pack reasons about quantities it was handed. This one reasons
about quantities it has to compute, and **nothing it concludes is reachable
without arithmetic**:

| Quantity | How it exists |
|---|---|
| `input_power`, `output_power`, `rated_output` | measured |
| `efficiency`, `efficiency_percent`, `load_percent`, `power_loss` | derived |
| `degraded`, `overloaded`, `advice` | concluded, from derived values |
| `nearest_region` | classified, on two derived axes |

A rule can only conclude a literal — `Inference` carries a `Value`, not a
computation — so before features existed a domain shaped like this one
could not be written as a pack at all. It needed an engine change, which is
the one thing `model-packs/README.md` says a new domain must never need.

## Two things worth reading the files for

**`features.yaml` declares F001 before the F002 it depends on.** Features
run to a fixpoint in identifier order, not declaration order, so the file
never has to be topologically sorted by hand. Requiring that would make the
ordering of the file semantic, and orderings get edited by people thinking
about something else.

**`geometry.yaml`'s axes are both derived.** If derivation fails, the space
has no coordinates and the engine declines to classify rather than
substituting a default. `process-002` is that case.

## The stopped pump

`process-002` measures zero watts in and zero watts out.

Efficiency is `0/0`. It is not zero — it is undefined, and the pack's whole
second purpose is that the engine says so. `pump.efficiency` comes back
`INVALID`: every operand was available and the operation has no result,
which is a different statement from `UNKNOWN`, where the operands were not
available at all.

The distinction is not pedantry. A model computing `0/0 = 0` would classify
an idle pump as catastrophically inefficient and recommend servicing it,
and the recommendation would look perfectly reasonable all the way down.

What follows from it is worth tracing:

- `pump.efficiency` — `INVALID`, arithmetic has no result
- `pump.efficiency_percent` — `UNKNOWN`, because its operand was not
  available; the cascade correctly degrades rather than propagating
  invalidity as though it were a value
- `pump.power_loss` — `0`, and correctly so: zero in minus zero out is a
  defined subtraction, and a stopped pump really is losing nothing
- classification — declined, because one of the two axes has no coordinate
- `R001` — does not fire; comparing against an `INVALID` property is
  undecidable, not false, so the pump is never concluded to be degraded on
  the strength of a number that does not exist

This is `INVALID`'s first producer in the engine. Until features, the
status was declared, guarded against in condition evaluation, and never
actually written by anything.
