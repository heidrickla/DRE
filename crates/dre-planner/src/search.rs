//! Deterministic search over action sequences.
//!
//! # Why the frontier ordering carries a state hash
//!
//! Textbook Dijkstra leaves expansion order unspecified among equal-cost
//! frontier entries, and `BinaryHeap` resolves it by heap layout, which
//! depends on insertion history. Grid navigation generates equal-cost
//! paths in bulk — that is the *normal* case, not a corner case — so two
//! runs could return different, equally optimal routes.
//!
//! Ordering by cost, then depth, then action identifier is still not
//! enough: two genuinely different states can be reached at the same cost
//! and depth by the same action name. So the final tie-break is the
//! canonical hash of the state itself, which differs whenever the states
//! do. The ordering is then total over distinct nodes, and expansion order
//! is a function of the problem rather than of the heap.
//!
//! Note this makes the *hash* semantically load-bearing rather than merely
//! diagnostic. It is an arbitrary but fixed total order — which is exactly
//! what is needed, since any deterministic choice will do so long as it is
//! the same one every time.

use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap};

use dre_causal::{applicable, apply, Action, Effect};
use dre_core::canonical::Canonical;
use dre_core::context::ExecutionContext;
use dre_core::fixed::Fixed;
use dre_core::hash::Hash32;
use dre_core::id::Id;
use dre_proof::{NodeKind, NodeRef, ProofNode, Trace};
use dre_state::{CanonicalState, Value};
use dre_symbolic::{evaluate, referenced_subjects, Condition, Operator, Truth};

use crate::{Constraint, Goal, Plan, Strategy, Undecidable};

/// Failures during search.
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    /// A bound was exceeded. Never a truncated answer.
    #[error(transparent)]
    Limit(#[from] dre_core::CoreError),

    /// A traversal in a goal or constraint could not be evaluated.
    #[error(transparent)]
    Eval(#[from] dre_symbolic::EvalError),

    /// Applying an action failed.
    #[error(transparent)]
    Apply(#[from] dre_causal::ApplyError),

    /// A\* was asked to search a model its heuristic cannot bound.
    ///
    /// Refused rather than run, because the failure is silent otherwise:
    /// the search returns a plan that looks perfectly ordinary and is
    /// simply not the cheapest one.
    #[error(
        "A* heuristic is not admissible for action {action}: it moves the goal {distance} units \
         at cost {cost}, so the Manhattan estimate can exceed the true remaining cost. \
         Use Dijkstra, or give the action a cost of at least {distance}."
    )]
    Inadmissible {
        /// The offending action.
        action: Id,
        /// Total goal-dimension movement it can produce in one step.
        distance: Fixed,
        /// What it charges for that movement.
        cost: Fixed,
    },
}

/// A frontier node.
#[derive(Clone, PartialEq, Eq)]
struct Node {
    priority: Fixed,
    cost: Fixed,
    depth: u64,
    last_action: Id,
    state_hash: Hash32,
    state: CanonicalState,
    path: Vec<Id>,
    /// Proof nodes for the steps along `path`, in the same order.
    ///
    /// Carried so the final decision can name the candidates it rests on.
    /// A decision that pointed only at its last action would claim the
    /// route was one step long.
    proof_path: Vec<NodeRef>,
}

