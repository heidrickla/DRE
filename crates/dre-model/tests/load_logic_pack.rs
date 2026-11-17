//! Loading the real `model-packs/logic` pack from disk.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::PathBuf;

use dre_model::{load_dir, SectionKind};

fn logic_pack_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/dre-model.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("model-packs/logic")
}

#[test]
fn loads_the_logic_pack() {
    let pack = load_dir(&logic_pack_dir()).expect("logic pack must load");

    assert_eq!(pack.manifest.id.as_str(), "simple-logic");
    assert_eq!(pack.manifest.version.to_string(), "0.1.0");
    assert!(pack.manifest.description.contains("chaining"));
}

#[test]
fn declares_only_the_sections_it_needs() {
    let pack = load_dir(&logic_pack_dir()).expect("logic pack must load");

    assert!(pack.section(SectionKind::Ontology).is_some());
    assert!(pack.section(SectionKind::Rules).is_some());

    // A pure logic domain has no geometry. If loading ever required one,
    // a domain assumption would have leaked into the runtime.
    assert!(pack.section(SectionKind::Geometry).is_none());
    assert!(pack.section(SectionKind::Causal).is_none());
}

#[test]
fn model_hash_is_stable_across_loads() {
    let first = load_dir(&logic_pack_dir()).expect("load");
    let second = load_dir(&logic_pack_dir()).expect("load");
    assert_eq!(first.model_hash(), second.model_hash());
}

#[test]
fn missing_pack_is_an_error_not_an_empty_pack() {
    let result = load_dir(&logic_pack_dir().join("does-not-exist"));
    assert!(result.is_err());
}
