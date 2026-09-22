// SPDX-License-Identifier: MIT OR Apache-2.0
//! The client's cooked physics asset (`.rnr`, the "RnR Asset"): the collision shapes of one mesh
//! (level-collision note 2026-09-20; layout Observed in build 25810808 and read the same way as
//! the client's loader `FUN_140cc66a0` and its shape reader `FUN_1410f7ac0`).
//!
//! A mesh's `.rnr` is the catalog product of the mesh's asset id at the mesh's path with the
//! extension [`RNR_EXTENSION`] (Observed: `objects/.../galleon_body.cgf` sub 0 and
//! `galleon_body.rnr` sub 100; a slice's baked `handcart_group_a_settlement__28236438930.cgf` and
//! its `.rnr` under the slice's id at other sub ids; each of the build's 10,513 meshes with a
//! `.rnr` of its path has it under its own id). The file is little-endian:
//! the magic and version, the material lists (a `.mtl` path and one material id per triangle),
//! 16 bytes the server does not use, one or two flags, the shape count, then the shapes. A shape
//! is its kind, its data and a trailer: a `u32` length and that many bytes, which hold the
//! shape's pose (a 3 x 4 matrix) when there are 48 of them (Observed: `primitive_cube.rnr` is a
//! box of half extents 1 posed 1 m up, and `primitive_cube.cgf` spans z 0 to 2 m).
//!
//! Every one of the 10,621 `.rnr` files of the build reads to its last byte (Observed). The
//! kinds they use: triangle meshes 16,900, boxes 6,846, compounds 1,689, cylinders 410, convex
//! hulls 175, spheres 21, capsules 1. The kinds the loader knows and no file uses are read and
//! kept as [`ShapeKind::Unsupported`].

use crate::rigid::Vec3;

/// The file extension of a physics asset.
pub const RNR_EXTENSION: &str = ".rnr";
/// The magic of an `.rnr` file.
pub const RNR_MAGIC: u32 = 0x1234_abcd;
/// A 3 x 4 rigid-or-affine matrix: three rows, each the row of the 3 x 3 part and then the
/// translation along that axis (`world = rows[i][0..3] · local + rows[i][3]`).
pub type Affine = [[f32; 4]; 3];

/// The identity [`Affine`].
pub const IDENTITY_AFFINE: Affine = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// Versions from which the loader reads the header parts (`FUN_140cc66a0`).
const MATERIALS_FROM_VERSION: u32 = 2;
const HEADER_BLOCK_FROM_VERSION: u32 = 3;
const SHAPE_COUNT_FROM_VERSION: u32 = 4;
const FLAGS_FROM_VERSION: u32 = 5;
const SECOND_FLAG_FROM_VERSION: u32 = 7;
/// Bytes of the header block the server does not use.
const HEADER_BLOCK_LEN: usize = 16;
/// Bytes of a pose trailer: 12 floats.
const POSE_LEN: usize = 48;
/// The low 31 bits of a convex hull's point count (the top bit is a flag of the loader).
const POINT_COUNT_MASK: u32 = 0x7fff_ffff;
/// Floats in a rows-with-translation matrix.
const AFFINE_FLOATS: usize = 12;
/// Columns of an [`Affine`] row.
const AFFINE_COLUMNS: usize = 4;
/// Indices per triangle.
const TRIANGLE_CORNERS: usize = 3;
/// Floats per plane: the normal and the distance.
const PLANE_FLOATS: usize = 4;

/// The shape kind numbers of the loader's switch (`FUN_1410f7ac0`).
mod kind {
    pub const BOX: u32 = 1;
    pub const SPHERE: u32 = 2;
    pub const CONVEX_HULL: u32 = 3;
    pub const THREE_FLOATS: u32 = 4;
    pub const CYLINDER: u32 = 5;
    pub const TWO_FLOATS: u32 = 6;
    pub const CAPSULE: u32 = 7;
    pub const THREE_POINTS: u32 = 8;
    pub const TRIANGLE_MESH: u32 = 10;
    pub const COMPOUND: u32 = 11;
    pub const TRANSFORMED: u32 = 12;
    pub const ORIENTED_POINTS: u32 = 0x11;
    pub const SCALED: u32 = 0x12;
    pub const SCALED_OTHER: u32 = 0x13;
    pub const HEIGHT_FIELD: u32 = 0x14;
}
/// The height-field form that carries data (`FUN_1410a2af0`).
const HEIGHT_FIELD_WITH_DATA: u32 = 1;
/// The `u32` header words of a height field with data.
const HEIGHT_FIELD_WORDS: usize = 4;