impl Ord for Node {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| self.depth.cmp(&other.depth))
            .then_with(|| self.last_action.cmp(&other.last_action))
            .then_with(|| self.state_hash.cmp(&other.state_hash))
            // The path, last, because the frontier may now hold the same
            // state more than once — one entry per route that reached it.
            //
            // The four keys above do not separate those. Two routes can
            // arrive at one state with equal cost and depth by the same
            // final action whenever that action erases the difference
            // between their predecessors, which any `set` effect can do:
            // setting `x` to 5 from `x = 1` and from `x = 2` lands in the
            // same place. Without this the heap would order such entries
            // by layout, and layout depends on insertion history.
            //
            // The previous version never met the case, because settling a
            // state at generation time kept the second route off the
            // frontier entirely. It was deterministic partly *because* of
            // the bug that made it return suboptimal plans.
            .then_with(|| self.path.cmp(&other.path))
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// A target point derived from the goal's numeric equality conditions.
///
/// The goal defines both the coordinates and the axes, so no geometry
/// section is needed: `agent.x EQ 4` says there is a dimension `agent.x`
/// whose target is 4. Conditions that are not numeric equalities are
/// ignored — they still gate the goal test, they just cannot contribute a
/// distance.
fn goal_target(goal: &Goal) -> Vec<(Id, Fixed)> {
    let mut target = Vec::new();

    for condition in &goal.conditions {
        if let Condition::Compare {
            subject,
            operator: Operator::Eq,
            operand,
        } = condition
        {
            let coordinate = match operand {
                Value::Integer(value) => Fixed::from_int(*value).ok(),
                Value::Fixed(value) => Some(*value),
                _ => None,
            };
            if let Some(coordinate) = coordinate {
                target.push((subject.clone(), coordinate));
            }
        }
    }

    target.sort_by(|left, right| left.0.cmp(&right.0));
    target
}

/// Manhattan distance from a state to the goal target.
///
/// # Admissibility
///
/// A\* only returns optimal plans when the heuristic never overestimates.
/// This one is admissible exactly when no action moves the goal
/// dimensions further, in total, than it charges — see
/// [`check_admissible`], which the planner runs before any A\* search and
/// which refuses rather than quietly returning a suboptimal plan.
///
/// Note the deliberate choice of Manhattan over the weighted *squared*
/// distance `dre-geometry` provides. Squared distance is fine for ranking
/// regions, where only the order matters, but as a search heuristic it
/// overestimates badly — 32 against a true cost of 8 on a 4-by-4 diagonal
/// — and would break optimality immediately.
///
/// A property that is missing or not definitely known contributes nothing,
/// which keeps the estimate low rather than inventing a distance.
fn heuristic(target: &[(Id, Fixed)], state: &CanonicalState) -> Fixed {
    let mut total = Fixed::ZERO;

    for (subject, goal_value) in target {
        // Resolved the same way the goal check resolves it. They disagreed:
        // `satisfied` goes through `evaluate`, which resolves a sole-instance
        // type name, and this did its own bare lookup and did not. A goal
        // written `host.reboot_completed` against an instance `host-1` then
        // contributed zero estimated distance, so A* expanded like Dijkstra
        // and moved the ProofHash -- silently, because a heuristic that
        // underestimates is still admissible and nothing downstream can tell.
        let current = state
            .property(subject)
            .filter(|property| {
                matches!(
                    property.status,
                    dre_state::EpistemicStatus::Known | dre_state::EpistemicStatus::Inferred
                )
            })
            .and_then(|property| match property.value.as_ref() {
                Some(Value::Integer(value)) => Fixed::from_int(*value).ok(),
                Some(Value::Fixed(value)) => Some(*value),
                _ => None,
            });

        let Some(current) = current else { continue };

        let delta = match current.checked_sub(*goal_value) {
            Ok(delta) => delta,
            Err(_) => continue,
        };
        let magnitude = delta.checked_abs().unwrap_or(Fixed::ZERO);
        total = total.checked_add(magnitude).unwrap_or(total);
    }

    total
}

