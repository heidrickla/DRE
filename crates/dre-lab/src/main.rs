//! Phase 12 — DRE Lab CLI.
//!
//! Runs experiments, replays them, benchmarks packs, and validates pack
//! structure. A web UI is explicitly deferred (plan section 18).
//!
//! Reasoning lives in `dre_lab`'s library half; this file is argument
//! parsing and output formatting only.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use dre_core::context::{ExecutionContext, ENGINE_VERSION};
use dre_lab::runner;

/// Deterministic Reasoning Engine — experiment lab.
#[derive(Debug, Parser)]
#[command(name = "dre", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run an experiment and report its decision and hashes.
    Run {
        /// Path to the experiment definition.
        experiment: PathBuf,
        /// Print the full proof trace.
        #[arg(long)]
        proof: bool,
        /// Print only the steps the final decision rested on.
        #[arg(long)]
        why: bool,
        /// Store this run's hashes as the replay baseline.
        #[arg(long)]
        record: bool,
        /// Note to store alongside the baseline.
        #[arg(long, default_value = "")]
        note: String,
    },
    /// Ask what the engine would have decided had one thing been different.
    ///
    /// Runs the experiment twice — once as written, once with the given
    /// properties changed — and reports whether the decision moved and
    /// which rules stopped or started firing.
    ///
    /// Adds no reasoning: both halves are ordinary runs, so nothing here
    /// can conclude anything `dre run` could not.
    Counterfactual {
        /// Path to the experiment definition.
        experiment: PathBuf,
        /// `entity.property=value`, repeatable. The value is parsed as
        /// YAML by the same strict reader a pack goes through, so
        /// `true`, `900` and `{fixed: "0.5"}` all mean what they mean
        /// everywhere else.
        #[arg(long = "set", value_name = "SUBJECT=VALUE")]
        set: Vec<String>,
        /// `entity.property`, repeatable. Returns the property to
        /// UNKNOWN — "what would it have decided without this reading?"
        #[arg(long = "unset", value_name = "SUBJECT")]
        unset: Vec<String>,
        /// Print every property that ended up different, not just the
        /// count.
        #[arg(long)]
        full: bool,
    },
    /// Rank the experiment's unknowns by what measuring one would change.
    ///
    /// Runs a counterfactual for every value the ontology admits for every
    /// unknown property, and reports which observations could move the
    /// decision and which could not. Numerics are probed at the thresholds
    /// the pack itself declares. Unknowns with nothing to probe are declined
    /// by name, with the reason, rather than skipped.
    Survey {
        /// Path to the experiment definition.
        experiment: PathBuf,
    },
    /// Rank the pack's declared hypotheses against what was observed.
    ///
    /// "Which account best explains this state?", which is a different
    /// question from "what should I do about it?" and is answered by a
    /// different layer. Reports every hypothesis with the components of its
    /// score, not only the winner, because "why not the other one" is what
    /// a diagnosis is judged on.
    Explain {
        /// Path to the experiment definition.
        experiment: PathBuf,
    },
    /// Re-run an experiment and verify it reproduces its stored record.
    Replay {
        /// Path to the experiment definition.
        experiment: PathBuf,
        /// Compare two fresh runs instead of the stored baseline.
        ///
        /// Weaker: it proves the engine agrees with itself now, not that
        /// it still agrees with what was recorded.
        #[arg(long)]
        self_only: bool,
    },
    /// Run an experiment repeatedly and report unique hash counts.
    Benchmark {
        /// Path to the experiment definition.
        experiment: PathBuf,
        /// Iterations to run.
        #[arg(long, default_value_t = 1_000)]
        iterations: u32,
        /// Do not append a row to the benchmark archive.
        ///
        /// Archiving is on by default: a history only accumulates if
        /// recording is the default, and the archive is gitignored, so
        /// there is nothing for an unwanted row to pollute.
        #[arg(long)]
        no_archive: bool,
    },
    /// Report an experiment's benchmark history.
    History {
        /// Path to the experiment definition.
        experiment: PathBuf,
    },
    /// Check a model pack's structure and internal consistency.
    Validate {
        /// Path to the model pack directory.
        pack: PathBuf,
        /// Also bind the pack to an experiment, enabling the stronger
        /// checks that need instance-to-type bindings.
        #[arg(long)]
        experiment: Option<PathBuf>,
    },
    /// Report the engine identity.
    Engine,
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Engine => report_engine(),
        Command::Run {
            experiment,
            proof,
            why,
            record,
            note,
        } => cmd_run(&experiment, proof, why, record, &note),
        Command::Counterfactual {
            experiment,
            set,
            unset,
            full,
        } => cmd_counterfactual(&experiment, &set, &unset, full),
        Command::Survey { experiment } => cmd_survey(&experiment),
        Command::Explain { experiment } => cmd_explain(&experiment),
        Command::Replay {
            experiment,
            self_only,
        } => cmd_replay(&experiment, self_only),
        Command::Benchmark {
            experiment,
            iterations,
            no_archive,
        } => cmd_benchmark(&experiment, iterations, !no_archive),
        Command::History { experiment } => cmd_history(&experiment),
        Command::Validate { pack, experiment } => cmd_validate(&pack, experiment.as_deref()),
    }
}

