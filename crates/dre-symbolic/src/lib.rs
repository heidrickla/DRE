//! Phase 4 — Symbolic reasoning.
//!
//! Deterministic forward-chaining inference over canonical state.
//!
//! Rule ordering is the whole determinism story for this crate. Production
//! rule systems classically resolve conflicts by recency or by whichever
//! rule the matcher reached first, both of which depend on internal data
//! structure ordering. Here the order is total and declared:
//!
//! ```text
//! priority ASC, specificity DESC, rule_id ASC
//! ```
//!
//! Because `rule_id` is unique, the comparison never falls through to an
//! arbitrary tie-break — two distinct rules always have a defined order,
//! whatever the model author wrote.
//!
//! # Two ways to derive a fact
//!
//! A rule concludes a *literal*: [`Inference`] carries a `Value`, not a
//! computation. That is deliberate — it keeps rule firing decidable and
//! keeps the proof readable — but it means a pack cannot say "efficiency
//! is output over input" at all.
//!
//! [`features`] is the other half: arithmetic over properties, evaluated
//! before rules run, so a rule can then reason about a derived quantity as
//! if it had been measured. The two share condition evaluation and the
//! three-valued discipline; they differ only in whether the conclusion is
//! written down or computed.

pub mod eval;
pub mod features;

pub use eval::{evaluate, forward_chain, referenced_subjects, EvalError, Outcome, Truth};
pub use features::{derive_all, Derived, Expression, Feature, FeatureError, FeaturesSection};

use std::cmp::Ordering;

use dre_core::id::Id;
use dre_state::Value;
use serde::{Deserialize, Serialize};

/// Comparison and logical operators, per plan section 10.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Operator {
    /// Equal.
    Eq,
    /// Not equal.
    Ne,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Gte,
    /// Less than.
    Lt,
    /// Less than or equal.
    Lte,
    /// The subject exists in state.
    Exists,
    /// The subject's status is `KNOWN`.
    Known,
    /// The subject's status is `UNKNOWN`.
    Unknown,
}

impl Operator {
    /// Whether this operator asks about a property's status rather than
    /// its value, and so takes no operand.
    #[must_use]
    pub const fn tests_status(self) -> bool {
        matches!(self, Self::Exists | Self::Known | Self::Unknown)
    }
}

/// Which way along a declared relationship a hop travels.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// From the edge's `from` endpoint to its `to`.
    #[default]
    Out,
    /// The reverse.
    In,
}

/// One step along a declared relationship type.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hop {
    /// The relationship type traversed.
    pub edge: Id,
    /// Which way. Defaults to `out`.
    #[serde(default)]
    pub direction: Direction,
}

/// A traversal from an anchor entity to a property on the entities it
/// reaches.
///
/// `path` is a list of structured hops rather than a dotted string inside an
/// `Id`, and that is deliberate. `Id`'s charset is justified as safe to embed
/// in canonical encodings without escaping — but decisively, a path smuggled
/// into an `Id` has near-miss spellings that are *legal* `Id`s.
/// `unit>mounts.active` parses, splits at the last dot, matches no instance
/// and no type, and reads `UNKNOWN` forever. That is precisely the failure
/// mode 0.15.0 cost three independent authors: loads, validates clean,
/// concludes nothing. A structured block under `deny_unknown_fields` makes
/// every typo a parse error instead.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reach {
    /// The anchor. An entity instance or a type, resolved by exactly the
    /// same rule as any other subject's entity half.
    pub from: Id,
    /// Hops applied in order.
    pub path: Vec<Hop>,
    /// The property read on the entities finally reached.
    pub read: Id,
    /// Further tests on the **same** reached entity, conjoined with the main
    /// comparison before the collapse sees it.
    ///
    /// Empty by default, which is exactly the behaviour before 0.21.0.
    #[serde(default)]
    pub also: Vec<TargetTest>,
}

