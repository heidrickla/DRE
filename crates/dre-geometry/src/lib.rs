//! Phase 6 — Geometric reasoner.
//!
//! Treats state as a point in a weighted space and reasons about where it
//! sits relative to declared regions.
//!
//! Two deliberate restrictions, both from plan section 12. Distances are
//! *squared* and never square-rooted, because squared distance preserves
//! ordering and a fixed-point square root would introduce a rounding step
//! whose direction is one more thing to specify. And regions start
//! rectangular with centroids: manifolds and learned embeddings are
//! explicitly deferred, since the point of the MVP is to find out whether
//! the substrate works, not to make the geometry sophisticated.
//!
//! Status: skeleton. Distance and containment are implemented and tested;
//! nearest-region search, boundary distance, and direction are not.

use dre_core::collections::DetMap;
use dre_core::fixed::Fixed;
use dre_core::id::Id;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Failures in geometric reasoning.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum GeometryError {
    /// A point did not supply a coordinate for a declared dimension.
    ///
    /// An error rather than a default of zero: silently substituting zero
    /// would place the point somewhere the model never described, and the
    /// resulting distance would look perfectly reasonable.
    #[error("point is missing coordinate for dimension {dimension}")]
    MissingCoordinate {
        /// The dimension with no coordinate.
        dimension: Id,
    },

    /// Fixed-point arithmetic failed while computing a distance.
    #[error("arithmetic failure computing {operation}: {source}")]
    Arithmetic {
        /// What was being computed.
        operation: &'static str,
        /// The underlying fixed-point error.
        #[source]
        source: dre_core::CoreError,
    },
}

/// Result alias for geometric reasoning.
pub type Result<T> = std::result::Result<T, GeometryError>;

/// A coordinate in the space, keyed by dimension.
pub type Point = DetMap<Id, Fixed>;

/// One axis of the space.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dimension {
    /// Dimension identifier, matching a property name.
    pub id: Id,
    /// Relative weight in distance calculations.
    ///
    /// Present because dimensions carry different units — a temperature
    /// delta of 100 and a pressure delta of 100 are not comparable
    /// magnitudes, and an unweighted distance would let whichever quantity
    /// happens to have the larger numeric range dominate.
    pub weight: Fixed,
}

/// A weighted state space.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Space {
    /// Axes, in declaration order.
    pub dimensions: Vec<Dimension>,
}

/// A closed interval along one dimension.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interval {
    /// Inclusive lower bound.
    pub min: Fixed,
    /// Inclusive upper bound.
    pub max: Fixed,
}

impl Interval {
    /// Whether a value lies within the interval, bounds inclusive.
    #[must_use]
    pub fn contains(&self, value: Fixed) -> bool {
        value >= self.min && value <= self.max
    }
}

/// A named rectangular region with a centroid.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    /// Unique identifier. Also the tie-break when two regions are
    /// equidistant, which keeps nearest-region search total.
    pub id: Id,
    /// Representative point, used for distance ranking.
    ///
    /// Required: a region with no centroid cannot be ranked, and ranking is
    /// what classification is.
    pub centroid: Point,

    /// Axis-aligned bounds, used for containment.
    ///
    /// Optional, and empty means unbounded. That follows from what
    /// [`Region::contains`] already documents — a region may bound only
    /// some dimensions and be a slab through the space rather than a box —
    /// and a region bounding *none* of them is the limit of that, not a
    /// special case. Requiring the key made a purely centroid-defined
    /// region inexpressible for no reason anything else in the design
    /// asked for.
    #[serde(default)]
    pub bounds: DetMap<Id, Interval>,
}

impl Region {
    /// Whether a point lies inside every declared bound.
    ///
    /// Dimensions the region does not bound are unconstrained, so a region
    /// may be a slab through the space rather than a full box.
    #[must_use]
    pub fn contains(&self, point: &Point) -> bool {
        self.bounds.iter().all(|(dimension, interval)| {
            point
                .get(dimension)
                .is_some_and(|value| interval.contains(*value))
        })
    }
}

/// Weighted squared distance between two points.
///
/// Not square-rooted: ordering is preserved, and taking a fixed-point root
/// would add a rounding decision without changing any comparison outcome.
pub fn weighted_squared_distance(space: &Space, from: &Point, to: &Point) -> Result<Fixed> {
    let mut total = Fixed::ZERO;

    for dimension in space.dimensions.iter() {
        let left = *from
            .get(&dimension.id)
            .ok_or_else(|| GeometryError::MissingCoordinate {
                dimension: dimension.id.clone(),
            })?;
        let right = *to
            .get(&dimension.id)
            .ok_or_else(|| GeometryError::MissingCoordinate {
                dimension: dimension.id.clone(),
            })?;

        let arithmetic = |operation: &'static str| {
            move |source: dre_core::CoreError| GeometryError::Arithmetic { operation, source }
        };

        let delta = left.checked_sub(right).map_err(arithmetic("delta"))?;
        let squared = delta.checked_mul(delta).map_err(arithmetic("square"))?;
        let weighted = squared
            .checked_mul(dimension.weight)
            .map_err(arithmetic("weight"))?;
        total = total
            .checked_add(weighted)
            .map_err(arithmetic("accumulate"))?;
    }

    Ok(total)
}

