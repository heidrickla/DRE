//! Determinism conformance suite.
//!
//! Plan section 21 asks for tests written specifically to *break*
//! determinism rather than to confirm it. That distinction drives the
//! design here.
//!
//! A workload that simply ran the same pure function 100,000 times would
//! pass trivially and prove nothing — it would return the same value even
//! if the engine were riddled with ordering bugs, because nothing would
//! vary between iterations. So [`phase1_workload`] takes a seed and uses
//! it to vary everything that *should not* matter: the order entities are
//! declared in, the order properties are attached, the order map keys are
//! inserted. Everything that *should* matter is held fixed.
//!
//! The acceptance criterion then has teeth. 100,000 iterations means
//! 100,000 different construction orders, and a single differing hash
//! means some structure leaked its insertion history into the output.
//!
//! This is also why the suite lives outside `dre-core`: it should exercise
//! the crates the way an experiment does, through their public surface,
//! rather than reaching into internals a real caller could not.

use dre_core::canonical::CanonicalWriter;
use dre_core::collections::DetMap;
use dre_core::context::ExecutionContext;
use dre_core::fixed::Fixed;
use dre_core::hash::Hash32;
use dre_core::id::Id;
use dre_core::version::Version;
use dre_core::Result;
use dre_state::{CanonicalState, Entity, EpistemicStatus, Property, Value};

/// A deterministic permutation source.
///
/// Test scaffolding, not engine code. The engine has no random number
/// generator and must not acquire one — this exists precisely to attack
/// the engine from outside by feeding it the same content in many
/// different orders.
///
/// An LCG rather than a real PRNG because the quality of the shuffle does
/// not matter; only that it is reproducible from the seed, so a failing
/// iteration can be replayed exactly.
pub struct Permuter(u64);

impl Permuter {
    /// Seed the permuter.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        // Offset so seed 0 does not produce a degenerate stream.
        Self(seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1))
    }

    /// Next value in the stream.
    fn next_value(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        // Use the high bits; the low bits of an LCG have short periods.
        self.0 >> 16
    }

    /// Shuffle in place (Fisher-Yates).
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        if items.is_empty() {
            return;
        }
        let mut index = items.len() - 1;
        while index > 0 {
            let swap_with = (self.next_value() % (index as u64 + 1)) as usize;
            items.swap(index, swap_with);
            index -= 1;
        }
    }
}

/// The fixed content of the phase 1 workload.
///
/// Deliberately spans every `Value` variant and a range of epistemic
/// states, because the encoding of each is a separate opportunity to leak
/// nondeterminism. A workload of eight identical integer properties would
/// exercise one code path and pass while the others were broken.
fn fixtures() -> Result<Vec<(Id, Id, Vec<Property>)>> {
    let entity = |name: &str| Id::new(name);
    let prop = |name: &str, value: Option<Value>, status: EpistemicStatus| -> Result<Property> {
        Ok(Property {
            name: Id::new(name)?,
            value,
            status,
        })
    };

    let mut out = Vec::new();

    for (index, name) in ["object-a", "object-b", "object-c", "object-d"]
        .into_iter()
        .enumerate()
    {
        let scale = i64::try_from(index).unwrap_or(0);
        out.push((
            entity(name)?,
            entity("generic-object")?,
            vec![
                prop(
                    "temperature",
                    Some(Value::Integer(700 + scale * 37)),
                    EpistemicStatus::Known,
                )?,
                prop(
                    "efficiency",
                    Some(Value::Fixed(Fixed::from_raw(500_000 + scale * 12_345))),
                    EpistemicStatus::Inferred,
                )?,
                prop(
                    "bearing_fault",
                    Some(Value::Boolean(index % 2 == 0)),
                    EpistemicStatus::Conflicting,
                )?,
                prop(
                    "grade",
                    Some(Value::Symbol(entity("grade-a")?)),
                    EpistemicStatus::Known,
                )?,
                prop(
                    "paired_with",
                    Some(Value::Reference(entity("object-a")?)),
                    EpistemicStatus::Known,
                )?,
                // No value. The three no-value states must stay
                // distinguishable in the encoding.
                prop("internal_state", None, EpistemicStatus::Unknown)?,
                prop("core_pressure", None, EpistemicStatus::Unobservable)?,
                prop("legacy_field", None, EpistemicStatus::NotApplicable)?,
            ],
        ));
    }

    Ok(out)
}

/// Build the workload's canonical state, declaring everything in an order
/// determined by `seed`.
fn build_state(seed: u64) -> Result<CanonicalState> {
    let mut permuter = Permuter::new(seed);
    let mut entities = fixtures()?;
    permuter.shuffle(&mut entities);

    let mut state = CanonicalState::default();
    for (id, type_id, mut properties) in entities {
        permuter.shuffle(&mut properties);

        let mut map: DetMap<Id, Property> = DetMap::new();
        for property in properties {
            map.insert(property.name.clone(), property);
        }

        state.entities.insert(
            id.clone(),
            Entity {
                id,
                type_id,
                properties: map,
            },
        );
    }

    Ok(state)
}

/// Fixed-point accumulation, exercised in a seed-varied order.
///
/// Addition is associative and commutative in exact integer arithmetic, so
/// summing in a different order must give an identical result. Under
/// floating point it would not — which is the concrete reason `Fixed`
/// exists, and this is the check that would catch a regression to `f64`.
fn accumulate(seed: u64) -> Result<Fixed> {
    let mut terms: Vec<Fixed> = (1..=32)
        .map(|n| Fixed::from_raw(n * 1_234_567 - 7_000_000))
        .collect();
    Permuter::new(seed ^ 0xA5A5_A5A5).shuffle(&mut terms);

    let mut total = Fixed::ZERO;
    for term in terms {
        total = total.checked_add(term)?;
    }
    Ok(total)
}

/// The phase 1 workload: load an input object, produce one output hash.
///
/// `seed` varies only the order in which content is constructed. Two
/// different seeds describe the *same* problem and must therefore produce
/// the same hash.
pub fn phase1_workload(seed: u64) -> Result<Hash32> {
    let state = build_state(seed)?;
    let total = accumulate(seed)?;
    let context = ExecutionContext::with_defaults()?;

    let mut writer = CanonicalWriter::new();
    writer
        .tag("phase1-workload")
        .write(&Version::new(0, 1, 0))
        .write(&state)
        .write(&total)
        .write(&context.engine_hash());

    Ok(Hash32::of(writer.as_bytes()))
}