/// One more test applied to a reached entity, beside the main comparison.
///
/// # What this is for
///
/// A rule may need to know that **one** reached entity satisfies several
/// things at once. Without this, the nearest available spelling is two
/// separate existential conditions over the same traversal — and two
/// independent `some` quantifiers do not assert anything about a single
/// entity. An externally authored pack shipped exactly that shape: a rule
/// named "at least one scoped **degraded** VM is exposed to host CPU
/// saturation" whose conditions established only that some VM was degraded
/// and some VM was exposed. `experiments/vm-host-004-unpaired-promotes-a-winner.yaml`
/// is that rule concluding a confident wrong diagnosis, with the failing VMs
/// on a healthy host and the one exposed VM perfectly well.
///
/// # Why conjunction and not a filter
///
/// A filter — "of the targets that pass this test, does the main one hold" —
/// looks more natural and introduces a set the author never declared. `every`
/// over an empty set is **true**, so a filter matching nothing would make
/// every universal traversal hold, which is the same hazard
/// [`crate::eval::EvalError::UnresolvedReachAnchor`] exists to prevent one
/// level up.
///
/// So this conjoins instead. Each reached entity contributes
/// `main AND every test here`, and the collapse still decides what a set of
/// those means. There is no hidden set and no hidden quantifier.
///
/// # What it deliberately cannot do
///
/// Read a property on anything but the entity reached, conclude about that
/// entity, or bind it for use elsewhere. It is a paired aggregate over one
/// traversal, not a join — the objections that refused a bound second
/// variable are recorded in `docs/decisions.md` and none of them is answered
/// by this.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetTest {
    /// The property read on the reached entity.
    pub read: Id,
    /// How it is compared.
    pub operator: Operator,
    /// What it is compared against. Absent for the status operators.
    #[serde(default)]
    pub operand: Option<Value>,
}

/// How a set of reached values becomes one answer.
///
/// **Mandatory, with no default.** This is the whole point of the design: a
/// rule spanning two entity types is refused because choosing cross-product
/// or pairwise silently "would make an answer depend on a decision nobody
/// wrote down". A traversal has the same problem one layer down — several
/// targets, one truth needed — and the resolution is that the author has to
/// write the decision down. There is no spelling of a reach that omits it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Collapse {
    /// Exactly one target must be reached; several is an error.
    The,
    /// Holds when it holds of every target. Vacuously true over none —
    /// which is why an anchor that fails to resolve must be an error rather
    /// than an empty set.
    Every,
    /// Holds when it holds of at least one target.
    Some,
    /// Holds when it holds of at least this many targets.
    ///
    /// Asked for by the first author to use traversal against a real graph:
    /// *"the missing option was more like `count` or `at_least: N`, because
    /// topology often cares about 'attached to at least two networks'."*
    /// They worked around it with a scalar `attached_network_count` property
    /// that something else had to maintain.
    ///
    /// Written `collapse: {at_least: 2}`. `at_least: 1` means what `some`
    /// means; prefer `some`, which says it in a word. Zero is refused —
    /// "at least none of them" is true of everything and cannot be what
    /// anybody meant.
    ///
    /// Note what this is **not**: it counts targets that satisfy the test,
    /// not targets reached. Counting the reach itself would be a different
    /// question — "how many mounts does this unit have" — and that is a
    /// property of the topology rather than a truth about it.
    AtLeast(u32),
}

/// A condition within a rule.
///
/// Untagged, so a pack writes conditions as plain mappings rather than
/// naming the variant:
///
/// ```yaml
/// if:
///   - subject: machine.temperature   # Compare
///     operator: GT
///     operand: 800
///   - subject: machine.vibration     # Status
///     operator: KNOWN
///   - any:                           # Any
///       - subject: machine.pressure
///         operator: LT
///         operand: 100
/// ```
///
/// Serialization is untagged, so a condition round-trips to the same
/// plain mapping an author wrote. **Deserialization is not** — see the
/// hand-written `Deserialize` below for why an untagged read of this type
/// is unsafe.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(untagged)]
pub enum Condition {
    /// Compare a property against a literal.
    Compare {
        /// Property being tested.
        subject: Id,
        /// Operator applied.
        operator: Operator,
        /// Literal compared against.
        operand: Value,
    },
    /// Test a property's epistemic status.
    Status {
        /// Property being tested.
        subject: Id,
        /// One of `EXISTS`, `KNOWN`, `UNKNOWN`.
        operator: Operator,
    },
    /// All nested conditions hold.
    All {
        /// The conjuncts.
        all: Vec<Condition>,
    },
    /// Any nested condition holds.
    Any {
        /// The disjuncts.
        any: Vec<Condition>,
    },
    /// The nested condition does not hold.
    Not {
        /// The negated condition.
        not: Box<Condition>,
    },
    /// Compare a property reached along declared relationships.
    ///
    /// The second entity type is **reached, never bound**. The author never
    /// writes the target type name — it comes from the relationship's
    /// declaration — so a rule still binds exactly one instance of exactly
    /// one type, `bindings_for` still sees one type, and
    /// `EvalError::MultipleTypes` still fires for a genuine two-type rule.
    /// The standing refusal stays true word for word.
    Reach {
        /// What to traverse and read.
        reach: Reach,
        /// How several reached values become one answer.
        collapse: Collapse,
        /// The comparison applied to each reached value.
        operator: Operator,
        /// The literal compared against; absent iff the operator tests
        /// status.
        operand: Option<Value>,
    },
}