/// Check that the Manhattan heuristic cannot overestimate for this model.
///
/// # Why this is checked rather than documented
///
/// An inadmissible heuristic does not crash, produce a warning, or return
/// anything that looks wrong. A\* simply returns a plan that is valid,
/// reaches the goal, and is not the cheapest — and no downstream check can
/// tell, because "optimal" is not a property of a plan you can read off the
/// plan. The failure is invisible by construction, which makes it exactly
/// the kind that has to be caught at the boundary.
///
/// # The condition
///
/// The heuristic charges one unit of estimate per unit of distance across
/// the goal dimensions. So it never overestimates as long as no single
/// action buys more distance than it costs:
///
/// ```text
/// for each action:  sum over goal dimensions |delta| <= cost
/// ```
///
/// The sum matters, not the maximum. An action moving `x` and `y` by one
/// each at cost 1 covers two units of Manhattan distance for one unit of
/// cost, and the estimate would then be twice the truth on a diagonal.
///
/// Three cases and their treatment:
///
/// - [`Effect::Add`] — bounded movement, compared against cost as above.
/// - [`Effect::Set`] — unbounded: one action can close any distance, so no
///   finite cost makes the estimate safe. Rejected outright.
/// - [`Effect::Clear`] — returns the property to `UNKNOWN`, which the
///   heuristic then skips, so the estimate can only fall. Always safe.
///
/// Actions touching no goal dimension are irrelevant to the estimate and
/// pass regardless of cost.
pub fn check_admissible(actions: &[Action], goal: &Goal) -> Result<(), SearchError> {
    let target = goal_target(goal);
    if target.is_empty() {
        // Nothing numeric to estimate against: the heuristic is a constant
        // zero, which is admissible for any model at all.
        return Ok(());
    }

    // Sorted by identifier so that a model breaking the rule in several
    // places always reports the same one first. A diagnostic that named a
    // different action run to run would be its own small determinism bug.
    let mut ordered: Vec<&Action> = actions.iter().collect();
    ordered.sort_by(|left, right| left.id.cmp(&right.id));

    for action in ordered {
        let mut distance = Fixed::ZERO;

        for (dimension, _) in &target {
            let Some(effect) = action.effects.get(dimension) else {
                continue;
            };

            match effect {
                Effect::Clear => {}
                Effect::Add(delta) => {
                    let magnitude = delta.checked_abs().unwrap_or(Fixed::MAX);
                    distance = distance.checked_add(magnitude).unwrap_or(Fixed::MAX);
                }
                Effect::Set(_) => {
                    return Err(SearchError::Inadmissible {
                        action: action.id.clone(),
                        distance: Fixed::MAX,
                        cost: action.cost,
                    });
                }
            }
        }

        if distance > action.cost {
            return Err(SearchError::Inadmissible {
                action: action.id.clone(),
                distance,
                cost: action.cost,
            });
        }
    }

    Ok(())
}

/// Whether the goal holds in a state.
///
/// # Errors
///
/// Fails when a goal condition contains a traversal that cannot be
/// evaluated.
pub fn satisfied(goal: &Goal, state: &CanonicalState) -> Result<bool, SearchError> {
    let mut holds = Truth::True;
    for condition in &goal.conditions {
        holds = holds.and(evaluate(condition, state)?);
    }
    Ok(holds.is_true())
}

/// Whether a state violates any hard constraint.
///
/// What an *undecidable* constraint does is the constraint's own choice,
/// declared as `on_undecidable` and defaulting to `block`.
///
/// Both readings are needed, which is why neither is wired in here. A
/// safety invariant that permits when it cannot tell is not covering the
/// case it exists for; a procedural invariant that blocks when it cannot
/// tell prunes every candidate before the facts that would satisfy it are
/// established, and the planner reports no path where one exists.
fn violates_hard(
    constraints: &[Constraint],
    state: &CanonicalState,
) -> Result<Option<Id>, SearchError> {
    for constraint in constraints.iter().filter(|constraint| constraint.hard) {
        let mut holds = Truth::True;
        for condition in &constraint.conditions {
            holds = holds.and(evaluate(condition, state)?);
        }

        let violated = match holds {
            Truth::False => true,
            Truth::Unknown => constraint.on_undecidable == Undecidable::Block,
            Truth::True => false,
        };
        if violated {
            return Ok(Some(constraint.id.clone()));
        }
    }
    Ok(None)
}

/// Total penalty from violated soft constraints.
///
/// Undecidable does not count as violated, for the same reason it does not
/// for hard constraints: penalising a state the model never objected to
/// would bias the search on missing information.
fn soft_penalty(constraints: &[Constraint], state: &CanonicalState) -> Result<Fixed, SearchError> {
    let mut total = Fixed::ZERO;

    for constraint in constraints.iter().filter(|constraint| !constraint.hard) {
        let mut holds = Truth::True;
        for condition in &constraint.conditions {
            holds = holds.and(evaluate(condition, state)?);
        }
        if holds == Truth::False {
            total = total.checked_add(constraint.penalty).unwrap_or(total);
        }
    }

    Ok(total)
}

