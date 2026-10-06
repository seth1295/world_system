//! Generic frame math and the V1 spatial topology implementations.

use core::fmt;

/// V1 right-handed body-fixed axis convention.
pub const BODY_FIXED_AXES: &str =
    "+Z is the positive rotation pole; +X is the prime meridian; right-handed";

/// Unit direction in a body-fixed frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dir {
    /// X component.
    pub x: f64,
    /// Y component.
    pub y: f64,
    /// Z component.
    pub z: f64,
}

impl Dir {
    /// Normalizes finite nonzero components into a unit direction.
    pub fn new(x: f64, y: f64, z: f64) -> Result<Self, SpatialError> {
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return Err(SpatialError::InvalidDirection);
        }
        let scale = libm::fmax(libm::fabs(x), libm::fmax(libm::fabs(y), libm::fabs(z)));
        if scale == 0.0 {
            return Err(SpatialError::InvalidDirection);
        }
        let scaled_x = x / scale;
        let scaled_y = y / scale;
        let scaled_z = z / scale;
        let length = libm::sqrt(scaled_x * scaled_x + scaled_y * scaled_y + scaled_z * scaled_z);
        Ok(Self { x: scaled_x / length, y: scaled_y / length, z: scaled_z / length })
    }

    /// Creates a direction from axial latitude and longitude in radians.
    pub fn from_axial_lat_lon(lat_rad: f64, lon_rad: f64) -> Result<Self, SpatialError> {
        if !lat_rad.is_finite() || !lon_rad.is_finite() {
            return Err(SpatialError::InvalidDirection);
        }
        let cos_lat = libm::cos(lat_rad);
        Self::new(cos_lat * libm::cos(lon_rad), cos_lat * libm::sin(lon_rad), libm::sin(lat_rad))
    }

    /// Returns display-chart axial latitude and longitude in radians.
    pub fn axial_lat_lon(self) -> (f64, f64) {
        (
            libm::atan2(self.z, libm::sqrt(self.x * self.x + self.y * self.y)),
            libm::atan2(self.y, self.x),
        )
    }
}

/// Unit quaternion for a declared frame orientation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quaternion {
    /// Scalar component.
    pub w: f64,
    /// X component.
    pub x: f64,
    /// Y component.
    pub y: f64,
    /// Z component.
    pub z: f64,
}

impl Quaternion {
    /// Validates a unit quaternion within the V1 storage tolerance.
    pub fn new(w: f64, x: f64, y: f64, z: f64) -> Result<Self, SpatialError> {
        let values = [w, x, y, z];
        if values.iter().any(|value| !value.is_finite()) {
            return Err(SpatialError::InvalidQuaternion);
        }
        let norm_squared = w * w + x * x + y * y + z * z;
        if (norm_squared - 1.0).abs() > 1.0e-12 {
            return Err(SpatialError::InvalidQuaternion);
        }
        Ok(Self { w, x, y, z })
    }
}

/// A V1 surface position in the body-fixed display chart.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pos30 {
    /// Cube face number.
    pub face: u8,
    /// 30-bit face coordinate along U.
    pub i30: u32,
    /// 30-bit face coordinate along V.
    pub j30: u32,
}

/// Within-cell coordinates and an optional radial coordinate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalPos {
    /// Addressed topology cell.
    pub cell: CellKey,
    /// Cell-local U in unsigned Q0.32 form.
    pub du: u32,
    /// Cell-local V in unsigned Q0.32 form.
    pub dv: u32,
    /// Radius in metres where the domain declares a radial coordinate.
    pub radial_m: f64,
}

/// Topology-specific cell key bits.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CellKey(pub u64);

/// Topology-specific tile key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TileKey {
    /// Cell level of the tile.
    pub level: u8,
    /// Ancestor cell key identifying the tile.
    pub address: CellKey,
}

/// Face-local cell range covered by one direction-cube tile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TileLayout {
    /// Cube face number.
    pub face: u8,
    /// First U cell index.
    pub i_start: u64,
    /// First V cell index.
    pub j_start: u64,
    /// Number of cells on each tile edge.
    pub edge: u64,
}