/// The fields any condition may carry, before deciding which one it is.
///
/// `deny_unknown_fields` is load-bearing rather than tidy: it is what
/// turns a misspelled key into an error instead of a condition quietly
/// missing a clause.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConditionFields {
    subject: Option<Id>,
    operator: Option<Operator>,
    operand: Option<Value>,
    all: Option<Vec<Condition>>,
    any: Option<Vec<Condition>>,
    not: Option<Box<Condition>>,
    reach: Option<Reach>,
    collapse: Option<Collapse>,
}

/// Read a condition by looking at which keys are present.
///
/// # Why this is not `#[serde(untagged)]`
///
/// It was, and the failure it produced is the reason this code exists.
///
/// An untagged enum tries each variant in turn and **discards the error**
/// from every one that fails. `Compare` and `Status` differ only in that
/// `Compare` also carries an `operand`, and serde ignores unknown fields
/// by default — so a `Compare` whose operand failed to parse did not fail.
/// It fell through to `Status`, which accepted the same mapping and simply
/// dropped the operand.
///
/// The result was a condition that changed meaning during parsing. An
/// author writing
///
/// ```yaml
/// - subject: artifact.custody_gap
///   operator: GT
///   operand: 0.0          # a float, which `Value` rejects on purpose
/// ```
///
/// got a *status test* on `custody_gap` instead of a comparison. It
/// loaded, it validated clean — there was no operand left to find fault
/// with — and the rule never fired. An independently authored pack lost
/// two rules and three conclusions to this, and nothing reported anything.
///
/// Dispatching on the keys present means a malformed `Compare` is a
/// malformed `Compare`, and says so.
impl<'de> Deserialize<'de> for Condition {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;

        let fields = ConditionFields::deserialize(deserializer)?;

        // The combining forms first: each is identified by one key, and
        // none of them may also carry a comparison.
        let combiners = usize::from(fields.all.is_some())
            + usize::from(fields.any.is_some())
            + usize::from(fields.not.is_some());
        // A `reach` counts as a comparison here. Leaving it out meant a
        // condition carrying both `all` and `reach` returned the `all` and
        // silently dropped the traversal, which contradicts this function
        // claiming that everything structural is checked in the loader.
        let compares =
            fields.subject.is_some() || fields.operator.is_some() || fields.reach.is_some();

        if combiners > 1 {
            return Err(D::Error::custom(
                "a condition takes exactly one of `all`, `any` or `not`",
            ));
        }
        if combiners == 1 && compares {
            return Err(D::Error::custom(
                "a condition is a comparison, a traversal, or a combination of \
                 conditions -- not more than one of them",
            ));
        }

        if let Some(all) = fields.all {
            return Ok(Self::All { all });
        }
        if let Some(any) = fields.any {
            return Ok(Self::Any { any });
        }
        if let Some(not) = fields.not {
            return Ok(Self::Not { not });
        }