/// An `.rnr` file that cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RnrError {
    /// The file does not start with [`RNR_MAGIC`].
    #[error("not an .rnr file")]
    NotRnr,
    /// A read lies outside the file.
    #[error(".rnr file is truncated at byte {at}")]
    Truncated {
        /// The offset that was read.
        at: usize,
    },
    /// A shape kind the loader does not know.
    #[error(".rnr shape kind {kind} at byte {at} is unknown")]
    UnknownKind {
        /// The kind number.
        kind: u32,
        /// Its offset.
        at: usize,
    },
    /// A shape trailer that is neither empty nor a pose.
    #[error(".rnr shape trailer of {len} bytes at byte {at}")]
    Trailer {
        /// Its length.
        len: usize,
        /// Its offset.
        at: usize,
    },
    /// Bytes follow the last shape.
    #[error(".rnr file has {count} bytes after its shapes")]
    TrailingBytes {
        /// How many.
        count: usize,
    },
}

/// One material list: the `.mtl` file and a material id per triangle of the meshes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialList {
    /// The material file path (empty in most files).
    pub path: String,
    /// The ids.
    pub ids: Vec<u16>,
}

/// One collision shape.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// What it is.
    pub kind: ShapeKind,
    /// Where it sits in its parent's space (the trailer); `None` for the identity.
    pub pose: Option<Affine>,
}

/// The geometry of a shape, in its own space.
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeKind {
    /// A box centred on the origin (Observed: `primitive_cube.rnr`).
    Box {
        /// Half its size along each axis, m.
        half_extents: Vec3,
        /// The rounding of its edges, m.
        margin: f32,
    },
    /// A sphere on the origin (Observed: `primitive_sphere.rnr`).
    Sphere {
        /// Its radius, m.
        radius: f32,
    },
    /// The convex hull of points (the loader's "RnR ConvexHullShape").
    ConvexHull {
        /// The hull's corners.
        points: Vec<Vec3>,
        /// The face planes: the outward unit normal and the distance from the origin.
        planes: Vec<[f32; PLANE_FLOATS]>,
        /// The rounding, m.
        margin: f32,
    },
    /// A cylinder from `from` to `to` (Observed: `primitive_cylinder.rnr`).
    Cylinder {
        /// One end's centre.
        from: Vec3,
        /// The other end's centre.
        to: Vec3,
        /// Its radius, m.
        radius: f32,
        /// The rounding, m.
        margin: f32,
    },
    /// A capsule around the segment from `from` to `to` (Observed: `primitive_capsule.rnr`).
    Capsule {
        /// One end of the segment.
        from: Vec3,
        /// The other end.
        to: Vec3,
        /// Its radius, m.
        radius: f32,
    },
    /// Triangles over shared points.
    TriangleMesh {
        /// The points.
        points: Vec<Vec3>,
        /// The corners of each triangle, as indices into `points`.
        triangles: Vec<[u16; TRIANGLE_CORNERS]>,
    },
    /// Child shapes, each with its place in this shape's space (the loader's "Compound Shape
    /// children").
    Compound(Vec<(Affine, Shape)>),
    /// One child shape at a place in this shape's space.
    Transformed(Affine, Box<Shape>),
    /// One child shape scaled per axis.
    Scaled(Vec3, Box<Shape>),
    /// A kind the loader knows that no file of the build uses; its data is skipped.
    Unsupported {
        /// The kind number.
        kind: u32,
    },
}

/// A read `.rnr` file.
#[derive(Debug, Clone, PartialEq)]
pub struct RnrAsset {
    /// The format version (6 or 7 in the build).
    pub version: u32,
    /// The material lists.
    pub materials: Vec<MaterialList>,
    /// The shapes (one in every file of the build).
    pub shapes: Vec<Shape>,
}

