// SPDX-License-Identifier: AGPL-3.0-or-later
//! The level collision file (level-collision note 2026-09-20): the solid shapes of a level that
//! a player's character collides with, as the client builds them from the level data.
//!
//! Each placed entity with a `GameRigidBodyComponent` whose collision filter blocks the
//! `Player` filter is a [`Body`]: one shape of its physics asset (`.rnr`) or of its own
//! collision shape, placed in the world. Shapes are kept once in [`LevelCollision::geometries`]
//! and shared by the bodies that place them. The solids of slices that a spawner places later
//! (an encounter's fog wall, an event's walls: not on initialize) are kept apart as
//! [`DeferredSlice`]s, which the server adds while it shows them. The file is the shared header of
//! `nw_content::container` with magic `NWCO`, then a postcard body of one [`LevelCollision`].
//! [`crate::collision_index::CollisionIndex`] answers the floor and ray queries.

use std::path::{Path, PathBuf};

use nw_content::container::{
    ContainerError, ContainerSpec, decode_value, encode_value, read_file, write_file,
};
use nw_formats::rigid::{Vec3, cross, dot, sub};
use serde::{Deserialize, Serialize};

pub use nw_formats::rnr::{Affine, IDENTITY_AFFINE};

/// First 4 bytes of every collision file.
pub const COLLISION_MAGIC: [u8; 4] = *b"NWCO";
/// Current collision file format version.
pub const COLLISION_FORMAT_VERSION: u16 = 1;
/// Version of the record types. Raise it when a type in this module changes; the golden test
/// fails until it is raised.
pub const COLLISION_SCHEMA_VERSION: u32 = 2;
/// Header fields of the collision file.
pub const COLLISION_CONTAINER: ContainerSpec = ContainerSpec {
    magic: COLLISION_MAGIC,
    format_version: COLLISION_FORMAT_VERSION,
    schema_hash: COLLISION_SCHEMA_VERSION,
};
/// A point is on a hull face when it is this near the face's plane, m (our choice: well under
/// the millimetre of the asset data).
const HULL_PLANE_TOLERANCE_M: f32 = 1e-4;
/// Rows of an [`Affine`].
const ROWS: usize = 3;

/// The collision file path of one level: `<dir>/collision-<client_build>-<level>.bin`.
pub fn collision_file_path(dir: &Path, client_build: &str, level: &str) -> PathBuf {
    dir.join(format!("collision-{client_build}-{level}.bin"))
}

/// The solid shapes of one level.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LevelCollision {
    /// The level folder name.
    pub level: String,
    /// Each shape once, in its own space.
    pub geometries: Vec<Geometry>,
    /// The placed shapes of the level as it loads.
    pub bodies: Vec<Body>,
    /// The solids of the slices that spawners place later, each where its spawner places it.
    pub deferred: Vec<DeferredSlice>,
}

/// The solids of one slice that a spawner places after the level loads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeferredSlice {
    /// The slice's asset id: its GUID.
    pub slice: [u8; 16],
    /// The slice's asset sub id.
    pub sub_id: u32,
    /// Where the spawner places the slice's origin.
    pub origin: Vec3,
    /// Its placed shapes (with those of the slices it spawns on initialize).
    pub bodies: Vec<Body>,
}

/// One shape in its own space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Geometry {
    /// Triangles; a face's outside is where its corners turn counter-clockwise.
    Mesh {
        /// The points.
        points: Vec<Vec3>,
        /// The corners of each triangle.
        triangles: Vec<[u16; 3]>,
    },
    /// A box centred on the origin.
    Box {
        /// Half its size along each axis.
        half_extents: Vec3,
    },
    /// A sphere on the origin.
    Sphere {
        /// Its radius.
        radius: f32,
    },
    /// A capsule around the segment from `from` to `to`.
    Capsule {
        /// One end of the segment.
        from: Vec3,
        /// The other end.
        to: Vec3,
        /// Its radius.
        radius: f32,
    },
    /// A capped cylinder from `from` to `to`.
    Cylinder {
        /// One cap's centre.
        from: Vec3,
        /// The other cap's centre.
        to: Vec3,
        /// Its radius.
        radius: f32,
    },
    /// A convex solid: its corners and the planes of its faces (`normal · x <= distance`
    /// inside; the normal points out).
    Convex {
        /// The corners.
        points: Vec<Vec3>,
        /// The face planes: outward unit normal and distance.
        planes: Vec<[f32; 4]>,
    },
}

