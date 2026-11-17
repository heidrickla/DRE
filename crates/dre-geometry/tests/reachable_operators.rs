//! The geometry operators plan section 16 requires, exercised.
//!
//! Five were specified: `DISTANCE`, `NEAREST_REGION`, `INSIDE_REGION`,
//! `BOUNDARY_DISTANCE` and `DIRECTION`. All five were implemented. Three
//! of them — `nearest_region`, `boundary_distance` and `direction` — had
//! **zero call sites anywhere in the workspace**, tests included, and two
//! were unreachable from any model pack at all.
//!
//! That is a milder fault than the ones this project has been finding:
//! nothing hashed them and nothing claimed they had run, so no result was
//! ever wrong because of them. But an operator a pack cannot invoke is not
//! an implemented operator, and the plan said all five were done.
//!
//! `boundary_distance` and containment are now writable into properties by
//! a pack. `direction` is not, and the reason is recorded rather than
//! quietly dropped: it returns one ordering per dimension, and no property
//! kind holds a vector. Naming a property per axis is a format decision,
//! not an oversight to patch here — so it is tested as a library function
//! and left unreachable, deliberately.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::cmp::Ordering;

use dre_core::collections::DetMap;
use dre_core::fixed::Fixed;
use dre_core::id::Id;
use dre_geometry::{
    boundary_distance, direction, nearest_region, rank_regions, Dimension, Interval, Point, Region,
    Space,
};

fn id(value: &str) -> Id {
    Id::new(value).expect("valid id")
}

fn fixed(value: i64) -> Fixed {
    Fixed::from_int(value).expect("in range")
}

fn space() -> Space {
    Space {
        dimensions: vec![
            Dimension {
                id: id("temperature"),
                weight: Fixed::ONE,
            },
            Dimension {
                id: id("vibration"),
                weight: Fixed::ONE,
            },
        ],
    }
}

fn point(temperature: i64, vibration: i64) -> Point {
    let mut out = Point::new();
    out.insert(id("temperature"), fixed(temperature));
    out.insert(id("vibration"), fixed(vibration));
    out
}

/// A region bounded on both axes, centred at (800, 10).
fn hot() -> Region {
    let mut bounds = DetMap::new();
    bounds.insert(
        id("temperature"),
        Interval {
            min: fixed(700),
            max: fixed(900),
        },
    );
    bounds.insert(
        id("vibration"),
        Interval {
            min: fixed(0),
            max: fixed(20),
        },
    );

    Region {
        id: id("HOT"),
        centroid: point(800, 10),
        bounds,
    }
}

// ---------------------------------------------------------------------
// BOUNDARY_DISTANCE
// ---------------------------------------------------------------------

#[test]
fn a_point_inside_the_region_is_zero_from_its_boundary() {
    let distance = boundary_distance(&space(), &hot(), &point(800, 10)).expect("computable");
    assert_eq!(distance, Fixed::ZERO);
}

#[test]
fn a_point_on_the_boundary_is_zero_from_it() {
    // The interval is inclusive, so the edge counts as inside.
    let distance = boundary_distance(&space(), &hot(), &point(900, 20)).expect("computable");
    assert_eq!(distance, Fixed::ZERO);
}

#[test]
fn a_point_outside_grows_with_the_overshoot() {
    // 10 over on temperature, nothing on vibration. The function squares
    // each overshoot, matching the ranking metric rather than mixing two
    // notions of distance in one space.
    let near = boundary_distance(&space(), &hot(), &point(910, 10)).expect("computable");
    let far = boundary_distance(&space(), &hot(), &point(1000, 10)).expect("computable");

    assert!(near > Fixed::ZERO, "10 outside must not read as inside");
    assert!(far > near, "further out must be further out");
}

#[test]
fn overshoot_on_two_axes_exceeds_overshoot_on_one() {
    let one = boundary_distance(&space(), &hot(), &point(910, 10)).expect("computable");
    let two = boundary_distance(&space(), &hot(), &point(910, 30)).expect("computable");
    assert!(two > one);
}