/// A little-endian reader over the file.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], RnrError> {
        let slice = self
            .bytes
            .get(self.at..self.at + len)
            .ok_or(RnrError::Truncated { at: self.at })?;
        self.at += len;
        Ok(slice)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], RnrError> {
        let at = self.at;
        <[u8; N]>::try_from(self.take(N)?).map_err(|_| RnrError::Truncated { at })
    }

    fn u8(&mut self) -> Result<u8, RnrError> {
        self.array::<1>().map(|[byte]| byte)
    }

    fn u16(&mut self) -> Result<u16, RnrError> {
        self.array().map(u16::from_le_bytes)
    }

    fn i16(&mut self) -> Result<i16, RnrError> {
        self.array().map(i16::from_le_bytes)
    }

    fn u32(&mut self) -> Result<u32, RnrError> {
        self.array().map(u32::from_le_bytes)
    }

    fn count(&mut self) -> Result<usize, RnrError> {
        let at = self.at;
        usize::try_from(self.u32()?).map_err(|_| RnrError::Truncated { at })
    }

    fn f32(&mut self) -> Result<f32, RnrError> {
        self.array().map(f32::from_le_bytes)
    }

    fn vec3(&mut self) -> Result<Vec3, RnrError> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }

    fn floats<const N: usize>(&mut self) -> Result<[f32; N], RnrError> {
        let mut out = [0.0; N];
        for value in &mut out {
            *value = self.f32()?;
        }
        Ok(out)
    }

    fn affine(&mut self) -> Result<Affine, RnrError> {
        let flat = self.floats::<AFFINE_FLOATS>()?;
        let row = |index: usize| -> [f32; AFFINE_COLUMNS] {
            let start = index * AFFINE_COLUMNS;
            [
                flat[start],
                flat[start + 1],
                flat[start + 2],
                flat[start + 3],
            ]
        };
        Ok([row(0), row(1), row(2)])
    }

    fn u16s(&mut self, count: usize) -> Result<Vec<u16>, RnrError> {
        (0..count).map(|_| self.u16()).collect()
    }

    fn skip(&mut self, len: usize) -> Result<(), RnrError> {
        self.take(len).map(|_| ())
    }
}

/// Reads the `.rnr` file `bytes`.
///
/// # Errors
///
/// Returns [`RnrError`] when the magic is wrong, a shape kind is unknown, a trailer is neither
/// empty nor a pose, or the file is cut short or longer than its shapes.
pub fn parse_rnr(bytes: &[u8]) -> Result<RnrAsset, RnrError> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.u32()? != RNR_MAGIC {
        return Err(RnrError::NotRnr);
    }
    let version = reader.u32()?;
    let mut materials = Vec::new();
    if version >= MATERIALS_FROM_VERSION {
        for _ in 0..reader.count()? {
            let len = reader.count()?;
            let path = String::from_utf8_lossy(reader.take(len)?).into_owned();
            let count = reader.count()?;
            let ids = reader.u16s(count)?;
            materials.push(MaterialList { path, ids });
        }
    }
    if version >= HEADER_BLOCK_FROM_VERSION {
        reader.skip(HEADER_BLOCK_LEN)?;
    }
    if version >= FLAGS_FROM_VERSION {
        let has_ids = reader.u8()? != 0;
        if version >= SECOND_FLAG_FROM_VERSION {
            reader.u8()?;
        }
        if has_ids {
            let count = usize::try_from(reader.i16()?.max(0)).unwrap_or_default();
            reader.u16s(count)?;
        }
    }
    let shape_count = if version >= SHAPE_COUNT_FROM_VERSION {
        reader.u8()?
    } else {
        1
    };
    let shapes = (0..shape_count)
        .map(|_| read_shape(&mut reader))
        .collect::<Result<Vec<_>, _>>()?;
    let trailing = bytes.len() - reader.at;
    if trailing != 0 {
        return Err(RnrError::TrailingBytes { count: trailing });
    }
    Ok(RnrAsset {
        version,
        materials,
        shapes,
    })
}

