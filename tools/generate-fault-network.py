#!/usr/bin/env python3
"""Generate a fault-propagation network whose answer nobody can trace.

Every other pack in this repository can be checked by hand. That is what
makes them good tests of *correctness* and useless as tests of depth: the
peak inference workload outside the grid packs is around a dozen steps, and
a careful reader reaches the same conclusion the engine does, by the same
route, in a couple of minutes. The README asks whether complex reasoning
emerges from deterministic composition, and nothing here had yet composed
enough to answer it.

This writes a domain that composes three subsystems over sixty entities:

    contested evidence  ->  arithmetic features  ->  ~450 chained rules

A dependency network of components is generated in layers. Exactly one
component is given a faulty sensor reading. Degradation then propagates
downstream, so most of the network ends up impaired, and the question the
engine has to answer is *which* component started it -- the one that is
impaired and whose every upstream is healthy.

## Why this is verifiable without being checkable

The generator plants the fault. It does not work out the consequences.

That distinction is the same one `generate-grid-pack.py` draws when it
asserts a plan length rather than a route: the length is Manhattan distance,
which the generator knows because it placed the obstacles, while the route
is the engine's business. Here the root cause is known because it was
planted, and which components end up impaired is the engine's business.

So the experiment asserts three things, all of them true by construction
and none of them requiring the generator to search the graph:

  * the planted component is the root cause;
  * a component the construction formula makes a *direct* dependent of it
    is impaired, and is not itself the root cause;
  * a component in a layer strictly upstream of the planted one is not
    impaired, because nothing upstream can be.

The second is the assertion that does the work. "Impaired but not the
root" is precisely the distinction that separates propagation from
diagnosis, and an engine that concluded "everything broken is to blame"
would satisfy the first assertion and fail that one.

## Why the root's reading is contested

The planted component's sensor readings disagree, and disagree in the way
`diagnosis-002` established the engine must resolve: three sensors sharing
one bus report it healthy, two independent probes report it faulty. Vote
counting picks healthy, three to two, and identifies no root cause at all.
Counting independence groups picks faulty.

That puts evidence resolution underneath sixty components' worth of
inference rather than beside it. A regression in resolution does not
produce a slightly different answer here; it produces a network with no
fault in it.

## No geometry

A pack declares one `space` with one `subject`, so it classifies exactly
one entity. With sixty entities there is no useful subset to classify and
the section is omitted. Recorded in `docs/decisions.md` rather than worked
around -- see the note there on what a domain needing a runtime change
means for the architectural claim.

Usage:
    python3 tools/generate-fault-network.py \\
      --out model-packs/fault-network \\
      --experiment experiments/fault-network-001.yaml
"""

import argparse
import os

# Readings are integers on a nominal scale of 1000. A component is locally
# faulty when its shortfall exceeds 20% of nominal, so the two readings
# below sit unambiguously either side of that line rather than near it --
# a threshold case would make the experiment a test of rounding.
NOMINAL = 1000
HEALTHY_READING = 980  # 2% short
FAULTY_READING = 600  # 40% short
FAULT_THRESHOLD = 20  # percent


def component(layer, index):
    """Component identifier. Dashes only -- the subject splits at the last dot."""
    return f"comp-{layer}-{index}"


def upstream(layer, index, width, degree):
    """The components one layer up that this one depends on.

    A fixed formula rather than a random draw, for the reason the grid
    generator places obstacles on a lattice: the pack has to be a
    deterministic function of its arguments or its `ModelHash` changes
    every time it is written.

    It also makes the dependency set computable by a reader without
    running a graph search, which is what lets the experiment assert a
    direct dependent without the generator searching for one.
    """
    if layer == 0:
        return []
    return [component(layer - 1, (index + k) % width) for k in range(degree)]


def dependents_of(layer, index, width, degree):
    """The layer-below components that depend on this one.

    The inverse of `upstream`, and inverted by formula rather than by
    scanning: w depends on index when (w + k) % width == index for some
    k < degree, so w is (index - k) % width.
    """
    return [component(layer + 1, (index - k) % width) for k in range(degree)]


