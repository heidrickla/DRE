//! Execution context.
//!
//! One context threads through a single run. It carries the configuration,
//! the engine identity, and the workload counters that the benchmark suite
//! in plan section 20 reports.
//!
//! Note what it deliberately does *not* carry: no clock, no random number
//! generator, no thread pool handle, no environment access. Those are the
//! usual routes by which ambient state reaches a decision. Withholding
//! them from the context means a reasoner cannot reach for one without
//! that showing up plainly as a new dependency in review.
//!
//! Elapsed time *is* measured, but by the lab harness around the engine,
//! never by the engine itself — a timing that cannot be observed cannot
//! change an answer.

use serde::{Deserialize, Serialize};

use crate::canonical::CanonicalWriter;
use crate::config::RuntimeConfig;
use crate::error::{CoreError, Result};
use crate::hash::Hash32;
use crate::version::Version;

/// Engine version, tracked separately from the crate version so that a
/// release which does not change reasoning behaviour does not invalidate
/// stored replay records.
///
/// The converse is the obligation: a release that *does* change reasoning
/// behaviour must move this, or `EngineHash` claims two engines are the
/// same when they decide differently — and every stored record produced
/// under the old behaviour would verify against the new one.
///
/// **0.2.0**, 2026-08-15. Four behaviour changes in one day, each of which
/// alone would require it:
///
/// - Conditions are read by explicit dispatch rather than as an untagged
///   enum, so a malformed comparison is an error instead of silently
///   becoming a status test.
/// - `IN`, `AND`, `OR` and `NOT` are gone from `Operator`; no evaluator
///   ever implemented them, so a condition using one could never fire.
/// - `max_rules`, `max_dimensions`, `max_regions` and `max_actions` are
///   enforced. They were in this hash while enforcing nothing.
/// - A hard constraint decides for itself what an undecidable condition
///   means, and the default is now to block rather than to permit.
///
/// **0.3.0**, same day. `boundary_distance` and region containment are
/// reachable from a pack for the first time — plan section 16 required
/// both and neither had a caller anywhere in the workspace.
///
/// Additive, and no existing pack declares the new fields, so no stored
/// decision changes. It still moves the version: an engine without these
/// outputs would silently *ignore* a pack that asks for them, since a
/// section deserializer skips fields it does not know. Two engines that
/// answer the same pack differently must not share a hash.
///
/// **0.4.0**, same day, and the reason is the sentence above. Every
/// structure a pack or experiment author writes by hand now denies unknown
/// fields, so a misspelled key is an error rather than a silently narrowed
/// model. A pack that loaded before may now refuse to, which is precisely
/// the difference this constant exists to record.
///
/// **0.5.0**, 2026-08-15. The planner's search strategy is configuration an
/// experiment may declare, and it is part of the engine identity.
///
/// No stored decision changes: the runner has always searched with
/// Dijkstra, which is what this configuration defaults to, and A\* was
/// measured on a 40-by-40 grid returning the same optimal plan.
///
/// It moves the version for the *third* reason, the one the accepts/decides
/// pair does not cover. A\* reached that identical plan having expanded 383
/// states where Dijkstra expanded 727, so it rejected a different set of
/// candidates and recorded a different proof. `ProofHash` is one of the
/// five. An engine knob that changes a verified hash belongs in the hash
/// that identifies the engine, whether or not it changes the answer.
///
/// **0.6.0**, same day. A geometric space names an entity **type**, and
/// every instance of it is classified.
///
/// This one is both halves at once, which is why it moves the version
/// despite changing no stored decision. It **decides** differently: a pack
/// with two entities of the declared type now produces two classifications
/// where it produced one. It also **accepts** differently: a pack whose
/// `subject` named an entity that is not also a type classified that
/// entity before and classifies nothing now.
///
/// No existing record moved, and that is the hazard rather than the
/// reassurance. Both geometric packs named their single entity the same
/// thing as its type — `machine` of type `machine` — so the old lookup and
/// the new one return the same entity, and every baseline verified against
/// the changed engine without complaint. An engine that answers a new pack
/// differently is a different engine even when it answers every old one
/// the same way.
///
/// **0.7.0**, same day. An incoming observation is checked against the
/// declared ontology before it may participate in resolution. A reading
/// whose kind, domain or vocabulary is wrong is recorded as rejected and
/// excluded; a subject whose every reading was defective becomes `INVALID`.
///
/// The first version bump driven by a domain rather than by an audit.
/// `device-cascade` submitted a temperature of 9999 against a declared max
/// of 200. It was admitted, it disagreed with a sound reading, the subject
/// resolved `CONFLICTING`, the overheating rule could not fire, and the
/// competing explanation never appeared — so the engine reported the right
/// diagnosis **because a sensor was lying**. Repairing the sensor broke it.
///
/// This changes one stored decision, deliberately: `device-cascade-001` no
/// longer concludes `POWER`. That conclusion was an artefact of the defect,
/// and removing the artefact leaves two live explanations the engine cannot
/// rank — which is the real gap, now visible instead of masked.
///
/// Nothing is repaired, clamped or substituted. A defective reading stops
/// being a value and stays visible as a rejection.
///
/// **0.8.0**, 2026-08-16. A resolution records the observations that
/// *backed* it, not only the one it overruled.
///
/// `Resolution.supporting` has carried them since evidence resolution was
/// built and nothing ever read it, so a contested measurement named the
/// reading it discarded and collapsed the decisive ones into an anonymous
/// input. The proof preserved provenance for the loser and dropped it for
/// the winners.
///
/// The ninth declared-but-unread find, and the first reported by somebody
/// outside the project: the third independent author noticed the *symptom*
/// while diagnosing a switch — that they could not tell from the proof
/// whether a stale reading had been ignored, rejected, or simply not
/// mattered.
///
/// No decision moves. Every `ProofHash` for an experiment using evidence
/// does, which is the whole point: two engines that explain the same answer
/// differently are different engines.
///
/// **0.9.0**, 2026-08-16. A rule subject may name an entity **type**, and
/// the rule is then evaluated once per instance of it.
///
/// Rules bound to an entity identifier, so a pack described one named
/// chassis. `switch-1.cpu_load` worked; `ethernet_switch.cpu_load` matched
/// no entity and the condition stayed unknown forever, so diagnosing a
/// fleet meant duplicating every rule per member or generating the pack
/// from outside.
///
/// This is the geometry defect of 0.6.0 one layer over, and it hid for the
/// same reason: every pack written in-house calls its single entity the
/// same thing as its type — `pump` of type `pump` — so the two readings
/// agree and neither is distinguishable. It took the third independent
/// author, whose entity was `switch-1` and whose type was
/// `ethernet_switch`, to separate them.
///
/// Nothing existing moved. Not one decision, proof, or model hash changed
/// across twenty-three experiments, because a type with one instance binds
/// to that instance and a subject naming no type is read exactly as before.
///
/// A rule referencing **two** types is refused rather than guessed at. That
/// is a join, and choosing cross-product or pairwise silently would make an
/// answer depend on a decision nobody wrote down.
///
/// **0.10.0**, 2026-08-16. An experiment may record an observation that was
/// *attempted* and produced no value.
///
/// Asked for by the third independent author, who wanted to state that "the
/// PSU telemetry read itself failed" rather than merely omit the value, and
/// whose workaround lost the distinction between "no observation yet" and
/// "the instrument attempted a measurement and failed".
///
/// `failed_reads` is a separate list from `evidence`, so exclusion from
/// resolution is **structural** rather than a filter somebody must
/// remember. It carries a required reason, because a failed read with no
/// reason is indistinguishable from an absent one.
///
/// It records and nothing more. **No new status and no new operator**: the
/// same author established that recording was sufficient for what they
/// found, and explicitly declined to claim the stronger requirement. Rules
/// can test only EXISTS, KNOWN and UNKNOWN, so a status would have given a
/// rule no new power anyway.
///
/// A failed read does **not** make a property `INVALID`. Nothing arrived to
/// be invalid; the property stays unobserved, with the reason recorded
/// beside it.
/// **0.11.0**, 2026-08-16. A pack may declare competing hypotheses, and the
/// engine ranks them against the observed state.
///
/// The first post-MVP track that adds reasoning rather than borrowing it.
/// Counterfactuals and surveys are ordinary runs with a comparison on top;
/// this is a new question the engine could not previously be asked.
///
/// `device-cascade` is the domain that demanded it and the reason it is not
/// speculative. Two correct rules concluded two different root causes, the
/// property merged to `CONFLICTING`, and that was the right answer to the
/// wrong question: "do my rules agree" is not "which account of this device
/// is better". A rule fires or it does not, so no arrangement of rules can
/// weigh one conclusion against another.
///
/// **It is not a count of symptoms.** Four sensors on one degraded bus are
/// one thing happening reported four times, and scoring them as four is the
/// evidence-counting mistake `dre-evidence` already refuses to make. A
/// mechanism is an independence group: observations a hypothesis hangs off
/// the same mechanism corroborate at a discount, and the *shape* of the
/// explanation decides the score rather than the length of the symptom list.
///
/// The weights live here rather than in packs, so they are covered by this
/// version and by `EngineHash`. A pack-tunable weight would let a modeller
/// who disliked a ranking adjust it until the intended hypothesis won.
///
/// Nothing existing moves. No pack in the repository declared an
/// `explanations.yaml`, the section is optional, and a run without one takes
/// the same path it always did — verified across every committed experiment,
/// where only the `engine` line changes and every decision and proof hash is
/// byte-identical.
/// **0.12.0**, 2026-08-16. A hypothesis may not account for the same
/// observation twice, and `rank_into` must be a usable subject.
///
/// Both were accepted and both were wrong, and the first is the one that
/// mattered. Coverage is credited once per supported mechanism and again per
/// observation within it, so listing one observation under three mechanisms
/// bought three independent credits for a fact established once: 2.85
/// against 0.95 for a rival explaining the identical evidence honestly.
/// Repeating it four times inside a single mechanism scored 1.70.
///
/// That defeats the claim 0.11.0 was built on — that the score is not a
/// count of symptoms — more directly than the shared-observation asymmetry
/// recorded there, and unlike that one it needs no adversarial
/// construction. Two mechanisms producing the same symptom is
/// **overdetermination**, which is a real thing a modeller would write down;
/// the format cannot express it, so it is refused rather than silently
/// discounted. If a domain needs it, it should arrive with that domain.
///
/// Found by an independent review of `dre-explain`, which was asked to
/// attack the layer and did — the bug was not among the five weaknesses the
/// question listed. Confirmed by measurement before it was fixed.
///
/// The second is smaller and the same shape as everything else here: a
/// `rank_into` of `boiler-1.` split successfully, failed to parse as an
/// identifier, and the run returned having written nothing and recorded no
/// decision node. A pack that believed it was concluding a root cause
/// concluded nothing, quietly. Now an error, checked at validation as well
/// as at the point of writing.
///
/// Nothing existing moves. No committed pack could have hit either case, and
/// every decision and proof hash is byte-identical across the bump.
/// **0.13.0**, 2026-08-16. A subject naming an entity **instance** binds
/// that instance, even where a type shares the name.
///
/// 0.9.0 let a rule subject name a type and bind once per instance, and
/// claimed nothing existing moved. Nothing did — but it made a common shape
/// unmodellable, which no existing pack could reveal.
///
/// A pack that calls its single server `server` and its single endpoint
/// `endpoint` is doing the obvious thing, and every in-house pack does it:
/// `pump` of type `pump`, `machine` of type `machine`. In such a pack every
/// rule reading one entity and concluding about another was read as a join
/// between two types and **refused**. That is not a join; it is the ordinary
/// reason a pack declares more than one entity.
///
/// Found by the fourth independently authored pack, modelling OPC UA with
/// six entity types and one instance of each. It validated clean and could
/// not run at all: `rule R001 references entity types server and endpoint`.
/// Before 0.9.0 it would have worked, so a capability had silently removed
/// one.
///
/// The rule is now stated rather than emergent: **a name is a type exactly
/// when no instance answers to it.** `fleet`, which 0.9.0 was built for, is
/// untouched — its instances are `sw-a`, `sw-b`, `sw-c` and nothing is
/// called `switch`.
///
/// Verified: all twenty-nine previously committed experiments reproduce
/// their stored decision and proof unchanged. Only the `EngineHash` moves.
/// **0.14.0**, 2026-08-16. A symbol property may declare which of its values
/// dominates when two inferences disagree.
///
/// Two rules concluding different values give `CONFLICTING` and discard the
/// value. That is right for a **belief** — two sources disagreeing about a
/// temperature does not make either correct, and letting rule priority
/// decide would manufacture facts from authoring order.
///
/// It is wrong for a **policy**. An access decision where one rule says
/// `deny` and another says `manual_review` is not undecided: `deny`
/// dominates, whichever rule fired first. The fourth independent author hit
/// this modelling OPC UA, predicted `CONFLICTING`, said plainly the domain
/// answer was `deny`, and left the clash in rather than fudge it. Second
/// independent report of the class after `device-cascade` — and unlike that
/// one it is not answered by ranking explanations, because what is wanted is
/// dominance rather than best fit.
///
/// So the order is declared **on the value domain**, in the ontology, where
/// `ModelHash` covers it: `precedence: [deny, manual_review, allow]` says
/// what the words mean relative to each other. Rule priority still orders
/// firing and still does not decide merging; a pack that wants dominance has
/// to say so in the one place that describes what the values are.
///
/// Deliberately narrow. It applies to **inference only** — observations
/// disagreeing is an evidence question with provenance to reason over. Both
/// values must appear in the declared order, because a vocabulary that does
/// not mention one of them has said nothing about how it compares, and
/// guessing there would be the silent decision this exists to avoid. And a
/// resolution is recorded as a TIE-BREAK naming what it overruled, so a
/// dominated conclusion is visible rather than merely absent.
///
/// Nothing existing moves: no committed pack declares an order, so the table
/// is empty and the merge is the one it always was. Verified across all
/// thirty-one experiments before the demonstrating pack was added.
/// **0.15.0**, 2026-08-16. Type-named subjects resolve everywhere a subject
/// appears, not only in rules.
///
/// 0.9.0 let a rule subject name an entity type. 0.13.0 fixed how that
/// interacts with instance names. Neither noticed that **the rest of the
/// pack format still could not do it**: features, goals, constraints, an
/// action's effects and an explanation's observations all resolved subjects
/// their own way, and each had to remember separately.
///
/// The brief documents subjects once. Three independently authored packs —
/// Windows administration, network forwarding, Linux systemd — found this in
/// a single sitting, each in a different section. Every one of them wrote
/// type-named subjects on the first attempt, because that is what the
/// documentation says a subject is.
///
/// What made it expensive is that it mostly did not error. `unit.mount_timeout`
/// in an observation is not a malformed subject; it is a subject that reads
/// `UNKNOWN` forever. So the packs loaded, validated, ran, and quietly
/// concluded nothing: every observation undecided, every hypothesis an
/// unsupported assumption, every ranking declining to choose. A pack that
/// says nothing looks exactly like a domain the engine has no opinion about.
///
/// Two things changed. **Features bind per instance**, exactly as rules do,
/// because a derived quantity is as naturally per-instance as an inference
/// and a format that allows one but not the other is drawing a line nobody
/// can defend. **Everywhere else, a type naming exactly one instance
/// resolves to it** — reads and writes alike, so goals, constraints,
/// observations and effects all work. A type with several instances is left
/// alone rather than guessed at: what a goal naming a type should mean when
/// there are ten of them is a real question with a different answer per
/// section, and it waits for a domain that has ten.
///
/// A declined ranking now declares its `rank_into` target `UNKNOWN` rather
/// than leaving it absent. "Weighed these accounts and could not separate
/// them" is a different statement from "this property does not exist", and
/// two of the three authors asserted exactly that, having predicted the tie
/// correctly and failed on the property being missing.
///
/// All thirty-one previously committed experiments reproduce their decision
/// and proof unchanged.
/// **0.16.0**, 2026-08-16. Entity resolution lives in one place, and the
/// paths 0.15.0 missed now use it.
///
/// 0.15.0 fixed the resolution sites that were known. An independent mapping
/// of every site in the workspace found it had left two binding
/// implementations, three byte-identical copies of the sole-instance
/// fallback, and several bare lookups with none — so the mechanism that
/// produced the defect was intact even though the reported instances were
/// closed.
///
/// **One residual defect of the same class, confirmed by measurement.**
/// `record_underivable` applied the ORIGINAL unbound feature, so a
/// type-named feature that derived was fine and one that could not raised
/// `references unknown entity unit` — the exact error 0.15.0 was released to
/// remove, reachable through the other door. `linux-admin` and
/// `network-forwarding-policy` were latent on it, running only because every
/// operand in their committed experiments happens to be `KNOWN`. Removing a
/// single observation reproduced it.
///
/// A rule applied on the path that works and not on the path that fails is
/// not applied.
///
/// **The planner disagreed with itself about one goal.** `satisfied` goes
/// through `evaluate` and resolves a sole-instance type name; `heuristic`
/// did its own bare lookup and did not. A goal written `host.reboot_completed`
/// against an instance `host-1` contributed zero estimated distance, so A\*
/// expanded like Dijkstra and moved the `ProofHash` — silently, because a
/// heuristic that underestimates is still admissible and nothing downstream
/// can tell. Latent rather than dormant: `windows-admin-reboot` has exactly
/// that goal shape.
///
/// **And one feature declaration resolved subjects by two rules**, its
/// `when:` guard through `evaluate` with the fallback and its `expression:`
/// through a bare lookup without.
///
/// `CanonicalState::resolve_entity` is now the single definition — exact
/// instance wins, else the sole instance of that type, else nothing. A rule
/// that lives in one place cannot be forgotten by a section added tomorrow,
/// which is the actual fix; the three preceding paragraphs are its evidence.
///
/// All thirty-eight experiments reproduce decision and proof unchanged.
/// **0.17.0**, 2026-08-16. An experiment may declare the edges between its
/// entities, and the ontology's relationship types are finally read.
///
/// `RelationshipType` has been declarable since the ontology was written,
/// hashed into every `ModelHash`, and read by nothing. `StateBuilder::relate`
/// was written with it and had zero callers. `StateError::DuplicateRelationship`
/// and `StateError::UnknownRelationshipEndpoint` were unreachable. Two packs
/// declared a `relationships:` block and the brief admitted out loud that it
/// was "declared but not yet checked".
///
/// Topology is **input**, not model, for the same reason evidence is: a pack
/// says what kinds of thing exist and how they may relate; an experiment says
/// which ones actually do. Two sites with the same equipment are two
/// questions asked of one domain.
///
/// `RelationshipType` gains `targets_per_source` and `sources_per_target`,
/// both defaulting to `many` — the weakest claim, so packs that already
/// declare relationships parse byte-identically and gain no guarantee they
/// did not ask for. `one` is **enforced in `relate`**, not described in
/// prose, so a functional edge is functional in the state. Endpoint types are
/// checked there too rather than in `dre validate`, because validate is a
/// separate pass with no path into a decision and a check that only lives
/// there is not a refusal.
///
/// This is the groundwork for declared traversal, not the traversal itself.
/// `bus-independence` is the standing argument for wanting it at all: the
/// engine cannot reason around topology that was never declared, and
/// `bus-independence-003` shows it acting confidently and wrongly on
/// correlation nobody wrote down.
///
/// Nothing existing moves: no committed experiment declares an edge, so the
/// relationship map stays empty and `InputHash` is unchanged across all
/// thirty-eight.
/// **0.18.0**, 2026-08-16. A rule may traverse a declared relationship and
/// read a property on what it reaches, provided it says how several reached
/// values become one answer.
///
/// The join answer, and it does not lift the join refusal. A rule still
/// binds exactly one instance of exactly one type; the second type is
/// **reached, never bound** — the author never writes the target type name,
/// it comes from the relationship declaration — so `bindings_for` still sees
/// one type and `MultipleTypes` still fires for a genuine two-type rule.
///
/// Four independently authored packs each collapsed a graph into one entity
/// because they could not express this. It is the most-reported limitation
/// in the project's history.
///
/// **`collapse` is mandatory and has no default.** A traversal has the same
/// problem a join does — several targets, one truth needed — and the
/// resolution is that the author writes the decision down. Both rejected
/// designs converted one silent decision into another and both conceded the
/// new one was worse, because the old one refuses and the new one fires.
///
/// Two refusals rather than answers, and both are load-bearing:
///
/// An **unresolved anchor is an error, never an empty set**. `every` over an
/// empty set is true, so a silently unresolved anchor would make every
/// universal traversal hold, every rule containing one fire, and the proof
/// say so in well-formed prose. "Resolved with no edges" and "did not
/// resolve" must not share a representation.
///
/// **`the` over several targets is refused, not picked.** `the` asserts the
/// traversal is functional; if it is not, the pack has said something false
/// about its own topology. `targets_per_source: one` makes the claim
/// enforceable in the state.
///
/// `evaluate` became fallible to carry those two, threaded through
/// `dre-causal`, `dre-planner`, `dre-explain` and features. Degrading either
/// to `Unknown` would be the engine guessing precisely where it said it
/// would refuse.
///
/// A reach in a **goal** is refused at load. `goal_target` matches only
/// `Condition::Compare`, so a reach-shaped goal contributes zero estimated
/// distance and degrades A\* silently — an underestimating heuristic is
/// still admissible, so nothing downstream can tell.
///
/// A reach counts as **one leaf** in `specificity`. Counting hops would let
/// an author raise a rule's precedence by routing the same question through
/// a longer path.
///
/// Nothing existing moves: no committed pack contains a traversal, and the
/// forty-one prior experiments reproduce decision and proof unchanged.
/// **0.19.0**, 2026-08-16. An observation may declare that it is a negative
/// finding — something confirmed healthy, which refutes but is not an
/// explanatory achievement.
///
/// Found by the fifth independently authored pack, in the one experiment
/// where its author's prediction and the engine disagreed. **The author was
/// right.** A mechanism called `dependencies_clear` earned 1.75 of coverage
/// for accounting for `dns_ok`, `network_ok` and `tls_ok`, while the rival
/// account was charged 1.00 for leaving those same three "unexplained". So
/// an account was **rewarded for explaining non-problems and punished for
/// not explaining why the working parts work**, and the wrong hypothesis
/// won.
///
/// The observation set was conflating two things. A **symptom** is something
/// wrong that wants accounting for. A **negative finding** is something
/// confirmed healthy, whose entire diagnostic value is that it refutes —
/// `dns_ok` correctly contradicts `DNS` in the same run, and should not also
/// be creditable to whoever thinks to mention it.
///
/// `needs_explaining` defaults to **true**, so every pack written before
/// this reads exactly as it did. Set false, the observation earns no
/// coverage and incurs no unexplained penalty; contradiction is untouched.
///
/// Probably the same root as the networking author's separate complaint that
/// scoring "cannot encode specificity, so a generic routing-blackhole
/// account can tie a more specific STP-block account" — coverage rewarding
/// breadth over relevance. Two independent reports, two domains, one cause.
/// **0.20.0**, 2026-08-16. A traversal may require that **at least N** of
/// the entities it reaches satisfy the test.
///
/// The one thing the first author to use traversal against a real graph
/// asked for that was a genuine gap: *"the missing option was more like
/// `count` or `at_least: N`, because topology often cares about 'attached to
/// at least two networks'."* They worked around it with a scalar count
/// property that something else had to maintain.
///
/// Counted three-valued, like everything else here: `True` once enough
/// targets definitely satisfy it, `False` once too few possibly can, and
/// `Unknown` in between — so an undecided target that could still get you
/// there does not read as a failure.
///
/// `{at_least: 0}` is refused. It is true of everything and cannot be what
/// anybody meant.
///
/// Their other two requests are **not** built, and the reasons differ. Two-hop
/// traversal was already built and had never been tried — no pack in the
/// repository declared a path with more than one edge, and the brief only
/// ever showed one, so it is a documentation defect and the brief now shows
/// two. Concluding on a reached entity remains the deliberate limit: reach
/// writes to the anchor, and the workaround is to write the rule from the
/// other end with `direction: in`.
/// # 0.21.0 — a reached entity may be tested on more than one property
///
/// `Reach` gains `also`: further comparisons applied to the **same** reached
/// entity, conjoined with the main one before the collapse sees them.
///
/// Moves the version because it changes what the engine **accepts** and what
/// a pack using it **decides**. Packs that do not use it are byte-identical
/// and keep their `ModelHash`; `also` defaults to empty and an empty `also`
/// is exactly the 0.20.0 behaviour.
///
/// ## The domain, which took two experiments to establish
///
/// The eighth externally authored pack needed "at least one scoped VM that is
/// **both** degraded and exposed to host CPU saturation" and could only spell
/// it as two independent existentials over the same traversal. Its author
/// found the gap themselves, named the paired join that would close it, and
/// recorded that the format refuses one.
///
/// `vm-host-003-unpaired-aggregation` froze the defect and showed it was not
/// yet decisive: the false intermediate suppressed every account rather than
/// promoting one, and the engine correctly declined to conclude.
/// `vm-host-004-unpaired-promotes-a-winner` removed that protection, and the
/// account wins at 1.700000 on four supported observations with nothing
/// contradicting it — a confident wrong diagnosis, with the failing VMs on a
/// healthy host.
///
/// ## Why this is not the join that stays refused
///
/// It adds no binding axis. `bindings_for` still returns `Vec<Option<Id>>`
/// and `already_fired` is still keyed `(Id, Option<Id>)`, so activations stay
/// `rules x instances` and the termination argument is unchanged. The extra
/// work is per-target reads inside a reach that already loops its targets.
///
/// It cannot conclude about a reached entity, bind one for use elsewhere, or
/// pair two independently named types. Every objection recorded against the
/// two losing bound-variable designs still stands, and none of them is
/// answered here because none of them applies.
///
/// ## Conjunction, not a filter
///
/// A filter reads more naturally and introduces a set the author never
/// declared. `every` over an empty set is true, so a filter matching nothing
/// would make every universal traversal hold — the same hazard the
/// unresolved-anchor error exists to prevent one level up. Conjoining leaves
/// the collapse as the only thing deciding what a set of results means.
///
/// # 0.22.0 — an undeclared predicate is a defective reading
///
/// `defect_in` asked three questions of an incoming observation — kind, domain,
/// vocabulary — and reached them only after two `?` lookups. A miss on the
/// second returned `None`, which that function spells *no defect*, so a reading
/// naming a predicate the entity's type does not declare was admitted,
/// resolved, and written into state. `StateBuilder::set` does not consult the
/// ontology either, so nothing objected. The engine then reasoned over a
/// property the model never said could exist.
///
/// **The same shape as 0.7.0 and the same remedy.** That bump began rejecting
/// readings outside a declared domain; this one rejects readings against an
/// undeclared property. Both are the observation gate refusing input the
/// ontology does not admit, and both are reasoning changes rather than
/// validation features — hence a version bump and re-recorded baselines.
///
/// ⛔ ONLY THE PREDICATE ARM MOVED. An absent SUBJECT still returns `None`
/// there, deliberately: `Operator::UNKNOWN` on a subject that names nothing is
/// correct and `model-packs/process` depends on it. Collapsing the two misses
/// into one defect would have broken a shipped pack, which is why the arms are
/// written separately rather than as one guard.
///
/// **No stored decision is expected to move.** Measured across the corpus
/// before the change: 290 evidence rows in 58 experiments against 27
/// ontologies, and not one names an undeclared predicate. The bump is
/// therefore ceremony that costs a re-record and buys a closed hole — low
/// migration cost, which is not the same as low severity.
pub const ENGINE_VERSION: Version = Version::new(0, 22, 0);