fn report_engine() -> ExitCode {
    match ExecutionContext::with_defaults() {
        Ok(context) => {
            println!("engine version: {ENGINE_VERSION}");
            println!("engine hash:    {}", context.engine_hash());
            println!("proof mode:     {:?}", context.config().proof_mode);
            println!("max states:     {}", context.config().limits.max_states);
            println!("max depth:      {}", context.config().limits.max_depth);
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error.to_string()),
    }
}

fn cmd_run(
    path: &std::path::Path,
    show_proof: bool,
    show_why: bool,
    store: bool,
    note: &str,
) -> ExitCode {
    let result = match runner::run(path) {
        Ok(result) => result,
        Err(error) => return fail(&error.to_string()),
    };

    println!("Experiment: {}", result.experiment);
    println!();

    println!("Result:");
    for (entity_id, entity) in &result.state.entities {
        for (property_id, property) in &entity.properties {
            println!(
                "  {entity_id}.{property_id} = {} [{:?}]",
                runner::show_value(property.value.as_ref()),
                property.status
            );
        }
    }

    if let Some(plan) = &result.plan {
        println!();
        if plan.actions.is_empty() {
            println!("Plan: (already at the goal)");
        } else {
            println!("Plan ({} steps, cost {}):", plan.actions.len(), plan.cost);
            for (step, action) in plan.actions.iter().enumerate() {
                println!("  {}. {action}", step + 1);
            }
        }
    }

    println!();
    print_metrics(&result);

    println!();
    println!("Input hash:    {}", result.record.input);
    println!("Model hash:    {}", result.record.model);
    println!("Engine hash:   {}", result.record.engine);
    println!("Decision hash: {}", result.record.decision);
    println!("Proof hash:    {}", result.record.proof);

    if show_proof {
        println!();
        println!("Proof:");
        print!("{}", dre_proof::render(&result.proof));
    }

    if show_why {
        println!();
        print_why(&result.proof);
    }

    if store {
        match runner::record(path, note) {
            Ok((_, written)) => {
                println!();
                println!("Recorded baseline: {}", written.display());
            }
            Err(error) => return fail(&error.to_string()),
        }
    }

    println!();
    if result.passed() {
        println!("Expectations: PASS");
        ExitCode::SUCCESS
    } else {
        println!("Expectations: FAIL");
        for failure in &result.failures {
            println!("  - {failure}");
        }
        ExitCode::FAILURE
    }
}

/// Print the plan section 20 metrics for one run.
///
/// Two of them report absence rather than a number when there is nothing to
/// report — an unavailable peak and an experiment with no unknowns are both
/// real states, and printing `0` for either would be a false reading.
fn print_metrics(result: &dre_lab::RunResult) {
    println!("Reasoning steps:  {}", result.counters.reasoning_steps);
    println!("Rules evaluated:  {}", result.counters.rules_evaluated);
    println!("Rules fired:      {}", result.counters.rules_fired);
    println!("States expanded:  {}", result.counters.states_expanded);
    println!("Proof nodes:      {}", result.counters.proof_nodes);
    println!("Active dims:      {}", result.counters.active_dimensions);
    println!("Unknowns:         {}", show_resolution(&result.counters));
    println!("Execution:        {} us", result.elapsed_micros);
    println!(
        "Peak memory:      {}",
        dre_lab::memory::show_kib(dre_lab::memory::peak_rss_kib())
    );
}