def write(path, text):
    with open(path, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(text)


def build_ontology():
    return """# GENERATED by tools/generate-fault-network.py -- do not edit by hand.
#
# One entity type, instantiated sixty times. The reasoning is entirely in
# how the instances depend on each other, which is why the type itself is
# small.

entities:
  component:
    properties:
      # Declared per component in the experiment.
      nominal:
        type: integer
        min: 1
        max: 100000

      # Observed, and for one component contested.
      reading:
        type: integer
        min: 0
        max: 100000

      # Derived by features.yaml. Fixed-point, because every feature
      # produces fixed-point whatever the operation -- an integer operand
      # against these would compare as unknown and the rule would silently
      # never fire.
      deficit:
        type: fixed
      deficit_percent:
        type: fixed

      # Concluded by rules.yaml.
      locally_faulty:
        type: boolean
      impaired:
        type: boolean
      is_root_cause:
        type: boolean

      # Written by geometry.yaml, once per component. The space names this
      # entity *type*, so all sixty are classified against it.
      severity:
        type: symbol
        values: [NOMINAL, DEGRADED, FAILED]
"""


def build_geometry():
    return f"""# GENERATED by tools/generate-fault-network.py -- do not edit by hand.
#
# One space, sixty subjects.
#
# `subject` names an entity *type*, so every component is read into this
# space and classified separately, writing its own `severity`. That is the
# only reason this pack has a geometry section at all: until the space
# named a type it named a single entity, and a domain with sixty of them
# had no way to say which one mattered. It did not have a workaround, it
# had nothing.
#
# The axis is derived rather than measured, like `process/`. A component's
# severity is a band on its computed shortfall, so a break in the feature
# layer leaves this space without coordinates and the engine declines to
# classify rather than guessing a band.
#
# One dimension is unusual and deliberate. The weighting machinery exists
# to make unlike axes comparable and there is nothing here to weigh
# against, so a second axis would be decoration -- and a collinear one
# (`deficit` alongside `deficit_percent`, which is `deficit` scaled by a
# constant) would be worse than decoration, since it would double one
# reading's contribution and call the result a two-dimensional space.

space:
  subject: component
  classify_into: severity
  dimensions:
    - id: deficit_percent
      weight: 1

regions:
  - id: NOMINAL
    centroid:
      deficit_percent: 0
    bounds:
      deficit_percent: {{ min: 0, max: 10 }}

  - id: DEGRADED
    centroid:
      deficit_percent: 20
    bounds:
      deficit_percent: {{ min: 10, max: 30 }}

  - id: FAILED
    centroid:
      deficit_percent: 45
    bounds:
      deficit_percent: {{ min: 30, max: 1000 }}
"""


def build_features(components):
    lines = [
        """# GENERATED by tools/generate-fault-network.py -- do not edit by hand.
#
# Two features per component: the shortfall against nominal, and that
# shortfall as a percentage. Nothing downstream reads `reading` directly.
#
# The percentage is what the fault threshold is written against, so a
# component's local health is never a raw comparison against a measurement
# -- it is arithmetic over one. That is the property this layer exists to
# hold, and it is why a break in features does not give this pack a
# different answer, it gives it no answer.

features:"""
    ]
    index = 0
    for name in components:
        index += 1
        lines.append(
            f"""  - id: F{index:04d}
    subject: {name}.deficit
    description: Shortfall of {name} against its nominal output.
    expression:
      subtract:
        - subject: {name}.nominal
        - subject: {name}.reading"""
        )
        index += 1
        lines.append(
            f"""  - id: F{index:04d}
    subject: {name}.deficit_percent
    description: That shortfall as a percentage of nominal.
    expression:
      multiply:
        - divide:
            - subject: {name}.deficit
            - subject: {name}.nominal
        - literal: 100"""
        )
    return "\n".join(lines) + "\n"


def build_rules(layers, width, degree):
    """Four rule families over the network.

    Order within the file is not semantic -- the engine sorts by priority,
    then specificity, then identifier -- but the families are grouped so
    the file can be read at all.
    """
    local, propagate, clear, root = [], [], [], []
    index = 0

    def nxt():
        nonlocal index
        index += 1
        return f"R{index:04d}"

    for layer in range(layers):
        for w in range(width):
            name = component(layer, w)
            ups = upstream(layer, w, width, degree)

            local.append(
                f"""  - id: {nxt()}
    description: {name} is locally faulty -- shortfall past the threshold.
    if:
      - subject: {name}.deficit_percent
        operator: GT
        operand: {{ fixed: "{FAULT_THRESHOLD}" }}
    infer:
      - subject: {name}.locally_faulty
        value: true
    priority: 100"""
            )

            # The matching negative is not optional. Without it a healthy
            # component's `locally_faulty` stays UNKNOWN, `NOT unknown` is
            # unknown, and every rule downstream that needs to know a
            # component is *fine* never fires. Three-valued logic does not
            # let absence stand in for falsehood.
            local.append(
                f"""  - id: {nxt()}
    description: {name} is locally healthy -- shortfall within the threshold.
    if:
      - subject: {name}.deficit_percent
        operator: LTE
        operand: {{ fixed: "{FAULT_THRESHOLD}" }}
    infer:
      - subject: {name}.locally_faulty
        value: false
    priority: 100"""
            )

            propagate.append(
                f"""  - id: {nxt()}
    description: A locally faulty {name} is impaired.
    if:
      - subject: {name}.locally_faulty
        operator: EQ
        operand: true
    infer:
      - subject: {name}.impaired
        value: true
    priority: 200"""
            )

            for up in ups:
                propagate.append(
                    f"""  - id: {nxt()}
    description: {name} is impaired because {up} upstream of it is.
    if:
      - subject: {up}.impaired
        operator: EQ
        operand: true
    infer:
      - subject: {name}.impaired
        value: true
    priority: 200"""
                )

            # Clearing a component requires every upstream *explicitly*
            # false, not merely not-yet-true. That is what keeps the two
            # families mutually exclusive during the fixpoint: a component
            # cannot be cleared while anything above it is still undecided,
            # so it can never be cleared and then impaired, which would
            # merge as CONFLICTING rather than as a correction.
            clauses = [
                f"""      - subject: {name}.locally_faulty
        operator: EQ
        operand: false"""
            ]
            for up in ups:
                clauses.append(
                    f"""      - subject: {up}.impaired
        operator: EQ
        operand: false"""
                )
            clear.append(
                f"""  - id: {nxt()}
    description: {name} is sound -- locally healthy, and nothing above it is impaired.
    if:
{chr(10).join(clauses)}
    infer:
      - subject: {name}.impaired
        value: false
    priority: 300"""
            )

            # The diagnosis itself. Impaired, locally faulty, and every
            # upstream sound -- which is the only combination that means
            # the trouble started here rather than arriving from above.
            root_clauses = [
                f"""      - subject: {name}.impaired
        operator: EQ
        operand: true""",
                f"""      - subject: {name}.locally_faulty
        operator: EQ
        operand: true""",
            ]
            for up in ups:
                root_clauses.append(
                    f"""      - subject: {up}.impaired
        operator: EQ
        operand: false"""
                )
            root.append(
                f"""  - id: {nxt()}
    description: {name} is the root cause -- impaired, faulty, everything above it sound.
    if:
{chr(10).join(root_clauses)}
    infer:
      - subject: {name}.is_root_cause
        value: true
    priority: 400"""
            )

    header = f"""# GENERATED by tools/generate-fault-network.py -- do not edit by hand.
#
# {index} rules in four families, over a {layers}-layer network {width} wide.
#
#   local      each component's own health, from its computed shortfall
#   propagate  impairment travelling downstream, one rule per edge
#   clear      soundness travelling downstream, which needs its own family
#   root       the diagnosis: impaired, faulty, and nothing above it broken
#
# The `clear` family is the one worth understanding. It looks redundant --
# if a component is not impaired, why say so? -- and removing it does not
# make this pack wrong, it makes it silent. `root` asks whether every
# upstream component is sound, and in three-valued logic an upstream that
# was never decided is unknown, not false. Without `clear` the root rule
# never fires anywhere and the engine reports no fault in a network that
# plainly has one.

rules:"""

    return (
        header
        + "\n"
        + "\n".join(local)
        + "\n"
        + "\n".join(propagate)
        + "\n"
        + "\n".join(clear)
        + "\n"
        + "\n".join(root)
        + "\n"
    )


def build_experiment(layers, width, degree, root_layer, root_index, pack_out):
    planted = component(root_layer, root_index)
    ups = upstream(root_layer, root_index, width, degree)

    # A direct dependent, by the construction formula rather than by
    # searching the graph. This is the discriminating assertion: impaired,
    # and not the root.
    dependent = dependents_of(root_layer, root_index, width, degree)[0]

    # Strictly above the planted layer, so nothing can have impaired it.
    upstream_witness = component(root_layer - 1, 0) if root_layer > 0 else None

    entities = []
    for layer in range(layers):
        for w in range(width):
            entities.append(
                f"""      - id: {component(layer, w)}
        type: component
        properties:
          - name: nominal
            value: {NOMINAL}
            status: KNOWN
          - name: locally_faulty
          - name: impaired
          - name: is_root_cause
          - name: severity"""
            )

    sources = [
        """      # The three bus sensors on the contested component share one
      # failure mode, so they are one independence group however many of
      # them there are.
      - id: bus-sensor-a
        reliability: 900
        independence_group: bus-1
      - id: bus-sensor-b
        reliability: 900
        independence_group: bus-1
      - id: bus-sensor-c
        reliability: 900
        independence_group: bus-1

      # Two independent probes, each its own group.
      - id: probe-x
        reliability: 900
      - id: probe-y
        reliability: 900

      # One survey instrument reports every other component.
      - id: survey
        reliability: 900"""
    ]

    evidence = []
    counter = 0

    # The contested component first, so the disagreement is visible at the
    # top of the block rather than buried sixty entries down.
    for source in ("bus-sensor-a", "bus-sensor-b", "bus-sensor-c"):
        counter += 1
        evidence.append(
            f"""      - id: E{counter:04d}
        subject: {planted}
        predicate: reading
        value: {HEALTHY_READING}
        source: {source}
        reliability: 900
        observed_at: 990"""
        )
    for source in ("probe-x", "probe-y"):
        counter += 1
        evidence.append(
            f"""      - id: E{counter:04d}
        subject: {planted}
        predicate: reading
        value: {FAULTY_READING}
        source: {source}
        reliability: 900
        observed_at: 990"""
        )

    for layer in range(layers):
        for w in range(width):
            name = component(layer, w)
            if name == planted:
                continue
            counter += 1
            evidence.append(
                f"""      - id: E{counter:04d}
        subject: {name}
        predicate: reading
        value: {HEALTHY_READING}
        source: survey
        reliability: 900
        observed_at: 990"""
            )

    upstream_assertion = ""
    if upstream_witness:
        upstream_assertion = f"""
      # Layer {root_layer - 1} is strictly above the planted fault, so nothing
      # can have impaired it. True by construction, not by search.
      - subject: {upstream_witness}.impaired
        value: false"""

    return f"""# GENERATED by tools/generate-fault-network.py -- do not edit by hand.
#
# The depth experiment. Every other pack in this repository can be checked
# by hand; this one cannot, and unlike the grid packs the reason is the
# reasoning rather than the search.
#
# A {layers}-layer dependency network, {width} components wide, {degree} upstream edges
# each. One component -- {planted} -- has a faulty sensor reading. Impairment
# spreads downstream from it, so most of the network ends up impaired, and
# the question is which component started it.
#
# Getting there means resolving contested evidence, computing two features
# per component, and running {layers * width * (3 + degree) + layers * width} rules to a fixpoint. The answer
# is one property on one of sixty entities.
#
# What is asserted, and why each one is knowable without tracing the run:
#
#   {planted:<14} planted here. Known because this file planted it.
#   {dependent:<14} a direct dependent by the construction formula, so it
#                  must be impaired -- and must NOT be the root cause.
#                  This is the assertion that separates diagnosis from
#                  propagation: an engine concluding "everything broken is
#                  to blame" passes the first assertion and fails this one.
#   {(upstream_witness or "-"):<14} strictly upstream of the fault, so sound.

experiment:
  id: fault-network-001
  description: >-
    Root-cause diagnosis across a {layers * width}-component dependency network.
    Contested evidence feeds arithmetic features, which feed several
    hundred chained rules, to identify the single component whose failure
    explains every other failure downstream of it.
  pack: {pack_out}

  given:
    now: 1000

    # The same policy diagnosis-002 established. Corroboration is counted
    # in independence groups, so three sensors on one bus are one voice.
    policy:
      high_reliability: 800
      corroboration_required: 2
      freshness_window: 100

    sources:
{chr(10).join(sources)}

    evidence:
{chr(10).join(evidence)}

    # Every conclusion the pack can reach is declared UNKNOWN up front,
    # which is a claim rather than a formality. A property nothing has
    # mentioned is *absent*, and absent is not the same as undecided --
    # asserting `status: UNKNOWN` against an absent property fails, as it
    # should, because "the engine declined to conclude this" and "the
    # engine never had this concept" are different results.
    #
    # It also gives the unknown-resolution metric a denominator. Without
    # it the run reports "none started unknown" and the one number that
    # says how much of the question was actually answered is unavailable.
    entities:
{chr(10).join(entities)}

  expect:
    properties:
      # The planted fault, recovered.
      - subject: {planted}.is_root_cause
        value: true

      # Impaired, because it depends on the planted component directly.
      - subject: {dependent}.impaired
        value: true

      # ...and not to blame for it. UNKNOWN rather than false: no rule
      # concludes innocence, and a property nothing decided is undecided
      # rather than negative. Saying so is the whole point of the
      # epistemic status.
      - subject: {dependent}.is_root_cause
        status: UNKNOWN{upstream_assertion}

      # Geometry, once per component. The planted fault reads {FAULTY_READING}
      # against a nominal of {NOMINAL}, and everything else reads {HEALTHY_READING}, so
      # the bands each falls into are fixed by what this file planted.
      #
      # Worth asserting on a *healthy* component as well as the faulty one:
      # a space that classified only its first subject, or only one, would
      # still get the planted component right if it happened to be reached.
      - subject: {planted}.severity
        value: FAILED
      - subject: {dependent}.severity
        value: NOMINAL
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--layers", type=int, default=6, help="network depth")
    parser.add_argument("--width", type=int, default=10, help="components per layer")
    parser.add_argument("--degree", type=int, default=3, help="upstream edges per component")
    parser.add_argument("--root-layer", type=int, default=2, help="layer of the planted fault")
    parser.add_argument("--root-index", type=int, default=4, help="index of the planted fault")
    parser.add_argument("--out", required=True, help="pack directory to write")
    parser.add_argument("--experiment", help="experiment file to write")
    args = parser.parse_args()

    if args.degree > args.width:
        parser.error("degree cannot exceed width -- a component cannot depend on the same one twice")
    if not 0 <= args.root_layer < args.layers:
        parser.error(f"root layer {args.root_layer} is outside a {args.layers}-layer network")
    if not 0 <= args.root_index < args.width:
        parser.error(f"root index {args.root_index} is outside a {args.width}-wide layer")
    if args.root_layer == args.layers - 1:
        parser.error("planting in the last layer leaves nothing downstream to propagate to")

    out = args.out.replace(os.sep, "/").rstrip("/")
    os.makedirs(args.out, exist_ok=True)

    components = [
        component(layer, w) for layer in range(args.layers) for w in range(args.width)
    ]

    write(
        os.path.join(args.out, "model.yaml"),
        f"""# GENERATED by tools/generate-fault-network.py -- do not edit by hand.

model:
  id: {out.rsplit("/", 1)[-1]}
  version: 0.1.0
  # 0.1.0 to match every other pack, and every other pack is arguably
  # wrong. This one cannot run on an engine without the feature layer:
  # a version that skipped features.yaml would leave `deficit_percent`
  # absent, every threshold rule undecidable, and the network diagnosed
  # as healthy. The field is satisfied by every engine that has ever
  # existed, so nothing has yet had to be honest in it. See
  # docs/decisions.md.
  requires_engine: 0.1.0
  description: >-
    Generated {args.layers}-layer fault-propagation network of {len(components)} components.
    Composes contested evidence, arithmetic features and several hundred
    chained rules to a conclusion no reader traces by hand.
""",
    )
    write(os.path.join(args.out, "ontology.yaml"), build_ontology())
    write(os.path.join(args.out, "features.yaml"), build_features(components))
    write(os.path.join(args.out, "geometry.yaml"), build_geometry())
    write(
        os.path.join(args.out, "rules.yaml"),
        build_rules(args.layers, args.width, args.degree),
    )

    if args.experiment:
        write(
            args.experiment,
            build_experiment(
                args.layers,
                args.width,
                args.degree,
                args.root_layer,
                args.root_index,
                out,
            ),
        )

    print(f"wrote {args.out}: {len(components)} components, planted at "
          f"{component(args.root_layer, args.root_index)}")
    if args.experiment:
        print(f"wrote {args.experiment}")


if __name__ == "__main__":
    main()