/// A common interface for hierarchical spatial addressing.
pub trait Topology {
    /// Stable versioned topology ID.
    fn id(&self) -> &'static str;
    /// Returns the key level after validating the key for this topology.
    fn level(&self, key: CellKey) -> Result<u8, SpatialError>;
    /// Returns the parent cell, if the key has one.
    fn parent(&self, key: CellKey) -> Result<Option<CellKey>, SpatialError>;
    /// Returns the four or two children of a cell.
    fn children(&self, key: CellKey) -> Result<Vec<CellKey>, SpatialError>;
    /// Returns the containing tile key.
    fn tile_key(&self, key: CellKey, tile_log2: u8) -> Result<TileKey, SpatialError>;
    /// Verifies that an encoded tile key is canonical for its topology and field level.
    fn validate_tile_key(&self, tile: TileKey, tile_log2: u8) -> Result<(), SpatialError>;
    /// Returns the cell measure in the topology's declared measure.
    fn cell_measure(&self, key: CellKey) -> Result<f64, SpatialError>;
    /// Returns cardinal neighbors in topology order.
    fn neighbors(&self, key: CellKey) -> Result<Vec<CellKey>, SpatialError>;
}

/// V1 direction cube topology.
#[derive(Clone, Copy, Debug, Default)]
pub struct DirCube;

/// V1 normalized radial shell topology.
#[derive(Clone, Copy, Debug)]
pub struct Radial1d {
    extent_m: f64,
}

impl Default for Radial1d {
    fn default() -> Self {
        Self { extent_m: 1.0 }
    }
}

/// Edge in a direction-cube face chart.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FaceEdge {
    /// Minimum U boundary.
    UMinus,
    /// Maximum U boundary.
    UPlus,
    /// Minimum V boundary.
    VMinus,
    /// Maximum V boundary.
    VPlus,
}

/// Face-edge transform derived from the frozen V1 table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EdgeAdjacency {
    /// Neighboring face number.
    pub face: u8,
    /// Neighboring face edge.
    pub edge: FaceEdge,
    /// Whether the along-edge index is reversed.
    pub flip: bool,
}

/// Frozen face adjacency table in U-, U+, V-, V+ order.
pub const FACE_ADJACENCY: [[EdgeAdjacency; 4]; 6] = [
    [
        edge(4, FaceEdge::VPlus, true),
        edge(1, FaceEdge::UMinus, false),
        edge(5, FaceEdge::VPlus, false),
        edge(2, FaceEdge::UMinus, true),
    ],
    [
        edge(0, FaceEdge::UPlus, false),
        edge(3, FaceEdge::VMinus, true),
        edge(5, FaceEdge::UPlus, true),
        edge(2, FaceEdge::VMinus, false),
    ],
    [
        edge(0, FaceEdge::VPlus, true),
        edge(3, FaceEdge::UMinus, false),
        edge(1, FaceEdge::VPlus, false),
        edge(4, FaceEdge::UMinus, true),
    ],
    [
        edge(2, FaceEdge::UPlus, false),
        edge(5, FaceEdge::VMinus, true),
        edge(1, FaceEdge::UPlus, true),
        edge(4, FaceEdge::VMinus, false),
    ],
    [
        edge(2, FaceEdge::VPlus, true),
        edge(5, FaceEdge::UMinus, false),
        edge(3, FaceEdge::VPlus, false),
        edge(0, FaceEdge::UMinus, true),
    ],
    [
        edge(4, FaceEdge::UPlus, false),
        edge(1, FaceEdge::VMinus, true),
        edge(3, FaceEdge::UPlus, true),
        edge(0, FaceEdge::VMinus, false),
    ],
];

const fn edge(face: u8, edge: FaceEdge, flip: bool) -> EdgeAdjacency {
    EdgeAdjacency { face, edge, flip }
}

impl DirCube {
    /// Locates a unit direction on a level-zero through level-thirty cube grid.
    pub fn locate(self, direction: Dir, level: u8) -> Result<CellKey, SpatialError> {
        if level > 30 {
            return Err(SpatialError::InvalidLevel);
        }
        let (face, u, v) = project_direction(direction);
        let s = warp_uv(u);
        let t = warp_uv(v);
        let count = 1_u64 << level;
        let i = index_coordinate(s, count);
        let j = index_coordinate(t, count);
        Self::key(face, i, j, level)
    }

    /// Returns the canonical tuple used by the direction-cube key.
    pub fn address(self, key: CellKey) -> Result<Pos30, SpatialError> {
        let (face, i, j, level) = Self::decode(key)?;
        let shift = 30 - level;
        Ok(Pos30 { face, i30: (i << shift) as u32, j30: (j << shift) as u32 })
    }