        // A reach is a comparison whose left-hand side is a traversal rather
        // than a subject. Everything structural about it is checked here, in
        // the loader, rather than in `dre validate` — validate is a separate
        // pass with no path into a decision, so a check that lives only there
        // is not a refusal.
        if let Some(reach) = fields.reach {
            if fields.subject.is_some() {
                return Err(D::Error::custom(
                    "a condition reads either a `subject` or a `reach`, not both",
                ));
            }
            if reach.path.is_empty() {
                return Err(D::Error::custom(
                    "a `reach` needs at least one hop in `path`",
                ));
            }
            if let Some(Collapse::AtLeast(0)) = fields.collapse {
                return Err(D::Error::custom(
                    "`collapse: {at_least: 0}` is true of everything; use a                      positive count, or `some` for one",
                ));
            }
            let Some(collapse) = fields.collapse else {
                return Err(D::Error::custom(
                    "a `reach` must say how several reached values become one \
                     answer: `collapse: the`, `every` or `some`",
                ));
            };
            let Some(operator) = fields.operator else {
                return Err(D::Error::custom("a `reach` needs an `operator`"));
            };
            return match (operator.tests_status(), fields.operand) {
                (true, None) => Ok(Self::Reach {
                    reach,
                    collapse,
                    operator,
                    operand: None,
                }),
                (true, Some(_)) => Err(D::Error::custom(format!(
                    "operator {operator:?} tests status and takes no `operand`"
                ))),
                (false, Some(operand)) => Ok(Self::Reach {
                    reach,
                    collapse,
                    operator,
                    operand: Some(operand),
                }),
                (false, None) => Err(D::Error::custom(format!(
                    "operator {operator:?} compares against an `operand`, which is missing"
                ))),
            };
        }
        if fields.collapse.is_some() {
            return Err(D::Error::custom(
                "`collapse` only means something alongside a `reach`",
            ));
        }

        let (Some(subject), Some(operator)) = (fields.subject, fields.operator) else {
            return Err(D::Error::custom(
                "a condition needs `subject` and `operator`, or one of `all`, `any`, `not`",
            ));
        };

        // An operator that tests status takes no operand, and a comparison
        // requires one. Saying which is expected costs nothing here and
        // saves an author guessing.
        match (operator.tests_status(), fields.operand) {
            (true, None) => Ok(Self::Status { subject, operator }),
            (true, Some(_)) => Err(D::Error::custom(format!(
                "operator {operator:?} tests status and takes no `operand`"
            ))),
            (false, Some(operand)) => Ok(Self::Compare {
                subject,
                operator,
                operand,
            }),
            (false, None) => Err(D::Error::custom(format!(
                "operator {operator:?} compares against an `operand`, which is missing"
            ))),
        }
    }
}

/// A value a rule asserts when its conditions hold.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inference {
    /// Property being inferred.
    pub subject: Id,
    /// Value asserted.
    pub value: Value,
}

/// A deterministic inference rule.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// Unique identifier. Also the final tie-break, so it must be unique
    /// within a pack for the ordering to be total.
    pub id: Id,
    /// Conditions that must hold. Spelled `if` in a pack.
    #[serde(rename = "if", default)]
    pub conditions: Vec<Condition>,
    /// Values asserted when they do. Spelled `infer` in a pack.
    #[serde(rename = "infer", default)]
    pub inferences: Vec<Inference>,
    /// Lower fires earlier.
    #[serde(default)]
    pub priority: i64,
    /// Human-facing note. Carried so `dre validate` and proof rendering
    /// can quote the author's own words rather than only an identifier.
    #[serde(default)]
    pub description: String,
}

/// The `rules.yaml` section of a model pack.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulesSection {
    /// Declared rules, in file order. Firing order is computed, not this.
    #[serde(default)]
    pub rules: Vec<Rule>,
}

impl Condition {
    /// Whether this condition, or anything nested in it, is a traversal.
    ///
    /// Used to refuse a reach where the consumer cannot honour it. A goal is
    /// the case that matters: the planner's heuristic reads goal conditions
    /// by matching `Condition::Compare` only, so a reach-shaped goal
    /// contributes zero estimated distance and degrades A\* to Dijkstra
    /// silently — an underestimating heuristic is still admissible, so
    /// nothing downstream can tell. Refused at load rather than in
    /// `dre validate`, because validate is a separate pass with no path into
    /// a decision.
    #[must_use]
    pub fn contains_reach(&self) -> bool {
        match self {
            Self::Reach { .. } => true,
            Self::Compare { .. } | Self::Status { .. } => false,
            Self::All { all: nested } | Self::Any { any: nested } => {
                nested.iter().any(Self::contains_reach)
            }
            Self::Not { not } => not.contains_reach(),
        }
    }
}