#[test]
fn an_unbounded_dimension_contributes_nothing() {
    // Matching `Region::contains`, which ignores dimensions the region does
    // not bound. If these two disagreed, a point could be "inside" and a
    // non-zero distance from the boundary at once.
    let mut bounds = DetMap::new();
    bounds.insert(
        id("temperature"),
        Interval {
            min: fixed(700),
            max: fixed(900),
        },
    );
    let partial = Region {
        id: id("WARM"),
        centroid: point(800, 10),
        bounds,
    };

    let distance = boundary_distance(&space(), &partial, &point(800, 9_000)).expect("computable");
    assert_eq!(distance, Fixed::ZERO);
    assert!(partial.contains(&point(800, 9_000)));
}

// ---------------------------------------------------------------------
// INSIDE_REGION, and why it is not NEAREST_REGION
// ---------------------------------------------------------------------

#[test]
fn the_nearest_region_need_not_contain_the_point() {
    // The distinction the two properties exist to keep apart. A point far
    // outside every region still has a nearest one, and a pack reading the
    // classification as membership would conclude the opposite of the
    // truth.
    let regions = [hot()];
    let outside = point(5_000, 10);

    let ranked = nearest_region(&space(), &regions, &outside).expect("computable");
    assert_eq!(
        ranked.expect("a nearest region exists").region,
        id("HOT"),
        "every point has a nearest region"
    );
    assert!(
        !hot().contains(&outside),
        "and this one is nowhere near inside it"
    );
}

#[test]
fn nearest_region_agrees_with_the_head_of_the_ranking() {
    // `nearest_region` is `rank_regions().next()`, and the runner uses the
    // ranking directly. Pinning the equivalence keeps the convenience
    // wrapper honest rather than merely present.
    let regions = [hot()];
    let here = point(800, 10);

    let ranked = rank_regions(&space(), &regions, &here).expect("computable");
    let nearest = nearest_region(&space(), &regions, &here).expect("computable");

    assert_eq!(nearest.expect("some region").region, ranked[0].region);
}

#[test]
fn no_regions_means_no_nearest_region() {
    let nearest = nearest_region(&space(), &[], &point(800, 10)).expect("computable");
    assert!(nearest.is_none());
}

// ---------------------------------------------------------------------
// DIRECTION — implemented, tested, deliberately not reachable from a pack
// ---------------------------------------------------------------------

#[test]
fn direction_reports_a_sign_per_dimension() {
    let signs = direction(&space(), &point(700, 30), &point(800, 10)).expect("computable");

    assert_eq!(signs.get(&id("temperature")), Some(&Ordering::Greater));
    assert_eq!(signs.get(&id("vibration")), Some(&Ordering::Less));
}

#[test]
fn direction_to_the_same_point_is_equal_on_every_axis() {
    let here = point(800, 10);
    let signs = direction(&space(), &here, &here).expect("computable");

    assert!(signs.values().all(|sign| *sign == Ordering::Equal));
}

#[test]
fn direction_is_antisymmetric() {
    let a = point(700, 30);
    let b = point(800, 10);

    let forward = direction(&space(), &a, &b).expect("computable");
    let backward = direction(&space(), &b, &a).expect("computable");

    for (axis, sign) in forward.iter() {
        assert_eq!(
            backward.get(axis).map(|other| other.reverse()),
            Some(*sign),
            "{axis} disagreed when the endpoints were swapped"
        );
    }
}

#[test]
fn direction_needs_both_endpoints_on_every_axis() {
    // It returns an error rather than treating a missing coordinate as
    // zero, which would report a direction the state never supported.
    let mut partial = Point::new();
    partial.insert(id("temperature"), fixed(700));

    assert!(direction(&space(), &partial, &point(800, 10)).is_err());
}