    /// Returns the cell-centre direction using the inverse quadratic warp.
    pub fn cell_center(self, key: CellKey) -> Result<Dir, SpatialError> {
        let (face, i, j, level) = Self::decode(key)?;
        let count = (1_u64 << level) as f64;
        let u = inverse_warp((i as f64 + 0.5) / count);
        let v = inverse_warp((j as f64 + 0.5) / count);
        let (x, y, z) = face_to_xyz(face, u, v)?;
        Dir::new(x, y, z)
    }

    /// Returns the level-zero through level-thirty cell key for face indices.
    pub fn key(face: u8, i: u64, j: u64, level: u8) -> Result<CellKey, SpatialError> {
        if face > 5 || level > 30 || i >= (1_u64 << level) || j >= (1_u64 << level) {
            return Err(SpatialError::InvalidCellKey);
        }
        let mut path = 0_u64;
        for bit in (0..level).rev() {
            let quadrant = (((i >> bit) & 1) << 1) | ((j >> bit) & 1);
            path = (path << 2) | quadrant;
        }
        let path_shift = 61 - 2 * level;
        let marker_shift = 60 - 2 * level;
        Ok(CellKey((u64::from(face) << 61) | (path << path_shift) | (1_u64 << marker_shift)))
    }

    /// Decodes a canonical cell key to face, indices, and level.
    pub fn decode(key: CellKey) -> Result<(u8, u64, u64, u8), SpatialError> {
        let face = (key.0 >> 61) as u8;
        if face > 5 {
            return Err(SpatialError::InvalidCellKey);
        }
        let trailing = key.0.trailing_zeros();
        if trailing > 60 || !(60 - trailing).is_multiple_of(2) {
            return Err(SpatialError::InvalidCellKey);
        }
        let level = ((60 - trailing) / 2) as u8;
        if level > 30 {
            return Err(SpatialError::InvalidCellKey);
        }
        let marker = 1_u64 << (60 - 2 * level);
        let path_shift = 61 - 2 * level;
        let mask = if level == 0 { 0 } else { (1_u64 << (2 * level)) - 1 };
        let path = (key.0 >> path_shift) & mask;
        let low_mask = marker - 1;
        if key.0 & marker == 0
            || key.0 & low_mask != 0
            || Self::key_from_path(face, path, level) != key
        {
            return Err(SpatialError::InvalidCellKey);
        }
        let mut i = 0_u64;
        let mut j = 0_u64;
        for step in 0..level {
            let shift = 2 * (level - 1 - step);
            let quadrant = (path >> shift) & 3;
            i = (i << 1) | (quadrant >> 1);
            j = (j << 1) | (quadrant & 1);
        }
        Ok((face, i, j, level))
    }

    /// Maps one cardinal edge to its adjacent cell, including cube-face transforms.
    pub fn neighbor(self, key: CellKey, edge: FaceEdge) -> Result<CellKey, SpatialError> {
        let (face, mut i, mut j, level) = Self::decode(key)?;
        let count = 1_u64 << level;
        match edge {
            FaceEdge::UMinus if i > 0 => i -= 1,
            FaceEdge::UPlus if i + 1 < count => i += 1,
            FaceEdge::VMinus if j > 0 => j -= 1,
            FaceEdge::VPlus if j + 1 < count => j += 1,
            FaceEdge::UMinus | FaceEdge::UPlus | FaceEdge::VMinus | FaceEdge::VPlus => {
                let transform = FACE_ADJACENCY[usize::from(face)][edge_index(edge)];
                let mut along =
                    if matches!(edge, FaceEdge::UMinus | FaceEdge::UPlus) { j } else { i };
                if transform.flip {
                    along = count - 1 - along;
                }
                match transform.edge {
                    FaceEdge::UMinus => {
                        i = 0;
                        j = along;
                    }
                    FaceEdge::UPlus => {
                        i = count - 1;
                        j = along;
                    }
                    FaceEdge::VMinus => {
                        j = 0;
                        i = along;
                    }
                    FaceEdge::VPlus => {
                        j = count - 1;
                        i = along;
                    }
                }
                return Self::key(transform.face, i, j, level);
            }
        }
        Self::key(face, i, j, level)
    }

