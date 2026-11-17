//! YAML parsing policy for model packs.
//!
//! Model packs are YAML (plan section 9). YAML is a large, permissive
//! format with several features that are actively hostile to a system
//! whose selling point is auditable, reproducible reasoning — so the
//! parser is configured far more strictly than its defaults.
//!
//! Every setting below is a correctness decision, not a preference. The
//! rationale for the parser choice itself is in `docs/decisions.md`.

use serde::de::DeserializeOwned;
use serde_saphyr::{DuplicateKeyPolicy, MergeKeyPolicy, Options};

/// Maximum alias-replay events allowed while parsing one pack.
///
/// YAML aliases expand at parse time, so a small document can expand to an
/// arbitrarily large one — the "billion laughs" shape. DRE bounds every
/// other resource (plan section 22); leaving the parser unbounded would
/// put an unbounded step in front of a bounded engine.
const MAX_ALIAS_EVENTS: usize = 10_000;

/// Maximum nesting depth of alias replay.
const MAX_ALIAS_DEPTH: usize = 16;

/// Maximum expansions of any single anchor.
///
/// The crate defaults this to `usize::MAX`, bounded only by the global
/// event budget. Bounding it per anchor makes the failure report name the
/// anchor responsible rather than just the total.
const MAX_EXPANSIONS_PER_ANCHOR: usize = 64;

/// The deserialization policy for model pack YAML.
///
/// Differences from the crate defaults, each with a reason:
///
/// **`merge_keys: Error`** (default `Merge`). YAML merge keys (`<<`) let a
/// mapping silently inherit fields from an anchor defined elsewhere in the
/// file. For a rule set, that means a rule's conditions are not visible at
/// the rule — you have to resolve an inheritance chain to know what it
/// tests. A reasoning engine that claims its decisions are explainable
/// should not accept a model format where the model itself is indirect.
///
/// **`strict_booleans: true`** (default `false`). YAML 1.1 reads `yes`,
/// `no`, `on` and `off` as booleans. A model pack declaring a symbol value
/// of `NO` — a perfectly reasonable enumeration member — would silently
/// become `false`. This is the classic "Norway problem", and in a typed
/// property it produces a type error at best and a wrong value at worst.
///
/// **`alias_limits`** tightened, see the constants above.
///
/// Kept from the defaults because they are already correct here:
/// `duplicate_keys: Error` (two rules with one id would break the total
/// ordering that rule firing depends on), `legacy_octal_numbers: false`
/// (YAML 1.2), and an enabled parse `budget`.
#[must_use]
pub fn options() -> Options {
    serde_saphyr::options! {
        duplicate_keys: DuplicateKeyPolicy::Error,
        merge_keys: MergeKeyPolicy::Error,
        strict_booleans: true,
        alias_limits: serde_saphyr::alias_limits! {
            max_total_replayed_events: MAX_ALIAS_EVENTS,
            max_replay_stack_depth: MAX_ALIAS_DEPTH,
            max_alias_expansions_per_anchor: MAX_EXPANSIONS_PER_ANCHOR,
        },
    }
}

/// Deserialize model pack YAML under the policy above.
///
/// Every model pack section must be parsed through this rather than
/// `serde_saphyr::from_str`, or it silently gets the permissive defaults.
pub fn from_str<T: DeserializeOwned>(input: &str) -> Result<T, serde_saphyr::Error> {
    serde_saphyr::from_str_with_options(input, options())
}
