//! Phase 3 — Model pack format.
//!
//! Lets a reasoning domain be defined without modifying the engine. This
//! is the crate that carries the MVP's domain-isolation claim: adding a
//! domain must require a new model pack and no runtime change.
//!
//! # Layering
//!
//! A model pack has sections owned by different reasoners — `rules.yaml`
//! by the symbolic reasoner, `geometry.yaml` by the geometric one, and so
//! on. The obvious design has this crate know all of those types, but that
//! makes `dre-model` depend on every reasoner while every reasoner needs
//! the pack, which is a dependency cycle.
//!
//! So this crate owns the manifest, the ontology, and the section
//! *container*, holding each section as an unparsed [`SectionSource`]. The
//! owning reasoner parses its own section. Adding a reasoner then touches
//! that reasoner and the assembly point, never this crate.
//!
//! Status: skeleton. The loader and validator are not yet implemented, and
//! the YAML parser is an open decision — see `docs/decisions.md`.

pub mod loader;
pub mod yaml;

pub use loader::{load_dir, parse_section, LoadError};

use dre_core::canonical::{Canonical, CanonicalWriter};
use dre_core::collections::DetMap;
use dre_core::hash::Hash32;
use dre_core::id::Id;
use dre_core::version::Version;
use serde::{Deserialize, Serialize};

/// The sections a model pack may declare, per plan section 9.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    /// Entity and relationship types.
    Ontology,
    /// Derived feature declarations.
    Features,
    /// Symbolic inference rules.
    Rules,
    /// Geometric space, dimensions, and regions.
    Geometry,
    /// Actions, preconditions, and effects.
    Causal,
    /// Declared hypotheses and the observations they would account for.
    Explanations,
    /// Constraints candidate results must satisfy.
    Constraints,
    /// Target states.
    Goals,
}

impl SectionKind {
    /// Canonical file stem for this section within a pack directory.
    #[must_use]
    pub const fn file_stem(self) -> &'static str {
        match self {
            Self::Ontology => "ontology",
            Self::Features => "features",
            Self::Rules => "rules",
            Self::Geometry => "geometry",
            Self::Causal => "causal",
            Self::Explanations => "explanations",
            Self::Constraints => "constraints",
            Self::Goals => "goals",
        }
    }

    /// Every section kind, in a fixed order.
    #[must_use]
    pub const fn all() -> [Self; 8] {
        [
            Self::Ontology,
            Self::Features,
            Self::Rules,
            Self::Geometry,
            Self::Causal,
            Self::Explanations,
            Self::Constraints,
            Self::Goals,
        ]
    }
}

/// Unparsed source text for one section.
///
/// Hashed as raw bytes, so the `ModelHash` covers exactly what was on disk.
/// Hashing a parsed representation instead would let a parser change alter
/// stored hashes without the model having changed at all.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SectionSource {
    /// Which section this is.
    pub kind: SectionKind,
    /// Raw file bytes.
    pub bytes: Vec<u8>,
}

impl Canonical for SectionSource {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("section")
            .write_str(self.kind.file_stem())
            .write_bytes(&self.bytes);
    }
}

/// Model pack identity and metadata, from `model.yaml`.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelManifest {
    /// Pack identifier.
    pub id: Id,
    /// Pack version.
    pub version: Version,
    /// Minimum engine version this pack requires.
    pub requires_engine: Version,
    /// Human-facing description.
    #[serde(default)]
    pub description: String,
}

impl Canonical for ModelManifest {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("manifest")
            .write(&self.id)
            .write(&self.version)
            .write(&self.requires_engine)
            .write_str(&self.description);
    }
}

/// A loaded model pack, before section parsing.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ModelPack {
    /// Identity and metadata.
    pub manifest: ModelManifest,
    /// Declared sections, ordered by kind. Sections are optional: a pure
    /// logic pack declares no geometry.
    pub sections: DetMap<SectionKind, SectionSource>,
}

impl ModelPack {
    /// The `ModelHash` for this pack.
    #[must_use]
    pub fn model_hash(&self) -> Hash32 {
        self.canonical_hash()
    }

    /// Borrow a section, if declared.
    #[must_use]
    pub fn section(&self, kind: SectionKind) -> Option<&SectionSource> {
        self.sections.get(&kind)
    }
}

impl Canonical for ModelPack {
    fn write_canonical(&self, out: &mut CanonicalWriter) {
        out.tag("model-pack").write(&self.manifest);
        out.write_u64(self.sections.len() as u64);
        for (kind, source) in self.sections.iter() {
            out.write_str(kind.file_stem()).write(source);
        }
    }
}