/// Search for a plan from `initial` to `goal`.
///
/// Returns `Ok(None)` when the goal is unreachable within the bounds —
/// distinct from an error, which means a bound was hit and the search was
/// abandoned rather than exhausted.
pub fn plan(
    strategy: Strategy,
    actions: &[Action],
    goal: &Goal,
    constraints: &[Constraint],
    initial: CanonicalState,
    context: &mut ExecutionContext,
    trace: &mut Trace,
) -> Result<Option<Plan>, SearchError> {
    // Expansion considers actions in identifier order, so the successor
    // sequence is fixed by the pack rather than by file order.
    let mut ordered: Vec<&Action> = actions.iter().collect();
    ordered.sort_by(|left, right| left.id.cmp(&right.id));

    // Checked before the search rather than after, and before the trivial
    // goal case, so that a model whose heuristic does not hold is reported
    // even when it happens to start at the goal.
    if matches!(strategy, Strategy::AStar) {
        check_admissible(actions, goal)?;
    }

    let target = goal_target(goal);

    if satisfied(goal, &initial)? {
        return Ok(Some(Plan {
            actions: Vec::new(),
            cost: Fixed::ZERO,
        }));
    }

    let root_hash = initial.canonical_hash();

    // States whose cheapest cost is now final.
    //
    // Settled on *pop*, not on generation, and the difference is the whole
    // correctness of the search. Marking a state seen the moment it is
    // first generated assumes the first route to reach it is the cheapest
    // one, which in a uniform-cost search is simply untrue: a state can be
    // generated early by one expensive action and reached later by a
    // cheaper sequence, and the cheaper arrival is then discarded as
    // already seen. The result is a plan that is valid, reaches the goal,
    // and is not the cheapest — with nothing downstream able to tell.
    //
    // Every pack written alongside this engine used unit-cost actions, and
    // with uniform costs generation order tracks cost order exactly, so the
    // bug was invisible. It took an independently authored pack with costs
    // of 1, 2, 3 and 5 to expose it. See `tests/cheapest_path.rs`.
    //
    // Duplicates are therefore allowed onto the frontier and skipped when
    // popped, which is the standard lazy-deletion form of Dijkstra. The
    // alternative — a decrease-key heap — buys memory rather than
    // correctness and costs a data structure this does not need.
    let mut settled: BTreeSet<Hash32> = BTreeSet::new();

    let mut frontier: BinaryHeap<Reverse<Node>> = BinaryHeap::new();
    frontier.push(Reverse(Node {
        priority: Fixed::ZERO,
        cost: Fixed::ZERO,
        depth: 0,
        last_action: Id::new("__start").unwrap_or_else(|_| unreachable!("valid literal")),
        state_hash: root_hash,
        state: initial,
        path: Vec::new(),
        proof_path: Vec::new(),
    }));

    while let Some(Reverse(node)) = frontier.pop() {
        // A state can sit on the frontier several times, once per route
        // that reached it. The first pop is its cheapest arrival — that is
        // what ordering the frontier by cost means — so settling here and
        // discarding the rest is both correct and where the previous
        // version went wrong by settling at generation instead.
        //
        // Skipped before the expansion is charged, so a duplicate does not
        // consume budget it never spent.
        if !settled.insert(node.state_hash) {
            continue;
        }

        // Goal-test on *pop*, not on generation.
        //
        // Testing when a successor is generated returns the first route
        // that happens to touch the goal, which is only optimal when every
        // action costs the same. With varying costs it can return a
        // cheap-looking path that a more expensive first step would have
        // beaten. Testing on pop returns the goal state with the lowest
        // priority in the frontier, which is the textbook guarantee for
        // both Dijkstra and A*.
        //
        // It also lets the heuristic do its job: generation-time testing
        // ends the search before A* has had a chance to steer past the
        // states Dijkstra would have explored.
        if satisfied(goal, &node.state)? {
            if !node.path.is_empty() {
                trace.record(ProofNode {
                    kind: NodeKind::Decision {
                        candidate: node.last_action.clone(),
                    },
                    // The whole route, not just its last step. A plan is
                    // the sequence, so its justification is every action
                    // that got there.
                    inputs: node.proof_path.clone(),
                });
                context.charge_proof_node();
            }
            return Ok(Some(Plan {
                actions: node.path,
                cost: node.cost,
            }));
        }

        context.charge_state_expansion()?;

        for action in &ordered {
            if !applicable(action, &node.state)? {
                continue;
            }

            let next_state = apply(action, node.state.clone(), context.config().limits)?;
            let next_hash = next_state.canonical_hash();

            // Already final: no route found from here can beat the one it
            // was settled with. Anything not yet settled is offered to the
            // frontier even if it is already queued, because this arrival
            // may be the cheaper one.
            if settled.contains(&next_hash) {
                continue;
            }

            if let Some(violated) = violates_hard(constraints, &next_state)? {
                trace.record(ProofNode {
                    kind: NodeKind::Constraint {
                        constraint: violated,
                        passed: false,
                    },
                    // The step that would have entered the forbidden state.
                    inputs: node.proof_path.clone(),
                });
                context.charge_proof_node();
                continue;
            }

            let depth = node.depth + 1;
            context.check_depth(depth)?;

            let cost = match strategy {
                // Breadth-first ignores declared cost and counts steps, so
                // its notion of "best" is shallowest rather than cheapest.
                Strategy::BreadthFirst => {
                    Fixed::from_int(i64::try_from(depth).unwrap_or(i64::MAX)).unwrap_or(Fixed::MAX)
                }
                Strategy::Dijkstra | Strategy::AStar => {
                    node.cost.checked_add(action.cost).unwrap_or(Fixed::MAX)
                }
            };

            // Soft violations are paid for rather than forbidden. Charged
            // on entering the state, so a route that passes through a
            // hazard pays once per crossing.
            let penalty = soft_penalty(constraints, &next_state)?;
            if penalty > Fixed::ZERO {
                trace.record(ProofNode {
                    kind: NodeKind::Constraint {
                        constraint: action.id.clone(),
                        passed: false,
                    },
                    inputs: node.proof_path.clone(),
                });
                context.charge_proof_node();
            }
            let cost = cost.checked_add(penalty).unwrap_or(Fixed::MAX);

            // A* orders the frontier by cost-so-far plus estimated
            // remaining. Dijkstra is the same search with a zero
            // heuristic, which is why they share everything else.
            let priority = match strategy {
                Strategy::AStar => cost
                    .checked_add(heuristic(&target, &next_state))
                    .unwrap_or(Fixed::MAX),
                Strategy::BreadthFirst | Strategy::Dijkstra => cost,
            };

            let mut path = node.path.clone();
            path.push(action.id.clone());

            // A step depends on the step before it, and on whatever
            // established the properties its preconditions read. The first
            // gives the route its shape; the second is what connects a plan
            // back to the observations that made it applicable.
            let mut inputs = trace.sources_of(referenced_subjects(&action.preconditions).iter());
            if let Some(previous) = node.proof_path.last() {
                inputs.push(*previous);
            }
            inputs.sort_unstable();
            inputs.dedup();

            let candidate = trace.record(ProofNode {
                kind: NodeKind::Candidate {
                    candidate: action.id.clone(),
                },
                inputs,
            });
            context.charge_proof_node();

            let mut proof_path = node.proof_path.clone();
            proof_path.push(candidate);

            frontier.push(Reverse(Node {
                priority,
                cost,
                depth,
                last_action: action.id.clone(),
                state_hash: next_hash,
                state: next_state,
                path,
                proof_path,
            }));
        }
    }

    Ok(None)
}

/// The `goals.yaml` section of a model pack.
#[derive(Clone, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalsSection {
    /// Declared goals. The first is the one planned toward.
    #[serde(default)]
    pub goals: Vec<Goal>,
}

/// The `constraints.yaml` section of a model pack.
#[derive(Clone, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConstraintsSection {
    /// Declared constraints.
    #[serde(default)]
    pub constraints: Vec<Constraint>,
}