/// Workload counters for a run.
///
/// These are reported, not consulted. A counter that fed back into a
/// decision would make the result depend on how much work preceded it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Counters {
    /// Inference steps performed.
    pub reasoning_steps: u64,
    /// Rules whose conditions were evaluated.
    pub rules_evaluated: u64,
    /// Rules whose conditions held and whose inferences were applied.
    pub rules_fired: u64,
    /// States expanded by the planner.
    pub states_expanded: u64,
    /// Nodes recorded into the proof graph.
    pub proof_nodes: u64,

    /// Declared geometric dimensions the subjects could actually be read
    /// on, summed over every entity classified.
    ///
    /// Plan section 20's "active dimensions". Counted whether or not
    /// classification went ahead: a space of three dimensions with two
    /// readable is the interesting case, and reporting zero because the
    /// classifier declined would hide it.
    ///
    /// A space names an entity *type*, so a pack with sixty instances and
    /// one dimension reports 60 rather than 1. That is the number which
    /// says every instance was read; a run that classified only one would
    /// report a plausible small figure and nothing else would differ.
    pub active_dimensions: u64,

    /// Properties that were `UNKNOWN` in the state as authored.
    ///
    /// The denominator of plan section 20's unknown-resolution rate.
    /// `UNOBSERVABLE` is deliberately excluded — it is a permanent
    /// property of the model, not a gap evidence could close, so counting
    /// it would make the rate look worse the more honest the pack was.
    pub unknown_initial: u64,

    /// How many of those were decided by the end of the run.
    pub unknown_resolved: u64,
}