    /// Returns the two cardinal stencil cells used for an out-of-range corner query.
    pub fn corner_stencil(self, key: CellKey) -> Result<Option<[CellKey; 2]>, SpatialError> {
        let (_, i, j, level) = Self::decode(key)?;
        let edge_i = if i == 0 {
            Some(FaceEdge::UMinus)
        } else if i + 1 == 1_u64 << level {
            Some(FaceEdge::UPlus)
        } else {
            None
        };
        let edge_j = if j == 0 {
            Some(FaceEdge::VMinus)
        } else if j + 1 == 1_u64 << level {
            Some(FaceEdge::VPlus)
        } else {
            None
        };
        match (edge_i, edge_j) {
            (Some(u), Some(v)) => Ok(Some([self.neighbor(key, u)?, self.neighbor(key, v)?])),
            _ => Ok(None),
        }
    }

    /// Returns a tile's edge and its face-local starting cell indices.
    pub fn tile_layout(self, key: CellKey, tile_log2: u8) -> Result<TileLayout, SpatialError> {
        if tile_log2 > 30 {
            return Err(SpatialError::InvalidTileLog2);
        }
        let (face, i, j, level) = Self::decode(key)?;
        let tile_edge = 1_u64 << level.min(tile_log2);
        let tile_i = (i / tile_edge) * tile_edge;
        let tile_j = (j / tile_edge) * tile_edge;
        Ok(TileLayout { face, i_start: tile_i, j_start: tile_j, edge: tile_edge })
    }

    fn key_from_path(face: u8, path: u64, level: u8) -> CellKey {
        let path_shift = 61 - 2 * level;
        let marker_shift = 60 - 2 * level;
        CellKey((u64::from(face) << 61) | (path << path_shift) | (1_u64 << marker_shift))
    }

    fn triangle_measure(a: Dir, b: Dir, c: Dir) -> f64 {
        let cross_x = b.y * c.z - b.z * c.y;
        let cross_y = b.z * c.x - b.x * c.z;
        let cross_z = b.x * c.y - b.y * c.x;
        let numerator = libm::fabs(a.x * cross_x + a.y * cross_y + a.z * cross_z);
        let dot_ab = a.x * b.x + a.y * b.y + a.z * b.z;
        let dot_bc = b.x * c.x + b.y * c.y + b.z * c.z;
        let dot_ca = c.x * a.x + c.y * a.y + c.z * a.z;
        2.0 * libm::atan2(numerator, 1.0 + dot_ab + dot_bc + dot_ca)
    }
}

impl Topology for DirCube {
    fn id(&self) -> &'static str {
        "veyra.topo.dir_cube/1"
    }

    fn level(&self, key: CellKey) -> Result<u8, SpatialError> {
        Self::decode(key).map(|decoded| decoded.3)
    }

    fn parent(&self, key: CellKey) -> Result<Option<CellKey>, SpatialError> {
        let (face, i, j, level) = Self::decode(key)?;
        if level == 0 {
            return Ok(None);
        }
        Ok(Some(Self::key(face, i >> 1, j >> 1, level - 1)?))
    }

    fn children(&self, key: CellKey) -> Result<Vec<CellKey>, SpatialError> {
        let (face, i, j, level) = Self::decode(key)?;
        if level == 30 {
            return Err(SpatialError::InvalidLevel);
        }
        let children: Result<Vec<_>, _> = (0_u8..4)
            .map(|quadrant| {
                Self::key(
                    face,
                    i * 2 + u64::from(quadrant >> 1),
                    j * 2 + u64::from(quadrant & 1),
                    level + 1,
                )
            })
            .collect();
        children
    }

    fn tile_key(&self, key: CellKey, tile_log2: u8) -> Result<TileKey, SpatialError> {
        if tile_log2 > 30 {
            return Err(SpatialError::InvalidTileLog2);
        }
        let (face, i, j, level) = Self::decode(key)?;
        let ancestor_level = level.saturating_sub(tile_log2);
        let shift = level - ancestor_level;
        Ok(TileKey { level, address: Self::key(face, i >> shift, j >> shift, ancestor_level)? })
    }

    fn validate_tile_key(&self, tile: TileKey, tile_log2: u8) -> Result<(), SpatialError> {
        if tile_log2 > 30 {
            return Err(SpatialError::InvalidTileLog2);
        }
        if tile.level > 30 {
            return Err(SpatialError::InvalidLevel);
        }
        let (_, _, _, address_level) = Self::decode(tile.address)?;
        if address_level != tile.level.saturating_sub(tile_log2) {
            return Err(SpatialError::InvalidTileKey);
        }
        Ok(())
    }

    fn cell_measure(&self, key: CellKey) -> Result<f64, SpatialError> {
        let (face, i, j, level) = Self::decode(key)?;
        let count = (1_u64 << level) as f64;
        let u0 = inverse_warp(i as f64 / count);
        let u1 = inverse_warp((i + 1) as f64 / count);
        let v0 = inverse_warp(j as f64 / count);
        let v1 = inverse_warp((j + 1) as f64 / count);
        let p00 = face_direction(face, u0, v0)?;
        let p10 = face_direction(face, u1, v0)?;
        let p11 = face_direction(face, u1, v1)?;
        let p01 = face_direction(face, u0, v1)?;
        Ok(Self::triangle_measure(p00, p10, p11) + Self::triangle_measure(p00, p11, p01))
    }

    fn neighbors(&self, key: CellKey) -> Result<Vec<CellKey>, SpatialError> {
        [FaceEdge::UMinus, FaceEdge::UPlus, FaceEdge::VMinus, FaceEdge::VPlus]
            .into_iter()
            .map(|edge| self.neighbor(key, edge))
            .collect()
    }
}