/// The `geometry.yaml` section of a model pack.
#[derive(Clone, PartialEq, Eq, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GeometrySection {
    /// The space the entity's properties are read into.
    pub space: SpaceDecl,
    /// Declared regions.
    #[serde(default)]
    pub regions: Vec<Region>,
}

/// A declared space, naming the entity **type** whose properties form the
/// point.
///
/// One space, any number of subjects: every entity of the named type is
/// read into the space and classified separately, writing its own result
/// onto itself.
///
/// The type is what makes the section usable by a domain with more than one
/// interesting entity. It named a single entity until 2026-08-15, which no
/// pack could detect — every geometric pack called its one entity the same
/// thing as its type, so the two lookups agreed. A domain with sixty
/// components could not use geometry at all, and found it.
#[derive(Clone, PartialEq, Eq, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpaceDecl {
    /// Entity **type** whose instances are read as points. Every entity
    /// declaring this type is classified.
    pub subject: Id,
    /// Property to write the classification into, on each classified
    /// entity.
    #[serde(default = "default_classification")]
    pub classify_into: Id,
    /// Property to write the distance to the nearest region's boundary
    /// into. Absent means the distance is not computed.
    ///
    /// Opt-in because it is only meaningful for a pack that reasons about
    /// margins — "how far inside the safe band is it" rather than "which
    /// band is it in". A pack that does not ask does not pay for it, and
    /// its state does not gain a property it never mentions.
    #[serde(default)]
    pub boundary_distance_into: Option<Id>,
    /// Property to write region containment into. Absent means it is not
    /// computed.
    ///
    /// Distinct from `classify_into`, which always names the *nearest*
    /// region whether or not the point is in it. A point can be classified
    /// `SAFE` while sitting outside every declared region, and a pack that
    /// cannot tell those apart is reading a ranking as a membership test.
    #[serde(default)]
    pub inside_region_into: Option<Id>,
    /// Axes.
    pub dimensions: Vec<Dimension>,
}

fn default_classification() -> Id {
    // The identifier charset admits this, so the unwrap cannot fire; a
    // const would be preferable but `Id` validates at construction.
    Id::new("nearest_region").unwrap_or_else(|_| unreachable!("literal is a valid identifier"))
}

impl SpaceDecl {
    /// The weighted space this declaration describes.
    #[must_use]
    pub fn space(&self) -> Space {
        Space {
            dimensions: self.dimensions.clone(),
        }
    }
}

/// A region paired with its distance from a point.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ranked {
    /// The region.
    pub region: Id,
    /// Weighted squared distance from the point.
    pub distance: Fixed,
}

/// Rank regions by distance from a point, nearest first.
///
/// Ties break on region identifier, which is unique — so the ordering is
/// total and two equidistant regions never resolve by chance. Without
/// that, a symmetric state would classify differently between runs.
pub fn rank_regions(space: &Space, regions: &[Region], point: &Point) -> Result<Vec<Ranked>> {
    let mut ranked = Vec::with_capacity(regions.len());

    for region in regions {
        let distance = weighted_squared_distance(space, point, &region.centroid)?;
        ranked.push(Ranked {
            region: region.id.clone(),
            distance,
        });
    }

    ranked.sort_by(|left, right| {
        left.distance
            .cmp(&right.distance)
            .then_with(|| left.region.cmp(&right.region))
    });

    Ok(ranked)
}

/// The region nearest a point, or `None` when none are declared.
pub fn nearest_region(space: &Space, regions: &[Region], point: &Point) -> Result<Option<Ranked>> {
    Ok(rank_regions(space, regions, point)?.into_iter().next())
}