impl Rule {
    /// How specific this rule is, used as the second ordering key.
    ///
    /// Defined as the number of leaf conditions, so a rule constraining
    /// more of the state is *considered before* a broader one at equal
    /// priority, without the author hand-assigning priorities that stay
    /// consistent as the pack grows.
    ///
    /// **It orders firing. It does not override.** This comment used to say
    /// specificity "makes specific rules override general defaults", and
    /// that is false: both rules fire, and two inferences of different
    /// values for one property merge to `CONFLICTING` like any other
    /// disagreement. Overriding is what 0.14.0's declared value precedence
    /// is for, and it is declared on the value domain rather than inferred
    /// from how many conditions a rule happens to have.
    ///
    /// The false claim survived because nothing exercised it.
    /// `model-packs/process` reads like a demonstration and is not: R003
    /// requires `degraded UNKNOWN` and R004 requires `degraded EQ true`, so
    /// they are mutually exclusive and only one ever fires. See
    /// `specificity_orders_firing_and_does_not_override`, which is the test
    /// that should have existed.
    #[must_use]
    pub fn specificity(&self) -> u32 {
        fn count(condition: &Condition) -> u32 {
            match condition {
                // A reach is one leaf however far it travels. Counting hops
                // would let an author raise a rule's precedence by routing
                // the same question through a longer path, which is a knob
                // on rule ordering dressed up as a modelling choice.
                Condition::Compare { .. } | Condition::Status { .. } | Condition::Reach { .. } => 1,
                Condition::All { all: nested } | Condition::Any { any: nested } => {
                    nested.iter().map(count).sum()
                }
                Condition::Not { not: inner } => count(inner),
            }
        }
        self.conditions.iter().map(count).sum()
    }

    /// The total firing order: priority ASC, specificity DESC, id ASC.
    #[must_use]
    pub fn firing_order(&self, other: &Self) -> Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| other.specificity().cmp(&self.specificity()))
            .then_with(|| self.id.cmp(&other.id))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    fn rule(id: &str, priority: i64, condition_count: usize) -> Rule {
        let conditions = (0..condition_count)
            .map(|index| Condition::Status {
                subject: Id::new(format!("p{index}")).expect("valid id"),
                operator: Operator::Known,
            })
            .collect();
        Rule {
            id: Id::new(id).expect("valid id"),
            conditions,
            inferences: Vec::new(),
            priority,
            description: String::new(),
        }
    }

    fn ordered(mut rules: Vec<Rule>) -> Vec<String> {
        rules.sort_by(Rule::firing_order);
        rules.iter().map(|r| r.id.to_string()).collect()
    }

    #[test]
    fn lower_priority_fires_first() {
        let rules = vec![rule("R002", 200, 1), rule("R001", 100, 1)];
        assert_eq!(ordered(rules), ["R001", "R002"]);
    }

    #[test]
    fn more_specific_wins_at_equal_priority() {
        let general = rule("R001", 100, 1);
        let specific = rule("R002", 100, 3);
        assert_eq!(ordered(vec![general, specific]), ["R002", "R001"]);
    }

    #[test]
    fn identifier_breaks_remaining_ties() {
        let rules = vec![rule("R003", 100, 2), rule("R001", 100, 2)];
        assert_eq!(ordered(rules), ["R001", "R003"]);
    }

    #[test]
    fn ordering_is_independent_of_input_order() {
        let forward = vec![
            rule("R001", 100, 2),
            rule("R002", 100, 2),
            rule("R003", 50, 1),
        ];
        let reverse = vec![
            rule("R003", 50, 1),
            rule("R002", 100, 2),
            rule("R001", 100, 2),
        ];
        assert_eq!(ordered(forward), ordered(reverse));
    }

    #[test]
    fn specificity_counts_nested_leaves() {
        let nested = Rule {
            id: Id::new("R100").expect("valid id"),
            conditions: vec![Condition::All {
                all: vec![
                    Condition::Status {
                        subject: Id::new("a").expect("valid id"),
                        operator: Operator::Known,
                    },
                    Condition::Not {
                        not: Box::new(Condition::Status {
                            subject: Id::new("b").expect("valid id"),
                            operator: Operator::Unknown,
                        }),
                    },
                ],
            }],
            inferences: Vec::new(),
            priority: 0,
            description: String::new(),
        };
        assert_eq!(nested.specificity(), 2);
    }
}