impl Radial1d {
    /// Creates a radial topology with the domain's physical extent in metres.
    pub fn with_extent(extent_m: f64) -> Result<Self, SpatialError> {
        if !extent_m.is_finite() || extent_m <= 0.0 {
            return Err(SpatialError::InvalidExtent);
        }
        Ok(Self { extent_m })
    }

    /// Returns the physical domain extent in metres.
    pub const fn extent_m(self) -> f64 {
        self.extent_m
    }

    /// Creates a radial shell key from level and shell index.
    pub fn key(level: u8, index: u64) -> Result<CellKey, SpatialError> {
        if level > 30 || index >= (1_u64 << level) {
            return Err(SpatialError::InvalidCellKey);
        }
        Ok(CellKey((1_u64 << level) | index))
    }

    /// Decodes a radial shell key.
    pub fn decode(key: CellKey) -> Result<(u8, u64), SpatialError> {
        if key.0 == 0 || key.0 >= (1_u64 << 31) {
            return Err(SpatialError::InvalidCellKey);
        }
        let level = (63 - key.0.leading_zeros()) as u8;
        if level > 30 {
            return Err(SpatialError::InvalidCellKey);
        }
        Ok((level, key.0 - (1_u64 << level)))
    }

    /// Returns a shell's centre radius within a declared extent.
    pub fn center_radius(self, key: CellKey) -> Result<f64, SpatialError> {
        let (level, index) = Self::decode(key)?;
        Ok((index as f64 + 0.5) / (1_u64 << level) as f64 * self.extent_m)
    }

    /// Returns the one-shell halo neighbor with endpoint clamping.
    pub fn halo_neighbor(key: CellKey, extent: i8) -> Result<CellKey, SpatialError> {
        let (level, index) = Self::decode(key)?;
        let count = 1_u64 << level;
        let next = if extent < 0 { index.saturating_sub(1) } else { (index + 1).min(count - 1) };
        Self::key(level, next)
    }
}

