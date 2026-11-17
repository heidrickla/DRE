//! The engine must not read its own location.
//!
//! Every other attack in this suite varies something *inside* the problem —
//! declaration order, thread count, the order a pack lists its rules. This
//! one varies something outside it entirely: where the files happen to sit
//! on disk.
//!
//! The failure it looks for is a path reaching a hash. A pack loaded from
//! `/home/someone/DRE` and the same pack loaded from `/tmp/whatever` are
//! the same model, and a `ModelHash` that disagreed would make every replay
//! record valid only on the machine and in the directory that produced it.
//! Nothing in the engine is supposed to know where it is; this checks that
//! rather than assuming it.
//!
//! It became cheap to write once external packs started arriving on
//! branches checked out into `git worktree` directories beside the main
//! one, which is exactly the situation where a location dependency would
//! start mattering and would be maddening to diagnose.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Copy a directory tree, without pulling in a dependency to do it.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("destination is creatable");
    for entry in std::fs::read_dir(from).expect("source is readable") {
        let entry = entry.expect("directory entry");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("file copies");
        }
    }
}

/// Stage a pack and its experiment somewhere else entirely.
///
/// The relative layout is preserved because the runner resolves a pack
/// path relative to the experiment file — the point is to move the whole
/// thing, not to rearrange it.
fn stage(pack: &str, experiment: &str, label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "dre-location-{}-{}-{label}",
        std::process::id(),
        pack
    ));
    let _ = std::fs::remove_dir_all(&root);

    copy_tree(
        &repo_root().join("model-packs").join(pack),
        &root.join("model-packs").join(pack),
    );
    std::fs::create_dir_all(root.join("experiments")).expect("experiments dir");
    std::fs::copy(
        repo_root().join("experiments").join(experiment),
        root.join("experiments").join(experiment),
    )
    .expect("experiment copies");

    root
}

fn fingerprint(experiment_path: &Path) -> String {
    let result = dre_lab::run(experiment_path).expect("experiment runs");
    format!(
        "{} {} {} {} {}",
        result.record.input,
        result.record.model,
        result.record.engine,
        result.record.decision,
        result.record.proof
    )
}

/// Every pack, with an experiment that exercises it.
const CASES: [(&str, &str); 6] = [
    ("logic", "logic-001.yaml"),
    ("diagnosis", "diagnosis-003.yaml"),
    ("process", "process-002.yaml"),
    ("navigation", "navigation-001.yaml"),
    ("cold-chain-release", "cold-chain-release-002.yaml"),
    ("digital-evidence-triage", "digital-evidence-002.yaml"),
];

#[test]
fn all_five_hashes_are_independent_of_where_the_files_live() {
    for (pack, experiment) in CASES {
        let from_repo = fingerprint(&repo_root().join("experiments").join(experiment));

        let staged = stage(pack, experiment, "elsewhere");
        let from_elsewhere = fingerprint(&staged.join("experiments").join(experiment));
        let _ = std::fs::remove_dir_all(&staged);

        assert_eq!(
            from_repo, from_elsewhere,
            "{experiment} hashed differently when loaded from another directory; \
             something is reading its own path"
        );
    }
}

#[test]
fn two_copies_at_different_paths_agree_with_each_other() {
    // Neither copy is the repository, so a pass here cannot be explained by
    // one of them being the "real" one. This is the shape a `git worktree`
    // arrangement actually has: two checkouts, both elsewhere.
    let (pack, experiment) = CASES[1];

    let left = stage(pack, experiment, "left");
    let right = stage(pack, experiment, "right");

    let a = fingerprint(&left.join("experiments").join(experiment));
    let b = fingerprint(&right.join("experiments").join(experiment));

    let _ = std::fs::remove_dir_all(&left);
    let _ = std::fs::remove_dir_all(&right);

    assert_eq!(a, b, "two copies at different paths disagreed");
}

#[test]
fn the_staging_really_does_move_the_files() {
    // Guards the tests above from passing vacuously by staging into a
    // directory that turned out to be the repository itself.
    let (pack, experiment) = CASES[0];
    let staged = stage(pack, experiment, "control");

    assert!(
        staged.join("experiments").join(experiment).is_file(),
        "the experiment was not copied"
    );
    assert!(
        !staged.starts_with(repo_root().canonicalize().unwrap_or_else(|_| repo_root())),
        "staged inside the repository, so nothing was actually moved"
    );

    let _ = std::fs::remove_dir_all(&staged);
}