/// Reads one shape: its kind, its data and its trailer.
fn read_shape(reader: &mut Reader<'_>) -> Result<Shape, RnrError> {
    let at = reader.at;
    let number = reader.u32()?;
    let kind = match number {
        kind::BOX => ShapeKind::Box {
            half_extents: reader.vec3()?,
            margin: reader.f32()?,
        },
        kind::SPHERE => ShapeKind::Sphere {
            radius: reader.f32()?,
        },
        kind::CONVEX_HULL => read_convex_hull(reader)?,
        kind::CYLINDER => ShapeKind::Cylinder {
            from: reader.vec3()?,
            to: reader.vec3()?,
            radius: reader.f32()?,
            margin: reader.f32()?,
        },
        kind::CAPSULE => ShapeKind::Capsule {
            from: reader.vec3()?,
            to: reader.vec3()?,
            radius: reader.f32()?,
        },
        kind::TRIANGLE_MESH => read_triangle_mesh(reader)?,
        kind::COMPOUND => {
            let count = reader.count()?;
            let children = (0..count)
                .map(|_| Ok((reader.affine()?, read_shape(reader)?)))
                .collect::<Result<Vec<_>, RnrError>>()?;
            ShapeKind::Compound(children)
        }
        kind::TRANSFORMED => {
            let place = reader.affine()?;
            ShapeKind::Transformed(place, Box::new(read_shape(reader)?))
        }
        kind::SCALED | kind::SCALED_OTHER => {
            reader.u32()?;
            let scale = reader.vec3()?;
            ShapeKind::Scaled(scale, Box::new(read_shape(reader)?))
        }
        kind::THREE_FLOATS => skip_floats(reader, number, 3)?,
        kind::TWO_FLOATS => skip_floats(reader, number, 2)?,
        kind::THREE_POINTS | kind::ORIENTED_POINTS => skip_floats(reader, number, 10)?,
        kind::HEIGHT_FIELD => {
            if reader.u32()? == HEIGHT_FIELD_WITH_DATA {
                reader.skip(HEIGHT_FIELD_WORDS * size_of::<u32>())?;
                reader.vec3()?;
                reader.vec3()?;
                let len = reader.count()?;
                reader.skip(len)?;
            }
            ShapeKind::Unsupported { kind: number }
        }
        kind => return Err(RnrError::UnknownKind { kind, at }),
    };
    let trailer_at = reader.at;
    let pose = match reader.count()? {
        0 => None,
        POSE_LEN => Some(reader.affine()?),
        len => {
            return Err(RnrError::Trailer {
                len,
                at: trailer_at,
            });
        }
    };
    Ok(Shape { kind, pose })
}

/// Skips `floats` floats of a kind no file uses.
fn skip_floats(reader: &mut Reader<'_>, kind: u32, floats: usize) -> Result<ShapeKind, RnrError> {
    reader.skip(floats * size_of::<f32>())?;
    Ok(ShapeKind::Unsupported { kind })
}

/// A convex hull (`FUN_1410a10b0`): the point and plane counts, the points, the planes, the
/// margin, and a flag before the face lists the server does not use.
fn read_convex_hull(reader: &mut Reader<'_>) -> Result<ShapeKind, RnrError> {
    let point_count = usize::try_from(reader.u32()? & POINT_COUNT_MASK).unwrap_or_default();
    let plane_count = reader.count()?;
    let points = (0..point_count)
        .map(|_| reader.vec3())
        .collect::<Result<Vec<_>, _>>()?;
    let planes = (0..plane_count)
        .map(|_| reader.floats::<PLANE_FLOATS>())
        .collect::<Result<Vec<_>, _>>()?;
    let margin = reader.f32()?;
    if reader.u8()? != 0 {
        for _ in 0..2 {
            let count = reader.count()?;
            reader.u16s(count)?;
        }
    }
    Ok(ShapeKind::ConvexHull {
        points,
        planes,
        margin,
    })
}