impl Topology for Radial1d {
    fn id(&self) -> &'static str {
        "veyra.topo.radial_1d/1"
    }

    fn level(&self, key: CellKey) -> Result<u8, SpatialError> {
        Self::decode(key).map(|decoded| decoded.0)
    }

    fn parent(&self, key: CellKey) -> Result<Option<CellKey>, SpatialError> {
        let (level, _) = Self::decode(key)?;
        Ok(if level == 0 { None } else { Some(CellKey(key.0 >> 1)) })
    }

    fn children(&self, key: CellKey) -> Result<Vec<CellKey>, SpatialError> {
        let (level, index) = Self::decode(key)?;
        if level == 30 {
            return Err(SpatialError::InvalidLevel);
        }
        Ok(vec![Self::key(level + 1, index * 2)?, Self::key(level + 1, index * 2 + 1)?])
    }

    fn tile_key(&self, key: CellKey, tile_log2: u8) -> Result<TileKey, SpatialError> {
        if tile_log2 > 30 {
            return Err(SpatialError::InvalidTileLog2);
        }
        let (level, index) = Self::decode(key)?;
        Ok(TileKey { level, address: Self::key(level, index >> tile_log2.min(level))? })
    }

    fn validate_tile_key(&self, tile: TileKey, tile_log2: u8) -> Result<(), SpatialError> {
        if tile_log2 > 30 {
            return Err(SpatialError::InvalidTileLog2);
        }
        if tile.level > 30 {
            return Err(SpatialError::InvalidLevel);
        }
        let (address_level, tile_index) = Self::decode(tile.address)?;
        let tile_count = 1_u64 << tile.level.saturating_sub(tile_log2);
        if address_level != tile.level || tile_index >= tile_count {
            return Err(SpatialError::InvalidTileKey);
        }
        Ok(())
    }

    fn cell_measure(&self, key: CellKey) -> Result<f64, SpatialError> {
        let (level, index) = Self::decode(key)?;
        let count = (1_u64 << level) as f64;
        let inner = index as f64 / count;
        let outer = (index + 1) as f64 / count;
        let cubic = |value: f64| value * value * value;
        let extent_cubed = self.extent_m * self.extent_m * self.extent_m;
        if !extent_cubed.is_finite() {
            return Err(SpatialError::InvalidExtent);
        }
        Ok(4.0 * core::f64::consts::PI / 3.0 * extent_cubed * (cubic(outer) - cubic(inner)))
    }

    fn neighbors(&self, key: CellKey) -> Result<Vec<CellKey>, SpatialError> {
        Ok(vec![Self::halo_neighbor(key, -1)?, Self::halo_neighbor(key, 1)?])
    }
}

fn project_direction(direction: Dir) -> (u8, f64, f64) {
    let magnitudes = [libm::fabs(direction.x), libm::fabs(direction.y), libm::fabs(direction.z)];
    let mut axis = 0;
    if magnitudes[1] > magnitudes[axis] {
        axis = 1;
    }
    if magnitudes[2] > magnitudes[axis] {
        axis = 2;
    }
    match axis {
        0 if direction.x >= 0.0 => (0, direction.y / direction.x, direction.z / direction.x),
        0 => (3, direction.z / direction.x, direction.y / direction.x),
        1 if direction.y >= 0.0 => (1, -direction.x / direction.y, direction.z / direction.y),
        1 => (4, direction.z / direction.y, -direction.x / direction.y),
        2 if direction.z >= 0.0 => (2, -direction.x / direction.z, -direction.y / direction.z),
        _ => (5, -direction.y / direction.z, -direction.x / direction.z),
    }
}

fn face_to_xyz(face: u8, u: f64, v: f64) -> Result<(f64, f64, f64), SpatialError> {
    match face {
        0 => Ok((1.0, u, v)),
        1 => Ok((-u, 1.0, v)),
        2 => Ok((-u, -v, 1.0)),
        3 => Ok((-1.0, -v, -u)),
        4 => Ok((v, -1.0, -u)),
        5 => Ok((v, u, -1.0)),
        _ => Err(SpatialError::InvalidFace),
    }
}

fn face_direction(face: u8, u: f64, v: f64) -> Result<Dir, SpatialError> {
    let (x, y, z) = face_to_xyz(face, u, v)?;
    Dir::new(x, y, z)
}

fn warp_uv(value: f64) -> f64 {
    if value >= 0.0 {
        0.5 * libm::sqrt(1.0 + 3.0 * value)
    } else {
        1.0 - 0.5 * libm::sqrt(1.0 - 3.0 * value)
    }
}

fn inverse_warp(value: f64) -> f64 {
    if value >= 0.5 {
        (4.0 * value * value - 1.0) / 3.0
    } else {
        (1.0 - 4.0 * (1.0 - value) * (1.0 - value)) / 3.0
    }
}

fn index_coordinate(value: f64, count: u64) -> u64 {
    let index = libm::floor(value * count as f64) as u64;
    index.min(count - 1)
}

fn edge_index(edge: FaceEdge) -> usize {
    match edge {
        FaceEdge::UMinus => 0,
        FaceEdge::UPlus => 1,
        FaceEdge::VMinus => 2,
        FaceEdge::VPlus => 3,
    }
}

/// Spatial key, direction, or frame validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpatialError {
    /// Direction components must be finite and nonzero.
    InvalidDirection,
    /// Quaternion is nonfinite or not unit length.
    InvalidQuaternion,
    /// Cell level is outside the V1 range.
    InvalidLevel,
    /// Cell key is malformed for its declared topology.
    InvalidCellKey,
    /// Face number is not in the V1 range.
    InvalidFace,
    /// Tile size exponent is outside the V1 range.
    InvalidTileLog2,
    /// Tile address is not the canonical tile ancestor for its level.
    InvalidTileKey,
    /// Radial extent is negative or nonfinite.
    InvalidExtent,
}