/// Distance from a point to the nearest edge of a region's bounds.
///
/// Zero inside the region. Only bounded dimensions contribute, matching
/// [`Region::contains`].
pub fn boundary_distance(space: &Space, region: &Region, point: &Point) -> Result<Fixed> {
    let mut total = Fixed::ZERO;

    for dimension in space.dimensions.iter() {
        let Some(interval) = region.bounds.get(&dimension.id) else {
            continue;
        };
        let value = *point
            .get(&dimension.id)
            .ok_or_else(|| GeometryError::MissingCoordinate {
                dimension: dimension.id.clone(),
            })?;

        let arithmetic = |operation: &'static str| {
            move |source: dre_core::CoreError| GeometryError::Arithmetic { operation, source }
        };

        let overshoot = if value < interval.min {
            interval
                .min
                .checked_sub(value)
                .map_err(arithmetic("under"))?
        } else if value > interval.max {
            value
                .checked_sub(interval.max)
                .map_err(arithmetic("over"))?
        } else {
            Fixed::ZERO
        };

        let squared = overshoot
            .checked_mul(overshoot)
            .map_err(arithmetic("square"))?;
        let weighted = squared
            .checked_mul(dimension.weight)
            .map_err(arithmetic("weight"))?;
        total = total
            .checked_add(weighted)
            .map_err(arithmetic("accumulate"))?;
    }

    Ok(total)
}

/// Per-dimension direction from one point toward another.
///
/// Reports the sign of each component rather than a normalised vector,
/// because normalising needs a square root and the ordering-only use here
/// does not.
pub fn direction(
    space: &Space,
    from: &Point,
    to: &Point,
) -> Result<DetMap<Id, std::cmp::Ordering>> {
    let mut out = DetMap::new();

    for dimension in space.dimensions.iter() {
        let missing = || GeometryError::MissingCoordinate {
            dimension: dimension.id.clone(),
        };
        let start = *from.get(&dimension.id).ok_or_else(missing)?;
        let end = *to.get(&dimension.id).ok_or_else(missing)?;
        out.insert(dimension.id.clone(), end.cmp(&start));
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    fn id(value: &str) -> Id {
        Id::new(value).expect("valid id")
    }

    fn space(axes: &[(&str, i64)]) -> Space {
        Space {
            dimensions: axes
                .iter()
                .map(|(name, weight)| Dimension {
                    id: id(name),
                    weight: Fixed::from_int(*weight).expect("in range"),
                })
                .collect(),
        }
    }

    fn point(coordinates: &[(&str, i64)]) -> Point {
        coordinates
            .iter()
            .map(|(name, value)| (id(name), Fixed::from_int(*value).expect("in range")))
            .collect()
    }

    #[test]
    fn distance_to_self_is_zero() {
        let space = space(&[("temperature", 1), ("vibration", 1)]);
        let here = point(&[("temperature", 750), ("vibration", 830)]);
        assert_eq!(
            weighted_squared_distance(&space, &here, &here).expect("computable"),
            Fixed::ZERO
        );
    }

    #[test]
    fn distance_is_symmetric() {
        let space = space(&[("temperature", 1), ("vibration", 2)]);
        let a = point(&[("temperature", 750), ("vibration", 830)]);
        let b = point(&[("temperature", 900), ("vibration", 850)]);
        assert_eq!(
            weighted_squared_distance(&space, &a, &b).expect("computable"),
            weighted_squared_distance(&space, &b, &a).expect("computable")
        );
    }

    #[test]
    fn weight_changes_ranking() {
        // One step along temperature versus one step along vibration.
        let a = point(&[("temperature", 1), ("vibration", 0)]);
        let b = point(&[("temperature", 0), ("vibration", 1)]);
        let origin = point(&[("temperature", 0), ("vibration", 0)]);

        let weighted = space(&[("temperature", 1), ("vibration", 4)]);
        let da = weighted_squared_distance(&weighted, &origin, &a).expect("computable");
        let db = weighted_squared_distance(&weighted, &origin, &b).expect("computable");
        assert!(
            db > da,
            "the heavier axis should register the larger distance"
        );
    }

    #[test]
    fn missing_coordinate_is_an_error_not_a_default() {
        let space = space(&[("temperature", 1), ("pressure", 1)]);
        let partial = point(&[("temperature", 750)]);
        let full = point(&[("temperature", 750), ("pressure", 410)]);
        assert_eq!(
            weighted_squared_distance(&space, &partial, &full),
            Err(GeometryError::MissingCoordinate {
                dimension: id("pressure")
            })
        );
    }

    #[test]
    fn region_containment_ignores_unbounded_dimensions() {
        let region = Region {
            id: id("FAILURE_A"),
            centroid: point(&[("temperature", 900)]),
            bounds: [(
                id("temperature"),
                Interval {
                    min: Fixed::from_int(800).expect("in range"),
                    max: Fixed::from_int(1000).expect("in range"),
                },
            )]
            .into_iter()
            .collect(),
        };

        assert!(region.contains(&point(&[("temperature", 850), ("vibration", 5)])));
        assert!(!region.contains(&point(&[("temperature", 700), ("vibration", 5)])));
    }
}