/// A triangle mesh (`FUN_1410a3930`): a word the loader skips, the point and triangle counts, the
/// points, the corner indices, a flag before one more index per corner (the edge neighbours),
/// and a sized block the loader builds its search tree from.
fn read_triangle_mesh(reader: &mut Reader<'_>) -> Result<ShapeKind, RnrError> {
    reader.u32()?;
    let point_count = reader.count()?;
    let triangle_count = reader.count()?;
    let points = (0..point_count)
        .map(|_| reader.vec3())
        .collect::<Result<Vec<_>, _>>()?;
    let triangles = (0..triangle_count)
        .map(|_| Ok([reader.u16()?, reader.u16()?, reader.u16()?]))
        .collect::<Result<Vec<_>, RnrError>>()?;
    if reader.u8()? != 0 {
        reader.skip(triangle_count * TRIANGLE_CORNERS * size_of::<u16>())?;
    }
    let tree_len = reader.count()?;
    reader.skip(tree_len)?;
    Ok(ShapeKind::TriangleMesh { points, triangles })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `engineassets/objects/default/primitive_cube.rnr` of build 25810808 (113 bytes, in
    /// `Engine.pak`): version 7, one empty material list, one box of half extents 1 posed 1 m up.
    const PRIMITIVE_CUBE: &[u8] = &[
        0xcd, 0xab, 0x34, 0x12, 0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00,
        0x00, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3f, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x80, 0x3f,
    ];

    /// A version-6 file of one triangle (the layout of
    /// `objects/crafting/props/props_cutlasskeys/ck2_struc_pirate_treehut_under_t1_c.rnr`, a
    /// one-triangle mesh, with its search-tree block cut to 4 bytes).
    fn one_triangle() -> Vec<u8> {
        let mut bytes = Vec::new();
        let words = |bytes: &mut Vec<u8>, values: &[u32]| {
            for value in values {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        };
        let floats = |bytes: &mut Vec<u8>, values: &[f32]| {
            for value in values {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        };
        words(&mut bytes, &[RNR_MAGIC, 6, 1, 0, 1]);
        bytes.extend_from_slice(&6_u16.to_le_bytes());
        bytes.extend_from_slice(&[0; HEADER_BLOCK_LEN]);
        bytes.extend_from_slice(&[0, 1]);
        words(&mut bytes, &[kind::TRIANGLE_MESH, 100, 3, 1]);
        floats(&mut bytes, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        for index in [0_u16, 1, 2] {
            bytes.extend_from_slice(&index.to_le_bytes());
        }
        bytes.push(1);
        bytes.extend_from_slice(&[0xff; 6]);
        words(&mut bytes, &[4, 0, 0]);
        bytes
    }

    #[test]
    fn the_primitive_cube_is_a_box_posed_one_metre_up() {
        let asset = parse_rnr(PRIMITIVE_CUBE).expect("cube");
        assert_eq!(asset.version, 7);
        assert_eq!(asset.shapes.len(), 1);
        let shape = &asset.shapes[0];
        assert_eq!(
            shape.kind,
            ShapeKind::Box {
                half_extents: [1.0; 3],
                margin: 0.0
            }
        );
        assert_eq!(
            shape.pose,
            Some([
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 1.0]
            ])
        );
    }

    #[test]
    fn a_triangle_mesh_keeps_its_points_and_corners() {
        let asset = parse_rnr(&one_triangle()).expect("mesh");
        assert_eq!(asset.materials[0].ids, vec![6]);
        let ShapeKind::TriangleMesh { points, triangles } = &asset.shapes[0].kind else {
            panic!("{asset:?}");
        };
        assert_eq!(points[1], [1.0, 0.0, 0.0]);
        assert_eq!(triangles, &vec![[0, 1, 2]]);
        assert_eq!(asset.shapes[0].pose, None);
    }

    #[test]
    fn a_compound_holds_its_children_with_their_places() {
        let mut bytes = Vec::new();
        for word in [RNR_MAGIC, 6, 0] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&[0; HEADER_BLOCK_LEN]);
        bytes.extend_from_slice(&[0, 1]);
        bytes.extend_from_slice(&kind::COMPOUND.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        let place = [
            1.0_f32, 0.0, 0.0, 5.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        ];
        for value in place {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&kind::SPHERE.to_le_bytes());
        bytes.extend_from_slice(&2.0_f32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        let asset = parse_rnr(&bytes).expect("compound");
        let ShapeKind::Compound(children) = &asset.shapes[0].kind else {
            panic!("{asset:?}");
        };
        assert_eq!(children[0].0[0][3], 5.0);
        assert_eq!(children[0].1.kind, ShapeKind::Sphere { radius: 2.0 });
    }

    #[test]
    fn other_cut_and_long_files_are_errors() {
        assert_eq!(parse_rnr(b"CrCh\0\0\0\0"), Err(RnrError::NotRnr));
        let cut = &PRIMITIVE_CUBE[..PRIMITIVE_CUBE.len() - 1];
        assert!(matches!(parse_rnr(cut), Err(RnrError::Truncated { .. })));
        let mut long = PRIMITIVE_CUBE.to_vec();
        long.push(0);
        assert_eq!(parse_rnr(&long), Err(RnrError::TrailingBytes { count: 1 }));
        let mut unknown = one_triangle();
        let kind_at = 4 * 5 + 2 + HEADER_BLOCK_LEN + 2;
        unknown[kind_at] = 9;
        assert!(matches!(
            parse_rnr(&unknown),
            Err(RnrError::UnknownKind { kind: 9, .. })
        ));
    }
}
