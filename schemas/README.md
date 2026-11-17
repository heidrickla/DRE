# Schemas

Machine-readable schemas for model packs, canonical state, experiment
definitions, and replay records.

**Still empty** — but the gap this directory was created for is now closed
by code rather than by schema files, and that is worth recording so nobody
builds the same thing twice.

## The gap, and where it went

The original note here said:

> a pack can be structurally valid YAML and still declare a rule
> referencing a property no ontology defines. Catching that at validation
> rather than mid-run is the difference between a clear error and a
> decision quietly made on absent data.

That was exactly right, and it was true of every pack in the repository
until 2026-08-15 — `ontology.yaml` was loaded as bytes, so it reached the
`ModelHash` and editing it changed the model's identity, and then nothing
parsed it. On the day the check was written it found three real faults in
the shipped `diagnosis` pack: rules concluding `failure_mode`, `condition`
and `inspected_internally`, none of which the ontology declared.

Nothing was broken at run time. That is the point. The state builder
creates a property on first write, so those rules worked; what was wrong is
that the ontology claimed to describe the pack and did not, which means a
typo in any of those three names would have produced a rule that quietly
never fired.

It is now `dre validate`, in `crates/dre-lab/src/validate.rs`:

```bash
cargo run --bin dre -- validate model-packs/diagnosis
cargo run --bin dre -- validate model-packs/diagnosis --experiment experiments/diagnosis-001.yaml
```

It checks that every subject resolves to a declared property, that geometry
axes exist and region coordinates name real ones, that symbols written
anywhere are in the vocabulary their property declares, and — with an
experiment — that declared values match their declared kinds and domains.

One check earns the whole exercise on its own: region identifiers *are* the
symbols written to a geometry's `classify_into` property, so the regions and
the vocabulary are checked against each other. That catches the version of
this mistake people actually make — a region added later, in a different
file, with nothing connecting the two declarations.

## The other half, closed 2026-08-15

The note above is about faults a schema could not catch. There was a
second gap running the other way — faults a schema *would* have caught and
nothing did, because the parser was lenient.

Serde skips fields it does not recognise. A misspelled key in any section
was therefore dropped in silence: `perconditions` on an action gave an
action with no preconditions, applicable in every state; `hard: ture` gave
a soft constraint; `plan_lenght` gave an experiment that asserted nothing
and passed. This is exactly the "required keys and shape" checking a
schema language is good at, and it was missing.

Closed in code as well, by putting `deny_unknown_fields` on all thirty
structures a pack or experiment author writes by hand — see
`crates/dre-lab/tests/unknown_keys.rs`. So both halves are now the
validator's, and neither is waiting on a schema file.

## Why code rather than JSON Schema

A schema language can check shape: required keys, value types, patterns.
The shape half is now covered by the parser itself, above. The rest —
every fault worth catching here — is *referential*: this subject names a
property that type does not have, this region names an axis its space does
not declare, this value is outside the domain its property declares. Those
are cross-file relationships between separately parsed sections. A schema
would have to be generated from the ontology at validation time, which is
the same computation with an extra encoding step in the middle.

Schema files would still earn their place for **editor support**: completion
and inline errors while a pack is being written, which validation after the
fact cannot give. That is the reason to build them, and it is a different
reason from the one this file was created with.

## What is still unchecked

- ~~Relationship declarations.~~ Closed. Four packs and five experiments now
  declare edges. `StateBuilder::relate` enforces endpoint types and declared
  cardinality at run time rather than in validation, and `check_reaches`
  verifies statically that every hop names a declared relationship, that
  consecutive hops connect, and that the property read exists on the type the
  traversal ends at.
- ~~Domains at run time.~~ Closed in 0.7.0, by a domain rather than an audit:
  an incoming reading whose kind, domain or vocabulary is wrong is stated as
  rejected and excluded from resolution. What follows was the argument for
  leaving it, and is kept because the decision went the other way — an
  out-of-domain
  observation would become `INVALID` and rules over it would stop firing —
  which is a change to reasoning behaviour and deserves its own pass with
  its own re-recorded baselines.