/// Render the unknown-resolution rate as `resolved/initial (rate)`.
///
/// The rate is derived from per-mille integers rather than a float, because
/// floating point is denied workspace-wide and there is no reason for the
/// reporting path to be the one place it appears.
fn show_resolution(counters: &dre_core::context::Counters) -> String {
    let Some(permille) = counters.unknown_resolution_permille() else {
        return "none started unknown".to_owned();
    };
    format!(
        "{}/{} resolved ({}.{}%)",
        counters.unknown_resolved,
        counters.unknown_initial,
        permille / 10,
        permille % 10
    )
}

/// Describe how far the peak rose across a benchmark loop.
///
/// The interesting number, since the absolute peak is mostly the binary
/// itself: a run that leaks shows growth proportional to iterations, and a
/// run that does not shows none at all.
fn show_growth(before: Option<u64>, after: Option<u64>) -> String {
    match (before, after) {
        (Some(before), Some(after)) if after > before => {
            format!(" (grew {} KiB during the run)", after - before)
        }
        (Some(_), Some(_)) => " (no growth during the run)".to_owned(),
        _ => String::new(),
    }
}

/// Print the chain the run's final conclusion rested on.
///
/// What counts as the conclusion depends on how far the run got, so this
/// falls back through three kinds of node. A purely symbolic experiment
/// reaches no `Decision` — there is no classification and no plan, just
/// derived facts — and a run whose features could not be derived reaches
/// no `Inference` either. Stopping at the first kind would leave `--why`
/// useless on exactly the runs where the question is most worth asking:
/// the ones that concluded less than expected.
fn print_why(proof: &dre_proof::ProofGraph) {
    let conclusion = |wanted: fn(&dre_proof::NodeKind) -> bool| {
        proof
            .nodes()
            .iter()
            .enumerate()
            .rev()
            .find(|(_, node)| wanted(&node.kind))
            .map(|(index, _)| index)
    };

    let found = conclusion(|kind| matches!(kind, dre_proof::NodeKind::Decision { .. }))
        .or_else(|| conclusion(|kind| matches!(kind, dre_proof::NodeKind::Inference { .. })))
        .or_else(|| conclusion(|kind| matches!(kind, dre_proof::NodeKind::Feature { .. })));

    let Some(index) = found else {
        println!("Why: this run reached no decision and derived nothing.");
        return;
    };

    let reference = dre_proof::NodeRef(u32::try_from(index).unwrap_or(u32::MAX));
    match dre_proof::explain(proof, reference) {
        Some(chain) => {
            println!("Why (the steps this conclusion rested on):");
            print!("{chain}");
        }
        None => println!("Why: the concluding node is not in the graph."),
    }
}

/// Parse `entity.property=value` into a perturbation.
///
/// The value goes through the same strict YAML reader a pack section does,
/// so a scalar means here exactly what it means there — including the
/// `{fixed: ...}` form, and including the rejection of the ambiguous ones.
fn parse_set(spec: &str) -> Result<dre_lab::counterfactual::Perturbation, String> {
    let (subject, literal) = spec
        .split_once('=')
        .ok_or_else(|| format!("`{spec}` is not `entity.property=value`"))?;

    let value: dre_state::Value = dre_model::yaml::from_str(literal)
        .map_err(|error| format!("could not read `{literal}` as a value: {error}"))?;

    Ok(dre_lab::counterfactual::Perturbation {
        subject: dre_core::id::Id::new(subject.trim())
            .map_err(|error| format!("`{subject}` is not a valid subject: {error}"))?,
        value: Some(value),
        may_introduce: false,
    })
}