impl fmt::Display for SpatialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidDirection => "direction must be finite and nonzero",
            Self::InvalidQuaternion => "frame orientation must be a finite unit quaternion",
            Self::InvalidLevel => "topology level is outside the V1 range",
            Self::InvalidCellKey => "cell key is malformed for its topology",
            Self::InvalidFace => "face number is outside the V1 range",
            Self::InvalidTileLog2 => "tile_log2 is outside the V1 range",
            Self::InvalidTileKey => "tile key is not canonical for its topology",
            Self::InvalidExtent => "radial extent must be finite and nonnegative",
        })
    }
}

impl std::error::Error for SpatialError {}

#[cfg(test)]
mod tests {
    use super::{CellKey, Dir, DirCube, FACE_ADJACENCY, FaceEdge, Radial1d, Topology};

    #[test]
    fn dir_cube_key_parent_children_and_tiles_round_trip() {
        let topology = DirCube;
        for face in 0..6 {
            for i in 0..8 {
                for j in 0..8 {
                    let key = DirCube::key(face, i, j, 3).unwrap();
                    assert_eq!(DirCube::decode(key).unwrap(), (face, i, j, 3));
                    assert_eq!(topology.level(key).unwrap(), 3);
                    let parent = topology.parent(key).unwrap().unwrap();
                    assert!(topology.children(parent).unwrap().contains(&key));
                    let tile = topology.tile_key(key, 2).unwrap();
                    assert_eq!(DirCube::decode(tile.address).unwrap().3, 1);
                    assert_eq!(topology.level(topology.children(key).unwrap()[0]).unwrap(), 4);
                }
            }
        }
    }

    #[test]
    fn face_adjacency_is_involutive_including_flips() {
        let topology = DirCube;
        let edges = [FaceEdge::UMinus, FaceEdge::UPlus, FaceEdge::VMinus, FaceEdge::VPlus];
        for face in 0..6 {
            for edge in edges {
                let adjacency = FACE_ADJACENCY[usize::from(face)][match edge {
                    FaceEdge::UMinus => 0,
                    FaceEdge::UPlus => 1,
                    FaceEdge::VMinus => 2,
                    FaceEdge::VPlus => 3,
                }];
                let reverse = FACE_ADJACENCY[usize::from(adjacency.face)][match adjacency.edge {
                    FaceEdge::UMinus => 0,
                    FaceEdge::UPlus => 1,
                    FaceEdge::VMinus => 2,
                    FaceEdge::VPlus => 3,
                }];
                assert_eq!(reverse.face, face);
                assert_eq!(reverse.edge, edge);
                assert_eq!(reverse.flip, adjacency.flip);
                for along in 0..8 {
                    let key = match edge {
                        FaceEdge::UMinus => DirCube::key(face, 0, along, 3).unwrap(),
                        FaceEdge::UPlus => DirCube::key(face, 7, along, 3).unwrap(),
                        FaceEdge::VMinus => DirCube::key(face, along, 0, 3).unwrap(),
                        FaceEdge::VPlus => DirCube::key(face, along, 7, 3).unwrap(),
                    };
                    let across = topology.neighbor(key, edge).unwrap();
                    assert_eq!(topology.neighbor(across, adjacency.edge).unwrap(), key);
                }
            }
        }
    }

    #[test]
    fn cell_centres_relocate_to_the_same_cells() {
        for level in 0..=5 {
            let count = 1_u64 << level;
            for face in 0..6 {
                for i in 0..count {
                    for j in 0..count {
                        let key = DirCube::key(face, i, j, level).unwrap();
                        let center = DirCube.cell_center(key).unwrap();
                        assert_eq!(DirCube.locate(center, level).unwrap(), key);
                    }
                }
            }
        }
    }

    #[test]
    fn cube_cell_measures_partition_four_pi() {
        let topology = DirCube;
        for level in 0..=3 {
            let count = 1_u64 << level;
            let mut total = 0.0;
            for face in 0..6 {
                for i in 0..count {
                    for j in 0..count {
                        total += topology
                            .cell_measure(DirCube::key(face, i, j, level).unwrap())
                            .unwrap();
                    }
                }
            }
            assert!((total - 4.0 * core::f64::consts::PI).abs() < 1.0e-10);
        }
    }