/// One shape placed in the world.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Body {
    /// The shape, by index into [`LevelCollision::geometries`].
    pub geometry: u32,
    /// World from the shape's space (may scale).
    pub place: Affine,
}

impl LevelCollision {
    /// The deferred slice of asset id `slice` / `sub_id` whose origin is nearest `at` and at most
    /// `within` metres from it.
    pub fn deferred_at(
        &self,
        slice: [u8; 16],
        sub_id: u32,
        at: Vec3,
        within: f32,
    ) -> Option<&DeferredSlice> {
        self.deferred
            .iter()
            .filter(|deferred| deferred.slice == slice && deferred.sub_id == sub_id)
            .map(|deferred| {
                let offset = sub(deferred.origin, at);
                (dot(offset, offset).sqrt(), deferred)
            })
            .filter(|(distance, _)| *distance <= within)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, deferred)| deferred)
    }

    /// Encodes the collision with its header.
    pub fn to_bytes(&self, client_build: &str) -> Result<Vec<u8>, ContainerError> {
        encode_value(&COLLISION_CONTAINER, client_build, self)
    }

    /// Checks the header, then decodes the collision.
    pub fn from_bytes(bytes: &[u8], expected_build: &str) -> Result<Self, ContainerError> {
        decode_value(&COLLISION_CONTAINER, bytes, expected_build)
    }

    /// Writes the collision file to `path`, creating parent folders.
    pub fn save(&self, path: &Path, client_build: &str) -> Result<(), ContainerError> {
        write_file(path, &self.to_bytes(client_build)?)
    }

    /// Reads and checks the collision file at `path`.
    pub fn load(path: &Path, expected_build: &str) -> Result<Self, ContainerError> {
        Self::from_bytes(&read_file(path)?, expected_build)
    }
}

/// `a` after `b`: the place of `b`'s space in the space `a` maps from.
pub fn compose(a: &Affine, b: &Affine) -> Affine {
    let mut out = [[0.0; 4]; ROWS];
    for (row, out_row) in out.iter_mut().enumerate() {
        for (column, value) in out_row.iter_mut().enumerate() {
            let mut sum: f32 = (0..ROWS).map(|k| a[row][k] * b[k][column]).sum();
            if column == ROWS {
                sum += a[row][ROWS];
            }
            *value = sum;
        }
    }
    out
}

/// The point `point` of the space `place` maps from, in the space it maps to.
pub fn transform_point(place: &Affine, point: Vec3) -> Vec3 {
    let row = |r: &[f32; 4]| r[0] * point[0] + r[1] * point[1] + r[2] * point[2] + r[3];
    [row(&place[0]), row(&place[1]), row(&place[2])]
}

/// The direction `vector` mapped by `place` (no translation).
pub fn transform_vector(place: &Affine, vector: Vec3) -> Vec3 {
    let row = |r: &[f32; 4]| r[0] * vector[0] + r[1] * vector[1] + r[2] * vector[2];
    [row(&place[0]), row(&place[1]), row(&place[2])]
}

/// The inverse of `place`, or `None` when it flattens space.
pub fn invert(place: &Affine) -> Option<Affine> {
    let m = |r: usize, c: usize| place[r][c];
    let cofactor =
        |r0: usize, r1: usize, c0: usize, c1: usize| m(r0, c0) * m(r1, c1) - m(r0, c1) * m(r1, c0);
    let determinant = m(0, 0) * cofactor(1, 2, 1, 2) - m(0, 1) * cofactor(1, 2, 0, 2)
        + m(0, 2) * cofactor(1, 2, 0, 1);
    if determinant.abs() <= f32::EPSILON * f32::EPSILON {
        return None;
    }
    let inverse_det = determinant.recip();
    let linear = [
        [
            cofactor(1, 2, 1, 2) * inverse_det,
            -cofactor(0, 2, 1, 2) * inverse_det,
            cofactor(0, 1, 1, 2) * inverse_det,
        ],
        [
            -cofactor(1, 2, 0, 2) * inverse_det,
            cofactor(0, 2, 0, 2) * inverse_det,
            -cofactor(0, 1, 0, 2) * inverse_det,
        ],
        [
            cofactor(1, 2, 0, 1) * inverse_det,
            -cofactor(0, 2, 0, 1) * inverse_det,
            cofactor(0, 1, 0, 1) * inverse_det,
        ],
    ];
    let translation = [m(0, 3), m(1, 3), m(2, 3)];
    let mut out = [[0.0; 4]; ROWS];
    for (row, out_row) in out.iter_mut().enumerate() {
        out_row[..ROWS].copy_from_slice(&linear[row]);
        out_row[ROWS] = -dot(linear[row], translation);
    }
    Some(out)
}

