//! Deterministic collections.
//!
//! `std::collections::HashMap` and `HashSet` are banned throughout the
//! workspace (enforced by `clippy.toml`). Their default hasher is seeded
//! per process, so iteration order differs between two runs of the same
//! binary on the same input. Any engine step that iterates a map — rule
//! evaluation order, candidate generation, proof node emission — would
//! then produce a different result or a different proof each run.
//!
//! The ordered variants trade a constant factor of lookup speed for total
//! order. That is the correct trade here: the MVP's scale ceilings (plan
//! section 22) are small enough that the difference is not the bottleneck,
//! and determinism is the property being proven.

use std::collections::{BTreeMap, BTreeSet};

/// Deterministic ordered map. Iteration follows `Ord` on the key.
pub type DetMap<K, V> = BTreeMap<K, V>;

/// Deterministic ordered set. Iteration follows `Ord` on the element.
pub type DetSet<T> = BTreeSet<T>;
