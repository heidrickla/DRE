//! Loading and validating model packs.
//!
//! # Filesystem access and determinism
//!
//! `docs/determinism.md` bans environment and configuration reads from
//! engine crates. Reading a model pack is not an exception to that rule,
//! because it is a different thing: the caller names an explicit path, and
//! the same directory contents always produce the same pack. What the rule
//! forbids is *implicit* input — a value reaching a decision without
//! appearing in the input, model, or engine hashes. Everything read here
//! is covered by `ModelHash`.
//!
//! One consequence shapes the code below: **sections are read in a fixed
//! order, never in directory-listing order.** Readdir order varies by
//! filesystem, so iterating it would let the medium the pack was stored on
//! change the pack.

use std::fs;
use std::path::{Path, PathBuf};

use dre_core::collections::DetMap;
use dre_core::context::ENGINE_VERSION;
use dre_core::hash::Hash32;
use dre_core::version::Version;
use serde::Deserialize;
use thiserror::Error;

use crate::yaml;
use crate::{ModelManifest, ModelPack, SectionKind, SectionSource};

/// Name of the manifest file within a pack directory.
pub const MANIFEST_FILE: &str = "model.yaml";

/// Maximum bytes accepted for any one pack file.
///
/// Bounded for the same reason the engine bounds everything else: a pack
/// is untrusted input, and an unbounded read is an unbounded step.
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// Failures loading or validating a model pack.
#[derive(Debug, Error)]
pub enum LoadError {
    /// The pack directory or a required file is missing.
    #[error("{path}: {source}")]
    Io {
        /// Path being read.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },

    /// A file exceeded [`MAX_FILE_BYTES`].
    #[error("{path} is {size} bytes, over the {MAX_FILE_BYTES} byte limit")]
    FileTooLarge {
        /// Path being read.
        path: PathBuf,
        /// Its size.
        size: u64,
    },

    /// A YAML file did not parse under the pack policy.
    #[error("{path}: {source}")]
    Yaml {
        /// Path being parsed.
        path: PathBuf,
        /// Underlying parse error.
        #[source]
        source: Box<serde_saphyr::Error>,
    },

    /// The pack requires a newer engine than this one.
    #[error("pack {pack} requires engine {required}, this engine is {actual}")]
    EngineTooOld {
        /// The pack's identifier.
        pack: String,
        /// Version the pack asks for.
        required: Version,
        /// Version of this engine.
        actual: Version,
    },

    /// The pack declared no sections at all.
    #[error("pack {pack} declares no sections")]
    Empty {
        /// The pack's identifier.
        pack: String,
    },
}

/// Result alias for loading.
pub type Result<T> = std::result::Result<T, LoadError>;

/// The shape of `model.yaml`.
///
/// Wrapped in a `model:` key so the file reads as a document about a
/// model rather than as bare fields.
#[derive(Debug, Deserialize)]
struct ManifestFile {
    model: ModelManifest,
}

/// Load a model pack from a directory.
pub fn load_dir(directory: &Path) -> Result<ModelPack> {
    let manifest_path = directory.join(MANIFEST_FILE);
    let manifest_bytes = read_bounded(&manifest_path)?;
    let manifest_text = String::from_utf8_lossy(&manifest_bytes).into_owned();

    let parsed: ManifestFile =
        yaml::from_str(&manifest_text).map_err(|source| LoadError::Yaml {
            path: manifest_path,
            source: Box::new(source),
        })?;
    let manifest = parsed.model;

    // Fixed order, not readdir order.
    let mut sections: DetMap<SectionKind, SectionSource> = DetMap::new();
    for kind in SectionKind::all() {
        let path = directory.join(format!("{}.yaml", kind.file_stem()));
        if !path.is_file() {
            continue;
        }
        let bytes = read_bounded(&path)?;
        sections.insert(kind, SectionSource { kind, bytes });
    }

    let pack = ModelPack { manifest, sections };
    validate(&pack)?;
    Ok(pack)
}

/// Check a loaded pack for problems that are cheap to catch before use.
pub fn validate(pack: &ModelPack) -> Result<()> {
    if pack.manifest.requires_engine > ENGINE_VERSION {
        return Err(LoadError::EngineTooOld {
            pack: pack.manifest.id.to_string(),
            required: pack.manifest.requires_engine,
            actual: ENGINE_VERSION,
        });
    }

    if pack.sections.is_empty() {
        return Err(LoadError::Empty {
            pack: pack.manifest.id.to_string(),
        });
    }

    Ok(())
}

/// Read a file, refusing anything over [`MAX_FILE_BYTES`].
fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::metadata(path).map_err(|source| LoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    if metadata.len() > MAX_FILE_BYTES {
        return Err(LoadError::FileTooLarge {
            path: path.to_path_buf(),
            size: metadata.len(),
        });
    }

    fs::read(path).map_err(|source| LoadError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Parse one section of a loaded pack under the pack YAML policy.
///
/// Each reasoner calls this for the section it owns — see the layering
/// note in the crate docs.
pub fn parse_section<T: serde::de::DeserializeOwned>(
    pack: &ModelPack,
    kind: SectionKind,
) -> Result<Option<T>> {
    let Some(source) = pack.section(kind) else {
        return Ok(None);
    };

    let text = String::from_utf8_lossy(&source.bytes);
    let parsed = yaml::from_str(&text).map_err(|error| LoadError::Yaml {
        path: PathBuf::from(format!("{}.yaml", kind.file_stem())),
        source: Box::new(error),
    })?;

    Ok(Some(parsed))
}

/// The `ModelHash` for a pack, re-exported for convenience.
#[must_use]
pub fn model_hash(pack: &ModelPack) -> Hash32 {
    pack.model_hash()
}