/// The place of a rotation quaternion `[x, y, z, w]`, a uniform `scale` and a `translation`.
pub fn place_of(rotation: [f32; 4], scale: f32, translation: Vec3) -> Affine {
    let [x, y, z, w] = rotation;
    let rows = [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ];
    let mut out = [[0.0; 4]; ROWS];
    for (row, out_row) in out.iter_mut().enumerate() {
        for column in 0..ROWS {
            out_row[column] = rows[row][column] * scale;
        }
        out_row[ROWS] = translation[row];
    }
    out
}

/// The face planes of the convex hull of `points` (`normal · x <= distance` inside): every
/// plane through three of the points that has all points on its inner side, once.
pub fn hull_planes(points: &[Vec3]) -> Vec<[f32; 4]> {
    let mut planes: Vec<[f32; 4]> = Vec::new();
    for (i, &a) in points.iter().enumerate() {
        for (j, &b) in points.iter().enumerate().skip(i + 1) {
            for &c in points.iter().skip(j + 1) {
                let normal = cross(sub(b, a), sub(c, a));
                let length = dot(normal, normal).sqrt();
                if length <= f32::EPSILON {
                    continue;
                }
                let normal = [normal[0] / length, normal[1] / length, normal[2] / length];
                let distance = dot(normal, a);
                let side = |sign: f32| {
                    points
                        .iter()
                        .all(|&p| sign * (dot(normal, p) - distance) <= HULL_PLANE_TOLERANCE_M)
                };
                for sign in [1.0_f32, -1.0] {
                    if !side(sign) {
                        continue;
                    }
                    let plane = [
                        sign * normal[0],
                        sign * normal[1],
                        sign * normal[2],
                        sign * distance,
                    ];
                    let known = planes.iter().any(|known| {
                        known
                            .iter()
                            .zip(&plane)
                            .all(|(k, p)| (k - p).abs() <= HULL_PLANE_TOLERANCE_M)
                    });
                    if !known {
                        planes.push(plane);
                    }
                }
            }
        }
    }
    planes
}

#[cfg(test)]
mod tests {
    use nw_content::container::encode_container;

    use super::*;

    const BUILD: &str = "25810808";
    const TOLERANCE: f32 = 1e-5;
    /// postcard body of [`sample`]. If this test fails, the record types changed: raise
    /// `COLLISION_SCHEMA_VERSION` and replace these bytes with the new body.
    const GOLDEN_BODY: &[u8] = &[
        2, 108, 118, 6, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 128, 63, 0,
        0, 0, 64, 0, 0, 64, 64, 2, 0, 0, 128, 64, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 128, 63, 0, 0, 0, 63, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 64, 0, 0, 128, 63, 5, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 63, 0, 0, 128, 63, 1, 1, 0, 0, 128, 63, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 32, 65, 0, 0, 0, 0, 0, 0, 128, 63, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 128, 63, 0, 0, 0, 0, 1, 129, 129, 129, 129, 129, 129, 129, 129, 129, 129, 129,
        129, 129, 129, 129, 129, 200, 1, 0, 0, 128, 63, 0, 0, 0, 64, 0, 0, 64, 64, 1, 2, 0, 0, 128,
        63, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 63, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 63, 0, 0, 0, 0,
    ];