    #[test]
    fn corner_query_exposes_two_edge_cells() {
        for face in 0..6 {
            for i in [0, 7] {
                for j in [0, 7] {
                    let key = DirCube::key(face, i, j, 3).unwrap();
                    let cells = DirCube.corner_stencil(key).unwrap().unwrap();
                    assert_ne!(cells[0], cells[1]);
                    assert_eq!(DirCube::decode(cells[0]).unwrap().3, 3);
                    assert_eq!(DirCube::decode(cells[1]).unwrap().3, 3);
                }
            }
        }
    }

    #[test]
    fn radial_heap_keys_parent_children_measure_and_clamped_halo() {
        let topology = Radial1d::default();
        for level in 0..=10 {
            let count = 1_u64 << level;
            for index in 0..count {
                let key = Radial1d::key(level, index).unwrap();
                assert_eq!(Radial1d::decode(key).unwrap(), (level, index));
                if level > 0 {
                    let parent = topology.parent(key).unwrap().unwrap();
                    assert_eq!(topology.level(parent).unwrap(), level - 1);
                }
                let children = topology.children(key).unwrap_or_default();
                if level < 10 {
                    assert_eq!(children.len(), 2);
                }
            }
        }
        assert_eq!(
            Radial1d::halo_neighbor(Radial1d::key(2, 0).unwrap(), -1).unwrap(),
            Radial1d::key(2, 0).unwrap()
        );
        assert_eq!(
            Radial1d::halo_neighbor(Radial1d::key(2, 3).unwrap(), 1).unwrap(),
            Radial1d::key(2, 3).unwrap()
        );
        let sum: f64 = (0..8)
            .map(|index| topology.cell_measure(Radial1d::key(3, index).unwrap()).unwrap())
            .sum();
        assert!((sum - 4.0 * core::f64::consts::PI / 3.0).abs() < 1.0e-12);
        let doubled = Radial1d::with_extent(2.0).unwrap();
        let whole_shell = doubled.cell_measure(Radial1d::key(0, 0).unwrap()).unwrap();
        assert!((whole_shell - 8.0 * 4.0 * core::f64::consts::PI / 3.0).abs() < 1.0e-12);
        assert!(Radial1d::with_extent(0.0).is_err());
    }

    #[test]
    fn axis_ties_prefer_x_then_y_then_z() {
        let cube = DirCube;
        assert_eq!(
            DirCube::decode(cube.locate(Dir::new(1.0, 1.0, 1.0).unwrap(), 0).unwrap()).unwrap().0,
            0
        );
        assert_eq!(
            DirCube::decode(cube.locate(Dir::new(0.0, -1.0, -1.0).unwrap(), 0).unwrap()).unwrap().0,
            4
        );
        assert_eq!(
            DirCube::decode(cube.locate(Dir::new(0.0, 0.0, -1.0).unwrap(), 0).unwrap()).unwrap().0,
            5
        );
    }

    #[test]
    fn direction_normalization_handles_extreme_finite_scales() {
        let large = Dir::new(1.0e308, 1.0e308, 0.0).unwrap();
        let small = Dir::new(1.0e-300, 0.0, 0.0).unwrap();
        assert!((large.x - large.y).abs() < 1.0e-15);
        assert_eq!((small.x, small.y, small.z), (1.0, 0.0, 0.0));
    }

    #[test]
    fn malformed_keys_are_refused() {
        assert!(DirCube::decode(CellKey(0)).is_err());
        assert!(DirCube::decode(CellKey(7_u64 << 61 | 1_u64 << 60)).is_err());
        assert!(DirCube::key(6, 0, 0, 1).is_err());
        assert!(DirCube::key(0, 2, 0, 1).is_err());
        assert!(Radial1d::decode(CellKey(0)).is_err());
    }

    #[test]
    fn deterministic_arbitrary_directions_relocate_to_the_same_cells() {
        let mut state = 0x6a09_e667_f3bc_c909_u64;
        for _ in 0..4096 {
            let mut component = || {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state as f64 / u64::MAX as f64) * 2.0 - 1.0
            };
            let direction = Dir::new(component(), component(), component()).unwrap();
            for level in [0, 1, 5, 12, 30] {
                let key = DirCube.locate(direction, level).unwrap();
                let center = DirCube.cell_center(key).unwrap();
                assert_eq!(DirCube.locate(center, level).unwrap(), key);
            }
        }
    }
}