fn cmd_counterfactual(
    path: &std::path::Path,
    set: &[String],
    unset: &[String],
    full: bool,
) -> ExitCode {
    if set.is_empty() && unset.is_empty() {
        eprintln!("nothing to change: pass --set or --unset");
        return ExitCode::from(2);
    }

    let mut perturbations = Vec::new();
    for spec in set {
        match parse_set(spec) {
            Ok(perturbation) => perturbations.push(perturbation),
            Err(message) => {
                eprintln!("{message}");
                return ExitCode::from(2);
            }
        }
    }
    for subject in unset {
        match dre_core::id::Id::new(subject.trim()) {
            Ok(subject) => perturbations.push(dre_lab::counterfactual::Perturbation {
                subject,
                value: None,
                may_introduce: false,
            }),
            Err(error) => {
                eprintln!("`{subject}` is not a valid subject: {error}");
                return ExitCode::from(2);
            }
        }
    }

    let experiment = match runner::load_experiment(path) {
        Ok(experiment) => experiment,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let pack_dir = runner::resolve_pack(path, &experiment.pack);

    let comparison = match dre_lab::counterfactual::compare(&experiment, &pack_dir, &perturbations)
    {
        Ok(comparison) => comparison,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    println!("Experiment: {}", comparison.base.experiment);
    for perturbation in &perturbations {
        match &perturbation.value {
            Some(value) => println!("  what if {} were {value:?}", perturbation.subject),
            None => println!("  what if {} were never observed", perturbation.subject),
        }
    }
    println!();

    // Said first and plainly. A counterfactual whose input hash did not
    // move asked nothing, and "no change" would otherwise read as a
    // finding about the engine rather than about the question.
    if !comparison.input_changed() {
        println!("The input did not change — the property already held that value.");
        println!("Nothing was asked, so nothing is proven about the engine's sensitivity.");
        return ExitCode::SUCCESS;
    }

    println!("Base decision:  {}", comparison.base.record.decision);
    println!("If instead:     {}", comparison.alternative.record.decision);
    println!();

    // Reported on what the engine *derived*, not on the DecisionHash. That
    // hash covers the state, and the state now contains the observation
    // just injected, so it moves whenever the input does — every
    // counterfactual would read as consequential. See
    // `Comparison::conclusion_changed`.
    if comparison.conclusion_changed() {
        println!("CONCLUSION CHANGED");
    } else {
        println!("CONCLUSION UNCHANGED — the engine would have concluded the same thing");
        println!("(the decision hash still moves: the observation is part of the state)");
    }

    if !comparison.no_longer_fired.is_empty() {
        println!("\nRules that stopped firing:");
        for rule in &comparison.no_longer_fired {
            println!("  - {rule}");
        }
    }
    if !comparison.newly_fired.is_empty() {
        println!("\nRules that started firing:");
        for rule in &comparison.newly_fired {
            println!("  + {rule}");
        }
    }

    match (&comparison.base.plan, &comparison.alternative.plan) {
        (Some(base), Some(alternative)) if base.actions != alternative.actions => {
            println!("\nPlan changed:");
            println!("  was  {} steps, cost {}", base.actions.len(), base.cost);
            println!(
                "  now  {} steps, cost {}",
                alternative.actions.len(),
                alternative.cost
            );
        }
        _ => {}
    }

    if comparison.deltas.is_empty() {
        println!("\nNo property ended up different.");
    } else if full {
        println!("\n{} properties differ:", comparison.deltas.len());
        for delta in &comparison.deltas {
            println!("  {}", delta.subject);
            println!("    was {}", delta.before);
            println!("    now {}", delta.after);
        }
    } else {
        println!(
            "\n{} properties differ (pass --full to list them).",
            comparison.deltas.len()
        );
    }

    ExitCode::SUCCESS
}

/// Report the explanation ranking for one experiment.
///
/// The ranking is computed during the ordinary run rather than here, so
/// this prints what the engine already decided instead of asking a second
/// question of its own. A report that re-derived the score could disagree
/// with the proof, and then there would be two answers.
fn cmd_explain(path: &std::path::Path) -> ExitCode {
    let result = match runner::run(path) {
        Ok(result) => result,
        Err(error) => return fail(&error.to_string()),
    };

    let Some(ranking) = result.ranking else {
        println!("{} declares no hypotheses to rank.", result.experiment);
        return ExitCode::SUCCESS;
    };

    println!("Experiment: {}", result.experiment);
    println!(
        "Observations holding: {}",
        show_ids(&ranking.holding, "none")
    );
    if !ranking.undecided.is_empty() {
        // Said out loud. A ranking made while some of the question set is
        // undecided is a ranking on partial information, and the number of
        // things nobody has looked at is part of reading it.
        println!("Undecided: {}", show_ids(&ranking.undecided, ""));
    }
    println!();

    for (position, scored) in ranking.scored.iter().enumerate() {
        println!(
            "  {}. {}  {}",
            position + 1,
            scored.hypothesis,
            scored.score
        );
        println!(
            "     coverage {}, {} contradicted, {} unexplained, {} mechanisms, {} assumed",
            scored.coverage,
            scored.contradicted.len(),
            scored.unexplained.len(),
            scored.complexity,
            scored.assumptions
        );
        for fit in &scored.mechanisms {
            let verdict = match fit.support {
                dre_explain::Support::Supported => "supported",
                dre_explain::Support::Contradicted => "CONTRADICTED",
                dre_explain::Support::Unsupported => "assumed",
            };
            println!(
                "       {} — {verdict} ({} of {} observed)",
                fit.id,
                fit.supported.len(),
                fit.supported.len() + fit.contradicted.len() + fit.silent.len()
            );
        }
        if !scored.unexplained.is_empty() {
            println!("       unexplained: {}", show_ids(&scored.unexplained, ""));
        }
    }

    println!();
    match ranking.winner() {
        Some(winner) => {
            let margin = ranking
                .margin()
                .map_or_else(|| "unopposed".to_owned(), |value| format!("by {value}"));
            println!("Best explanation: {} ({margin})", winner.hypothesis);
        }
        // Not a failure, and worth distinguishing from one. The engine
        // ranked what it was given and the evidence did not separate the
        // top two -- which is a finding, and the one `dre survey` exists to
        // act on.
        None => println!(
            "No explanation is preferred. Either nothing supports the leader, or the 
             top two score exactly equal and only their identifiers separate them."
        ),
    }

    ExitCode::SUCCESS
}

/// Join identifiers for display, or a stand-in when there are none.
fn show_ids(ids: &[dre_core::id::Id], empty: &str) -> String {
    if ids.is_empty() {
        return empty.to_owned();
    }
    ids.iter()
        .map(dre_core::id::Id::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

fn cmd_survey(path: &std::path::Path) -> ExitCode {
    let experiment = match runner::load_experiment(path) {
        Ok(experiment) => experiment,
        Err(error) => return fail(&error.to_string()),
    };
    let pack_dir = runner::resolve_pack(path, &experiment.pack);

    let pack = match dre_model::load_dir(&pack_dir) {
        Ok(pack) => pack,
        Err(error) => return fail(&error.to_string()),
    };

    let survey = match dre_lab::survey::survey(&experiment, &pack, &pack_dir) {
        Ok(survey) => survey,
        Err(error) => return fail(&error.to_string()),
    };

    println!("Experiment: {}", experiment.id);
    println!("Base decision: {}", survey.base_decision);
    if let Some(cost) = survey.base_cost {
        println!("Base plan cost: {cost}");
    }
    if let Some(leader) = &survey.base_leader {
        let lead = survey.base_lead.map_or_else(
            || "unopposed".to_owned(),
            |value| format!("leading by {value}"),
        );
        println!("Best explanation: {leader} ({lead})");
    }
    println!();

    if survey.surveyed.is_empty() && survey.declined.is_empty() {
        println!("Nothing is unknown — there is no observation left to take.");
        return ExitCode::SUCCESS;
    }

    // Reported before the decision-change ranking, and separately, because
    // it answers a different question. An observation can change the plan
    // without separating the accounts, and one that settles the diagnosis
    // may leave the plan alone.
    let discriminating: Vec<_> = survey
        .surveyed
        .iter()
        .filter(|entry| {
            entry
                .discrimination()
                .is_some_and(|spread| spread > dre_core::fixed::Fixed::ZERO)
        })
        .collect();
    let indifferent: Vec<_> = survey
        .surveyed
        .iter()
        .filter(|entry| entry.discrimination() == Some(dre_core::fixed::Fixed::ZERO))
        .collect();

    if !discriminating.is_empty() || !indifferent.is_empty() {
        println!("Which measurement would settle the diagnosis:");
    }
    for entry in &discriminating {
        let spread = entry
            .discrimination()
            .map_or_else(String::new, |value| value.to_string());
        let verdict = if entry.flips_leader() {
            "could overturn the diagnosis"
        } else {
            "would confirm or weaken it, not overturn it"
        };
        println!(
            "
  {} — {verdict} (spread {spread})",
            entry.subject
        );
        for outcome in &entry.outcomes {
            if let (Some(lead), Some(leader)) = (outcome.lead, outcome.leader.as_ref()) {
                println!(
                    "    {} -> {leader} in front, base leader at {lead}",
                    runner::show_value(Some(&outcome.value))
                );
            }
        }
    }
    // Said out loud rather than omitted. "This measurement cannot settle
    // anything" is a real answer, and an unknown missing from the report
    // is indistinguishable from one nobody thought to check.
    for entry in &indifferent {
        println!(
            "
  {} — separates nothing; every answer leaves the accounts where they are",
            entry.subject
        );
    }
    if !discriminating.is_empty() || !indifferent.is_empty() {
        println!();
    }

    let decisive: Vec<_> = survey
        .surveyed
        .iter()
        .filter(|entry| entry.decisive())
        .collect();
    let inert: Vec<_> = survey
        .surveyed
        .iter()
        .filter(|entry| !entry.decisive())
        .collect();

    if decisive.is_empty() {
        println!("No single observation would change this decision.");
    } else {
        println!("Worth measuring, most informative first:");
    }

    for entry in decisive {
        println!("\n  {}", entry.subject);

        // The spread is the reading. A property whose worst case is the
        // base cost can only help; one whose best case is the base cost can
        // only hurt, and going out to measure it buys nothing.
        if let (Some(best), Some(worst), Some(base)) =
            (entry.best_cost(), entry.worst_cost(), survey.base_cost)
        {
            if best < base && worst <= base {
                println!("    can only help — as good as {best}, no worse than {worst}");
            } else if best >= base {
                println!("    can only hurt — no better than {best}, as bad as {worst}");
            } else {
                println!("    between {best} and {worst}, against {base} now");
            }
        }

        for outcome in &entry.outcomes {
            let verdict = if outcome.decision_changed {
                "decides differently"
            } else {
                "no change"
            };
            match (outcome.plan_cost, outcome.plan_steps) {
                (Some(cost), Some(steps)) => println!(
                    "    if {:?} — {verdict}, plan {steps} steps costing {cost}",
                    outcome.value
                ),
                _ => println!("    if {:?} — {verdict}", outcome.value),
            }
        }
    }

    // Said explicitly rather than omitted. "This reading cannot help you"
    // is a real answer and the one that stops somebody going out to take a
    // measurement that changes nothing.
    if !inert.is_empty() {
        println!("\nNot worth measuring — no admissible value changes the decision:");
        for entry in inert {
            println!("  {}", entry.subject);
        }
    }

    if !survey.declined.is_empty() {
        println!("\nCould not be surveyed:");
        for entry in &survey.declined {
            println!("  {} — {}", entry.subject, entry.reason);
        }
    }

    ExitCode::SUCCESS
}

fn cmd_replay(path: &std::path::Path, self_only: bool) -> ExitCode {
    if self_only {
        return match runner::verify_replay(path) {
            Ok((verdict, decision)) => {
                println!("Decision hash: {decision}");
                println!("Compared:      two fresh runs (no stored baseline)");
                report_verdict(&verdict)
            }
            Err(error) => fail(&error.to_string()),
        };
    }

    match runner::verify_against_stored(path) {
        Ok((verdict, comparison)) => {
            println!("Recorded under: engine {}", comparison.recorded_under);
            if !comparison.note.is_empty() {
                println!("Note:           {}", comparison.note);
            }
            println!("Decision hash:  {}", comparison.fresh.decision);
            println!("Proof hash:     {}", comparison.fresh.proof);
            println!();
            report_verdict(&verdict)
        }
        Err(error) => fail(&error.to_string()),
    }
}

/// Print a replay verdict, spelling out what a mismatch means.
fn report_verdict(verdict: &dre_replay::Verdict) -> ExitCode {
    match verdict {
        dre_replay::Verdict::Match => {
            println!("Replay: PASS");
            ExitCode::SUCCESS
        }
        dre_replay::Verdict::NotComparable { differing } => {
            println!("Replay: NOT COMPARABLE");
            println!("  these differ: {differing:?}");
            println!("  the runs describe different problems, so nothing is proven either way");
            ExitCode::FAILURE
        }
        dre_replay::Verdict::DecisionMismatch { expected, actual } => {
            println!("Replay: FAIL - different decision");
            println!("  recorded: {expected}");
            println!("  now:      {actual}");
            ExitCode::FAILURE
        }
        dre_replay::Verdict::ProofMismatch { expected, actual } => {
            println!("Replay: FAIL - same decision, different reasoning");
            println!("  recorded: {expected}");
            println!("  now:      {actual}");
            println!("  a decision-only comparison would have passed this");
            ExitCode::FAILURE
        }
    }
}

fn cmd_benchmark(path: &std::path::Path, iterations: u32, archive: bool) -> ExitCode {
    let experiment = match runner::load_experiment(path) {
        Ok(experiment) => experiment,
        Err(error) => return fail(&error.to_string()),
    };
    let pack_dir = match path.parent() {
        Some(parent) if parent.join(&experiment.pack).is_dir() => parent.join(&experiment.pack),
        Some(parent) => parent.join("..").join(&experiment.pack),
        None => experiment.pack.clone(),
    };

    let mut decisions = std::collections::BTreeSet::new();
    let mut proofs = std::collections::BTreeSet::new();
    let mut total_micros = 0u128;
    let mut last: Option<dre_lab::RunResult> = None;

    // Sampled before the loop so the figure below is growth *across the
    // iterations*, not the cost of starting the process. VmHWM only ever
    // rises, so a per-iteration sample would be meaningless.
    let peak_before = dre_lab::memory::peak_rss_kib();

    for _ in 0..iterations {
        match runner::run_experiment(&experiment, &pack_dir) {
            Ok(result) => {
                decisions.insert(result.record.decision.to_string());
                proofs.insert(result.record.proof.to_string());
                total_micros += result.elapsed_micros;
                last = Some(result);
            }
            Err(error) => return fail(&error.to_string()),
        }
    }

    let peak_after = dre_lab::memory::peak_rss_kib();

    println!("Experiment: {}", experiment.id);
    println!("Iterations: {iterations}");
    println!("Unique decision hashes: {}", decisions.len());
    println!("Unique proof hashes:    {}", proofs.len());
    println!(
        "Mean execution:         {} us",
        total_micros / u128::from(iterations.max(1))
    );
    println!(
        "Peak memory:            {}{}",
        dre_lab::memory::show_kib(peak_after),
        show_growth(peak_before, peak_after)
    );

    if let Some(result) = &last {
        println!("Proof size:             {} nodes", result.proof.len());
        println!(
            "Reasoning steps:        {}",
            result.counters.reasoning_steps
        );
        println!(
            "Rules evaluated:        {}",
            result.counters.rules_evaluated
        );
        println!(
            "States expanded:        {}",
            result.counters.states_expanded
        );
        println!(
            "Active dimensions:      {}",
            result.counters.active_dimensions
        );
        println!(
            "Unknowns:               {}",
            show_resolution(&result.counters)
        );
    }

    let deterministic = decisions.len() == 1 && proofs.len() == 1;

    if archive {
        if let Some(result) = &last {
            let mean =
                u64::try_from(total_micros / u128::from(iterations.max(1))).unwrap_or(u64::MAX);
            let single = |set: &std::collections::BTreeSet<String>,
                          value: dre_core::hash::Hash32| {
                if set.len() == 1 {
                    Some(value)
                } else {
                    None
                }
            };

            let row = dre_lab::bench::BenchmarkRecord {
                experiment: experiment.id.clone(),
                recorded_at_unix: dre_lab::bench::now_unix(),
                profile: dre_lab::bench::profile().to_owned(),
                iterations,
                unique_decisions: decisions.len(),
                unique_proofs: proofs.len(),
                decision: single(&decisions, result.record.decision),
                proof: single(&proofs, result.record.proof),
                model: result.record.model,
                engine: result.record.engine,
                engine_version: ENGINE_VERSION.to_string(),
                mean_micros: mean,
                peak_memory_kib: peak_after,
                memory_growth_kib: match (peak_before, peak_after) {
                    (Some(before), Some(after)) => Some(after.saturating_sub(before)),
                    _ => None,
                },
                proof_nodes: result.proof.len(),
                counters: result.counters,
            };

            match dre_lab::bench::append(&runner::repo_root_for(path), &row) {
                Ok(written) => println!("Archived:               {}", written.display()),
                // A failed archive write must not fail the benchmark. The
                // measurement is the result; the history is bookkeeping,
                // and losing a row is not a reason to report the run as
                // broken -- but it is a reason to say so out loud.
                Err(error) => eprintln!("warning: could not archive this run: {error}"),
            }
        }
    }

    println!();
    println!(
        "Determinism: {}",
        if deterministic { "PASS" } else { "FAIL" }
    );

    if deterministic {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn cmd_history(path: &std::path::Path) -> ExitCode {
    let experiment = match runner::load_experiment(path) {
        Ok(experiment) => experiment,
        Err(error) => return fail(&error.to_string()),
    };

    let root = runner::repo_root_for(path);
    let records = match dre_lab::bench::load(&root, &experiment.id) {
        Ok(records) => records,
        Err(error) => return fail(&error.to_string()),
    };

    println!("Experiment: {}", experiment.id);
    println!(
        "Archive:    {}",
        dre_lab::bench::archive_path(&root, &experiment.id).display()
    );

    let Some(history) = dre_lab::bench::summarize(&records) else {
        println!();
        println!(
            "No runs recorded yet. Run `dre benchmark {}`.",
            path.display()
        );
        return ExitCode::SUCCESS;
    };

    println!(
        "Rows:       {} ({} comparable)",
        records.len(),
        history.considered
    );
    println!();
    println!("Mean execution:  {} us", history.latest_micros);
    println!(
        "Range:           {} us to {} us",
        history.fastest_micros, history.slowest_micros
    );

    match (history.previous_micros, history.drift_permille()) {
        (Some(previous), Some(drift)) => {
            let direction = if drift > 0 { "slower" } else { "faster" };
            let magnitude = drift.abs();
            println!(
                "Against previous: {previous} us, {}.{}% {direction}",
                magnitude / 10,
                magnitude % 10
            );
        }
        _ => println!("Against previous: nothing comparable to compare against"),
    }

    if !history.excluded.is_empty() {
        println!();
        println!("Set aside as not comparable ({}):", history.excluded.len());
        for reason in &history.excluded {
            println!("  - {reason}");
        }
    }

    println!();
    if history.agrees() {
        println!(
            "Agreement: PASS - one decision and one proof hash across {} {}",
            history.considered,
            if history.considered == 1 {
                "run"
            } else {
                "runs"
            }
        );
        ExitCode::SUCCESS
    } else {
        // The archive's real payload. Each row can be internally
        // deterministic -- one hash over its own loop -- while the answer
        // changed between runs, and only the history can see that.
        println!("Agreement: FAIL - the answer moved between runs");
        println!("  {} distinct decision hashes", history.decisions);
        println!("  {} distinct proof hashes", history.proofs);
        ExitCode::FAILURE
    }
}

fn cmd_validate(pack: &std::path::Path, experiment: Option<&std::path::Path>) -> ExitCode {
    let loaded = match dre_model::load_dir(pack) {
        Ok(loaded) => loaded,
        Err(error) => return fail(&error.to_string()),
    };

    println!("Pack:     {}", loaded.manifest.id);
    println!("Version:  {}", loaded.manifest.version);
    println!("Requires: engine {}", loaded.manifest.requires_engine);
    println!("Hash:     {}", loaded.model_hash());
    println!("Sections:");
    for (kind, source) in &loaded.sections {
        println!("  {} ({} bytes)", kind.file_stem(), source.bytes.len());
    }

    let findings = match experiment {
        None => dre_lab::validate::pack(&loaded),
        Some(path) => match runner::load_experiment(path) {
            Ok(given) => {
                println!("Bound to: {}", given.id);
                dre_lab::validate::experiment(&loaded, &given)
            }
            Err(error) => return fail(&error.to_string()),
        },
    };

    let findings = match findings {
        Ok(findings) => findings,
        Err(error) => return fail(&error.to_string()),
    };

    if !findings.is_empty() {
        println!();
        println!("Findings:");
        for finding in &findings {
            println!("  {finding}");
        }
    }

    println!();
    if dre_lab::validate::has_errors(&findings) {
        println!("Validate: FAIL");
        // Said explicitly, because the failure this catches is one that
        // never announces itself at run time: a rule on a property that
        // does not exist evaluates to unknown, quietly never fires, and
        // the run looks perfectly ordinary.
        println!("  a rule on a property nothing declares does not fail at run time -");
        println!("  it evaluates to unknown, never fires, and the answer looks normal");
        ExitCode::FAILURE
    } else {
        println!("Validate: PASS");
        if experiment.is_none() {
            println!("  pass --experiment <file> to also check types, kinds and domains");
        }
        ExitCode::SUCCESS
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::FAILURE
}