    /// One geometry of every kind and one body.
    fn sample() -> LevelCollision {
        LevelCollision {
            level: "lv".to_owned(),
            geometries: vec![
                Geometry::Mesh {
                    points: vec![[0.0; 3]],
                    triangles: vec![[0, 1, 0]],
                },
                Geometry::Box {
                    half_extents: [1.0, 2.0, 3.0],
                },
                Geometry::Sphere { radius: 4.0 },
                Geometry::Capsule {
                    from: [0.0; 3],
                    to: [0.0, 0.0, 1.0],
                    radius: 0.5,
                },
                Geometry::Cylinder {
                    from: [0.0; 3],
                    to: [0.0, 0.0, 2.0],
                    radius: 1.0,
                },
                Geometry::Convex {
                    points: vec![[0.0; 3]],
                    planes: vec![[0.0, 0.0, 1.0, 1.0]],
                },
            ],
            bodies: vec![Body {
                geometry: 1,
                place: [
                    [1.0, 0.0, 0.0, 10.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ],
            }],
            deferred: vec![DeferredSlice {
                slice: [0x81; 16],
                sub_id: 200,
                origin: [1.0, 2.0, 3.0],
                bodies: vec![Body {
                    geometry: 2,
                    place: IDENTITY_AFFINE,
                }],
            }],
        }
    }

    #[test]
    fn a_deferred_slice_is_found_by_its_id_near_its_origin() {
        let level = sample();
        assert!(
            level
                .deferred_at([0x81; 16], 200, [1.5, 2.0, 3.0], 1.0)
                .is_some()
        );
        assert!(
            level
                .deferred_at([0x81; 16], 200, [5.0, 2.0, 3.0], 1.0)
                .is_none()
        );
        assert!(
            level
                .deferred_at([0x81; 16], 2, [1.0, 2.0, 3.0], 1.0)
                .is_none()
        );
    }

    #[test]
    fn golden_body_bytes() {
        let sample = sample();
        let body = postcard::to_allocvec(&sample).expect("encode");
        assert_eq!(body, GOLDEN_BODY, "raise COLLISION_SCHEMA_VERSION");
        let bytes = encode_container(&COLLISION_CONTAINER, BUILD, GOLDEN_BODY).expect("header");
        assert_eq!(
            LevelCollision::from_bytes(&bytes, BUILD).expect("decode"),
            sample
        );
    }

    #[test]
    fn a_saved_file_loads_the_same_and_another_build_is_refused() {
        let dir = tempfile::tempdir().expect("dir");
        let path = collision_file_path(dir.path(), BUILD, "lv");
        sample().save(&path, BUILD).expect("save");
        assert_eq!(LevelCollision::load(&path, BUILD).expect("load"), sample());
        assert!(LevelCollision::load(&path, "1").is_err());
        assert!(path.ends_with("collision-25810808-lv.bin"));
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        a.iter().zip(&b).all(|(x, y)| (x - y).abs() < TOLERANCE)
    }

    #[test]
    fn places_compose_and_invert() {
        let quarter_turn = place_of(
            [
                0.0,
                0.0,
                std::f32::consts::FRAC_1_SQRT_2,
                std::f32::consts::FRAC_1_SQRT_2,
            ],
            2.0,
            [10.0, 0.0, 5.0],
        );
        // x turns to y and doubles, then moves.
        assert!(near(
            transform_point(&quarter_turn, [1.0, 0.0, 0.0]),
            [10.0, 2.0, 5.0]
        ));
        let up = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 1.0],
        ];
        let both = compose(&quarter_turn, &up);
        assert!(near(transform_point(&both, [0.0; 3]), [10.0, 0.0, 7.0]));
        let back = invert(&both).expect("invertible");
        assert!(near(
            transform_point(&back, transform_point(&both, [0.3, -2.0, 4.0])),
            [0.3, -2.0, 4.0]
        ));
        assert!(near(
            transform_vector(&both, [0.0, 0.0, 1.0]),
            [0.0, 0.0, 2.0]
        ));
        let flat = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0; 4]];
        assert_eq!(invert(&flat), None);
    }

    #[test]
    fn the_hull_of_a_cube_has_six_faces() {
        let corners: Vec<Vec3> = (0..8)
            .map(|bits| {
                let axis = |bit: u32| if bits & (1 << bit) == 0 { -1.0 } else { 1.0 };
                [axis(0), axis(1), axis(2)]
            })
            .collect();
        let planes = hull_planes(&corners);
        assert_eq!(planes.len(), 6, "{planes:?}");
        assert!(planes.contains(&[0.0, 0.0, 1.0, 1.0]));
        assert!(
            planes
                .iter()
                .all(|plane| (plane[3] - 1.0).abs() < TOLERANCE)
        );
    }
}