impl Counters {
    /// Unknown-resolution rate in parts per thousand.
    ///
    /// Returns `None` when nothing started unknown, which is a different
    /// statement from "resolved none of them" and must not render as 0%.
    ///
    /// Per-mille integers rather than a ratio because floating point is
    /// denied workspace-wide. The counters are reported rather than
    /// reasoned over, so this is presentation — but keeping even the
    /// presentation in exact arithmetic means no path exists by which an
    /// `f64` could reach a hash.
    #[must_use]
    pub const fn unknown_resolution_permille(&self) -> Option<u64> {
        if self.unknown_initial == 0 {
            return None;
        }
        Some(self.unknown_resolved.saturating_mul(1_000) / self.unknown_initial)
    }
}

/// Per-run execution state.
#[derive(Clone, Debug)]
pub struct ExecutionContext {
    config: RuntimeConfig,
    engine_version: Version,
    counters: Counters,
}

impl ExecutionContext {
    /// Create a context, validating the configuration up front.
    pub fn new(config: RuntimeConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            engine_version: ENGINE_VERSION,
            counters: Counters::default(),
        })
    }

    /// Create a context with the MVP defaults.
    pub fn with_defaults() -> Result<Self> {
        Self::new(RuntimeConfig::default())
    }

    /// The configuration governing this run.
    #[must_use]
    pub const fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// The counters accumulated so far.
    #[must_use]
    pub const fn counters(&self) -> &Counters {
        &self.counters
    }

    /// The engine version.
    #[must_use]
    pub const fn engine_version(&self) -> Version {
        self.engine_version
    }

    /// The `EngineHash` for this run.
    ///
    /// Covers the engine version and the full configuration, so a run made
    /// under different ceilings is identifiably a different engine rather
    /// than an apparent determinism failure.
    #[must_use]
    pub fn engine_hash(&self) -> Hash32 {
        let mut writer = CanonicalWriter::new();
        writer
            .tag("engine")
            .write(&self.engine_version)
            .write(&self.config);
        Hash32::of(writer.as_bytes())
    }

    /// Record an inference step.
    pub fn charge_reasoning_step(&mut self) {
        self.counters.reasoning_steps = self.counters.reasoning_steps.saturating_add(1);
    }

    /// Record a rule evaluation.
    pub fn charge_rule_evaluated(&mut self) {
        self.counters.rules_evaluated = self.counters.rules_evaluated.saturating_add(1);
    }

    /// Record a rule firing.
    pub fn charge_rule_fired(&mut self) {
        self.counters.rules_fired = self.counters.rules_fired.saturating_add(1);
    }

    /// Record a proof node.
    pub fn charge_proof_node(&mut self) {
        self.counters.proof_nodes = self.counters.proof_nodes.saturating_add(1);
    }

    /// Record how many geometric dimensions a subject was readable on.
    ///
    /// Accumulated across subjects, because a run classifies every entity
    /// of the declared type rather than a single named one. The space is
    /// declared once; the readings are per entity, and the metric is the
    /// work done rather than the size of the declaration.
    ///
    /// This *set* the value until the space named a type instead of an
    /// entity. A pack with one instance is unaffected — one call, same
    /// number — which is why no stored decision or benchmark row moved.
    pub fn add_active_dimensions(&mut self, dimensions: u64) {
        self.counters.active_dimensions =
            self.counters.active_dimensions.saturating_add(dimensions);
    }

    /// Record the unknown-resolution tally for the run.
    pub fn note_unknown_resolution(&mut self, initial: u64, resolved: u64) {
        self.counters.unknown_initial = initial;
        self.counters.unknown_resolved = resolved.min(initial);
    }

    /// Record a planner state expansion, enforcing the `max_states` ceiling.
    ///
    /// Returns an error rather than truncating the search. A planner that
    /// quietly stopped early would return a valid-looking plan that is not
    /// the plan the model actually implies, and nothing downstream could
    /// tell the difference.
    pub fn charge_state_expansion(&mut self) -> Result<()> {
        let cap = self.config.limits.max_states;
        if self.counters.states_expanded >= cap {
            return Err(CoreError::LimitExceeded {
                limit: "max_states",
                cap,
            });
        }
        self.counters.states_expanded = self.counters.states_expanded.saturating_add(1);
        Ok(())
    }

    /// Check a search depth against the `max_depth` ceiling.
    pub fn check_depth(&self, depth: u64) -> Result<()> {
        let cap = self.config.limits.max_depth;
        if depth > cap {
            return Err(CoreError::LimitExceeded {
                limit: "max_depth",
                cap,
            });
        }
        Ok(())
    }

    /// Check a declared count against a ceiling.
    fn check_count(count: u64, limit: &'static str, cap: u64) -> Result<()> {
        if count > cap {
            return Err(CoreError::LimitExceeded { limit, cap });
        }
        Ok(())
    }

    /// Check a pack's rule count against `max_rules`.
    ///
    /// # Why these four exist
    ///
    /// `max_rules`, `max_dimensions`, `max_regions` and `max_actions` were
    /// declared, documented, folded into [`Self::engine_hash`] — and read
    /// by nothing. Half the resource ceilings this engine advertised were
    /// decorative, and the hash asserted a distinction between two engines
    /// that would in fact behave identically.
    ///
    /// That is the same defect this project has now found five times in
    /// other guises: declared, hashed, never read. The ceilings are the
    /// instance closest to the identity primitive itself, which is why
    /// they are enforced here rather than removed.
    pub fn check_rule_count(&self, count: u64) -> Result<()> {
        Self::check_count(count, "max_rules", self.config.limits.max_rules)
    }

    /// Check one space's dimension count against `max_dimensions`.
    ///
    /// Per space, not per pack: the limit bounds the arity of the distance
    /// computation, which is what actually costs anything.
    pub fn check_dimension_count(&self, count: u64) -> Result<()> {
        Self::check_count(count, "max_dimensions", self.config.limits.max_dimensions)
    }

    /// Check one space's region count against `max_regions`.
    pub fn check_region_count(&self, count: u64) -> Result<()> {
        Self::check_count(count, "max_regions", self.config.limits.max_regions)
    }

    /// Check a pack's action count against `max_actions`.
    ///
    /// Bounding the branching factor rather than the search: `max_states`
    /// already bounds how far the planner goes, but a pack with an
    /// enormous action set exhausts that ceiling in a single level and
    /// reports depth exhaustion for what is really a modelling mistake.
    pub fn check_action_count(&self, count: u64) -> Result<()> {
        Self::check_count(count, "max_actions", self.config.limits.max_actions)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn engine_hash_is_stable_across_contexts() {
        let left = ExecutionContext::with_defaults().expect("valid config");
        let right = ExecutionContext::with_defaults().expect("valid config");
        assert_eq!(left.engine_hash(), right.engine_hash());
    }

    #[test]
    fn engine_hash_ignores_accumulated_work() {
        let mut context = ExecutionContext::with_defaults().expect("valid config");
        let before = context.engine_hash();
        context.charge_rule_evaluated();
        context.charge_reasoning_step();
        // Counters describe the run, not the engine.
        assert_eq!(before, context.engine_hash());
    }

    #[test]
    fn state_expansion_stops_at_the_ceiling() {
        let mut config = RuntimeConfig::default();
        config.limits.max_states = 2;
        let mut context = ExecutionContext::new(config).expect("valid config");

        assert!(context.charge_state_expansion().is_ok());
        assert!(context.charge_state_expansion().is_ok());
        assert_eq!(
            context.charge_state_expansion(),
            Err(CoreError::LimitExceeded {
                limit: "max_states",
                cap: 2,
            })
        );
        assert_eq!(context.counters().states_expanded, 2);
    }

    #[test]
    fn no_unknowns_is_not_the_same_as_resolving_none() {
        let mut context = ExecutionContext::with_defaults().expect("valid config");
        assert_eq!(context.counters().unknown_resolution_permille(), None);

        context.note_unknown_resolution(4, 0);
        assert_eq!(context.counters().unknown_resolution_permille(), Some(0));
    }

    #[test]
    fn resolution_rate_is_exact_thirds() {
        // 2 of 3 is 666.66...%o. Integer division truncates, which is the
        // intended behaviour: the rate must never round *up* and suggest
        // more was resolved than actually was.
        let mut context = ExecutionContext::with_defaults().expect("valid config");
        context.note_unknown_resolution(3, 2);
        assert_eq!(context.counters().unknown_resolution_permille(), Some(666));
    }

    #[test]
    fn resolving_more_than_started_unknown_is_clamped() {
        // A miscount upstream should show as 100%, never as an impossible
        // rate that would look like a metric bug rather than a tally bug.
        let mut context = ExecutionContext::with_defaults().expect("valid config");
        context.note_unknown_resolution(2, 9);
        assert_eq!(context.counters().unknown_resolved, 2);
        assert_eq!(
            context.counters().unknown_resolution_permille(),
            Some(1_000)
        );
    }

    #[test]
    fn counters_stay_out_of_the_engine_hash() {
        let mut context = ExecutionContext::with_defaults().expect("valid config");
        let before = context.engine_hash();
        context.add_active_dimensions(3);
        context.note_unknown_resolution(4, 3);
        assert_eq!(before, context.engine_hash());
    }

    #[test]
    fn active_dimensions_accumulate_across_subjects() {
        // One space, many entities read into it. Setting rather than
        // adding would report the last subject's readable dimensions as
        // though it were the run's, which on a sixty-entity pack is off by
        // a factor of sixty and looks like a plausible small number.
        let mut context = ExecutionContext::with_defaults().expect("valid config");
        context.add_active_dimensions(2);
        context.add_active_dimensions(2);
        assert_eq!(context.counters().active_dimensions, 4);
    }

    #[test]
    fn depth_check_respects_the_ceiling() {
        let context = ExecutionContext::with_defaults().expect("valid config");
        assert!(context.check_depth(20).is_ok());
        assert!(context.check_depth(21).is_err());
    }
}
