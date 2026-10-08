//! Generic frame math and the V1 spatial topology implementations.

use core::fmt;

const RADIAL_MAX_LEVEL: u8 = 30;

/// V1 right-handed body-fixed axis convention.
pub const BODY_FIXED_AXES: &str =
    "+Z is the positive rotation pole; +X is the prime meridian; right-handed";

/// Unit direction in a body-fixed frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dir {
    /// X component.
    x: f64,
    /// Y component.
    y: f64,
    /// Z component.
    z: f64,
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

    /// Returns the unit direction's X component.
    pub const fn x(self) -> f64 {
        self.x
    }

    /// Returns the unit direction's Y component.
    pub const fn y(self) -> f64 {
        self.y
    }

    /// Returns the unit direction's Z component.
    pub const fn z(self) -> f64 {
        self.z
    }

    fn validate(self) -> Result<(), SpatialError> {
        if !self.x.is_finite() || !self.y.is_finite() || !self.z.is_finite() {
            return Err(SpatialError::InvalidDirection);
        }
        let norm_squared = self.x * self.x + self.y * self.y + self.z * self.z;
        if !norm_squared.is_finite() || norm_squared == 0.0 || (norm_squared - 1.0).abs() > 1.0e-12
        {
            return Err(SpatialError::InvalidDirection);
        }
        Ok(())
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
    w: f64,
    /// X component.
    x: f64,
    /// Y component.
    y: f64,
    /// Z component.
    z: f64,
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

    /// Returns the quaternion's scalar component.
    pub const fn w(self) -> f64 {
        self.w
    }

    /// Returns the quaternion's X component.
    pub const fn x(self) -> f64 {
        self.x
    }

    /// Returns the quaternion's Y component.
    pub const fn y(self) -> f64 {
        self.y
    }

    /// Returns the quaternion's Z component.
    pub const fn z(self) -> f64 {
        self.z
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

/// Raster dimensions and local origin for a requested tile and halo.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TileRasterLayout {
    /// Width including horizontal halo.
    pub dim_i: u64,
    /// Height including vertical halo when the topology has a second axis.
    pub dim_j: u64,
    /// Signed tile-local coordinate of the first output column.
    pub offset_i: i64,
    /// Signed tile-local coordinate of the first output row.
    pub offset_j: i64,
}

/// A point in the coordinate chart accepted by a topology.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TopologyPoint {
    /// Unit direction on a direction-sphere chart.
    Direction(Dir),
    /// Normalized radial coordinate in the closed interval from zero to one.
    RadialFraction(f64),
}

/// V1 interpolation operator accepted by a topology stencil.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterpolationMode {
    /// Select the containing cell.
    Nearest,
    /// Cell-centered bilinear interpolation on a 2D chart.
    Bilinear,
    /// Linear interpolation between radial shell centers.
    Linear,
}

/// One cell and its nonnegative interpolation weight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeightedCell {
    /// Addressed topology cell.
    pub key: CellKey,
    /// Contribution weight; a complete stencil sums to one.
    pub weight: f64,
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
    /// Locates a topology-chart point at an exact level.
    fn locate_point(&self, point: TopologyPoint, level: u8) -> Result<CellKey, SpatialError>;
    /// Produces topology-owned interpolation weights at an exact level.
    fn interpolation_stencil(
        &self,
        point: TopologyPoint,
        level: u8,
        mode: InterpolationMode,
    ) -> Result<Vec<WeightedCell>, SpatialError>;
    /// Returns the topology-chart point at a cell center.
    fn point_for_cell(&self, key: CellKey) -> Result<TopologyPoint, SpatialError>;
    /// Returns the data layout and origin for a canonical tile key.
    fn tile_layout_for(&self, tile: TileKey, tile_log2: u8) -> Result<TileLayout, SpatialError>;
    /// Returns raster dimensions and halo-local origin for a tile.
    fn tile_raster_layout(
        &self,
        tile: TileKey,
        tile_log2: u8,
        halo: u8,
    ) -> Result<TileRasterLayout, SpatialError>;
    /// Returns a cell within a tile using tile-local coordinates.
    fn tile_cell(
        &self,
        tile: TileKey,
        tile_log2: u8,
        i: u64,
        j: u64,
    ) -> Result<CellKey, SpatialError>;
    /// Returns halo cells at a tile-local signed coordinate, applying topology boundary rules.
    fn tile_halo_cells(
        &self,
        tile: TileKey,
        tile_log2: u8,
        i: i64,
        j: i64,
    ) -> Result<Vec<WeightedCell>, SpatialError>;
    /// Returns the tile-local coordinate of a cell.
    fn tile_offset(&self, key: CellKey, tile_log2: u8) -> Result<(u64, u64), SpatialError>;
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
        direction.validate()?;
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

    /// Returns a body-fixed direction at Q0.32 coordinates within a cell.
    pub fn direction_at_local(self, key: CellKey, du: u32, dv: u32) -> Result<Dir, SpatialError> {
        let (face, i, j, level) = Self::decode(key)?;
        let count = (1_u64 << level) as f64;
        let unit = 4_294_967_296.0;
        let s = (i as f64 + f64::from(du) / unit) / count;
        let t = (j as f64 + f64::from(dv) / unit) / count;
        let (x, y, z) = face_to_xyz(face, inverse_warp(s), inverse_warp(t))?;
        Dir::new(x, y, z)
    }

    /// Returns a direction from continuous warped face coordinates in the closed unit chart.
    pub fn direction_at_face_st(self, face: u8, s: f64, t: f64) -> Result<Dir, SpatialError> {
        if face > 5
            || !s.is_finite()
            || !t.is_finite()
            || !(0.0..=1.0).contains(&s)
            || !(0.0..=1.0).contains(&t)
        {
            return Err(SpatialError::InvalidPosition);
        }
        let (x, y, z) = face_to_xyz(face, inverse_warp(s), inverse_warp(t))?;
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
        let count = 1_u64 << level;
        let edge_i = if i == 0 {
            Some(FaceEdge::UMinus)
        } else if i + 1 == count {
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
        self.corner_stencil_for_edges(key, edge_i, edge_j)
    }

    fn corner_stencil_for_edges(
        self,
        key: CellKey,
        edge_i: Option<FaceEdge>,
        edge_j: Option<FaceEdge>,
    ) -> Result<Option<[CellKey; 2]>, SpatialError> {
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

    fn direction_stencil_cells(
        self,
        face: u8,
        i: i64,
        j: i64,
        level: u8,
    ) -> Result<Vec<(CellKey, f64)>, SpatialError> {
        let count = 1_i64 << level;
        let out_i = i < 0 || i >= count;
        let out_j = j < 0 || j >= count;
        let clamped_i = i.clamp(0, count - 1) as u64;
        let clamped_j = j.clamp(0, count - 1) as u64;
        let boundary = Self::key(face, clamped_i, clamped_j, level)?;
        match (out_i, out_j) {
            (false, false) => Ok(vec![(boundary, 1.0)]),
            (true, false) => {
                let edge = if i < 0 { FaceEdge::UMinus } else { FaceEdge::UPlus };
                Ok(vec![(self.neighbor(boundary, edge)?, 1.0)])
            }
            (false, true) => {
                let edge = if j < 0 { FaceEdge::VMinus } else { FaceEdge::VPlus };
                Ok(vec![(self.neighbor(boundary, edge)?, 1.0)])
            }
            (true, true) => {
                let edge_i = if i < 0 { FaceEdge::UMinus } else { FaceEdge::UPlus };
                let edge_j = if j < 0 { FaceEdge::VMinus } else { FaceEdge::VPlus };
                let cells = self
                    .corner_stencil_for_edges(boundary, Some(edge_i), Some(edge_j))?
                    .ok_or(SpatialError::InvalidCellKey)?;
                Ok(vec![(cells[0], 0.5), (cells[1], 0.5)])
            }
        }
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

    fn locate_point(&self, point: TopologyPoint, level: u8) -> Result<CellKey, SpatialError> {
        match point {
            TopologyPoint::Direction(direction) => self.locate(direction, level),
            TopologyPoint::RadialFraction(_) => Err(SpatialError::InvalidPosition),
        }
    }

    fn interpolation_stencil(
        &self,
        point: TopologyPoint,
        level: u8,
        mode: InterpolationMode,
    ) -> Result<Vec<WeightedCell>, SpatialError> {
        if level > 30 {
            return Err(SpatialError::InvalidLevel);
        }
        let TopologyPoint::Direction(direction) = point else {
            return Err(SpatialError::InvalidPosition);
        };
        direction.validate()?;
        if mode == InterpolationMode::Nearest {
            return Ok(vec![WeightedCell { key: self.locate(direction, level)?, weight: 1.0 }]);
        }
        if mode != InterpolationMode::Bilinear {
            return Err(SpatialError::InvalidPosition);
        }
        let (face, u, v) = project_direction(direction);
        let count = (1_u64 << level) as f64;
        let grid_i = warp_uv(u) * count - 0.5;
        let grid_j = warp_uv(v) * count - 0.5;
        let i0 = libm::floor(grid_i) as i64;
        let j0 = libm::floor(grid_j) as i64;
        let tx = grid_i - i0 as f64;
        let ty = grid_j - j0 as f64;
        let mut weights = std::collections::BTreeMap::<CellKey, f64>::new();
        for (i, wi) in [(i0, 1.0 - tx), (i0 + 1, tx)] {
            for (j, wj) in [(j0, 1.0 - ty), (j0 + 1, ty)] {
                let weight = wi * wj;
                if weight == 0.0 {
                    continue;
                }
                for (key, share) in self.direction_stencil_cells(face, i, j, level)? {
                    *weights.entry(key).or_default() += weight * share;
                }
            }
        }
        Ok(weights.into_iter().map(|(key, weight)| WeightedCell { key, weight }).collect())
    }

    fn point_for_cell(&self, key: CellKey) -> Result<TopologyPoint, SpatialError> {
        self.cell_center(key).map(TopologyPoint::Direction)
    }

    fn tile_layout_for(&self, tile: TileKey, tile_log2: u8) -> Result<TileLayout, SpatialError> {
        self.validate_tile_key(tile, tile_log2)?;
        let (face, tile_i, tile_j, _) = Self::decode(tile.address)?;
        let edge = 1_u64 << tile.level.min(tile_log2);
        Ok(TileLayout { face, i_start: tile_i * edge, j_start: tile_j * edge, edge })
    }

    fn tile_raster_layout(
        &self,
        tile: TileKey,
        tile_log2: u8,
        halo: u8,
    ) -> Result<TileRasterLayout, SpatialError> {
        let layout = self.tile_layout_for(tile, tile_log2)?;
        let edge = i64::try_from(layout.edge).map_err(|_| SpatialError::InvalidTileKey)?;
        let halo = i64::from(halo);
        Ok(TileRasterLayout {
            dim_i: u64::try_from(edge + 2 * halo).map_err(|_| SpatialError::InvalidTileKey)?,
            dim_j: u64::try_from(edge + 2 * halo).map_err(|_| SpatialError::InvalidTileKey)?,
            offset_i: -halo,
            offset_j: -halo,
        })
    }

    fn tile_cell(
        &self,
        tile: TileKey,
        tile_log2: u8,
        i: u64,
        j: u64,
    ) -> Result<CellKey, SpatialError> {
        let layout = self.tile_layout_for(tile, tile_log2)?;
        if i >= layout.edge || j >= layout.edge {
            return Err(SpatialError::InvalidCellKey);
        }
        Self::key(layout.face, layout.i_start + i, layout.j_start + j, tile.level)
    }

    fn tile_halo_cells(
        &self,
        tile: TileKey,
        tile_log2: u8,
        i: i64,
        j: i64,
    ) -> Result<Vec<WeightedCell>, SpatialError> {
        let layout = self.tile_layout_for(tile, tile_log2)?;
        let count = 1_i64 << tile.level;
        let global_i = layout.i_start as i64 + i;
        let global_j = layout.j_start as i64 + j;
        let out_i = global_i < 0 || global_i >= count;
        let out_j = global_j < 0 || global_j >= count;
        let clamped_i = global_i.clamp(0, count - 1) as u64;
        let clamped_j = global_j.clamp(0, count - 1) as u64;
        let boundary = Self::key(layout.face, clamped_i, clamped_j, tile.level)?;
        match (out_i, out_j) {
            (false, false) => Ok(vec![WeightedCell {
                key: Self::key(layout.face, global_i as u64, global_j as u64, tile.level)?,
                weight: 1.0,
            }]),
            (true, false) => {
                let edge = if global_i < 0 { FaceEdge::UMinus } else { FaceEdge::UPlus };
                Ok(vec![WeightedCell { key: self.neighbor(boundary, edge)?, weight: 1.0 }])
            }
            (false, true) => {
                let edge = if global_j < 0 { FaceEdge::VMinus } else { FaceEdge::VPlus };
                Ok(vec![WeightedCell { key: self.neighbor(boundary, edge)?, weight: 1.0 }])
            }
            (true, true) => {
                let edge_i = if global_i < 0 { FaceEdge::UMinus } else { FaceEdge::UPlus };
                let edge_j = if global_j < 0 { FaceEdge::VMinus } else { FaceEdge::VPlus };
                let cells = self
                    .corner_stencil_for_edges(boundary, Some(edge_i), Some(edge_j))?
                    .ok_or(SpatialError::InvalidCellKey)?;
                Ok(vec![
                    WeightedCell { key: cells[0], weight: 0.5 },
                    WeightedCell { key: cells[1], weight: 0.5 },
                ])
            }
        }
    }

    fn tile_offset(&self, key: CellKey, tile_log2: u8) -> Result<(u64, u64), SpatialError> {
        let tile = self.tile_key(key, tile_log2)?;
        let layout = self.tile_layout_for(tile, tile_log2)?;
        let (face, i, j, level) = Self::decode(key)?;
        if face != layout.face || level != tile.level {
            return Err(SpatialError::InvalidTileKey);
        }
        Ok((i - layout.i_start, j - layout.j_start))
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
    /// Creates a radial topology with an extent that yields finite positive V1 shell measures.
    pub fn with_extent(extent_m: f64) -> Result<Self, SpatialError> {
        if !extent_m.is_finite() || extent_m <= 0.0 {
            return Err(SpatialError::InvalidExtent);
        }
        let topology = Self { extent_m };
        // The shell fraction is bounded by 1 at L0 and 2^-90 at the
        // innermost L30 shell. Check both bounds using the actual f64 measure path.
        topology.cell_measure(Self::key(0, 0)?)?;
        topology.cell_measure(Self::key(RADIAL_MAX_LEVEL, 0)?)?;
        Ok(topology)
    }

    /// Returns the physical domain extent in metres.
    pub const fn extent_m(self) -> f64 {
        self.extent_m
    }

    /// Creates a radial shell key from level and shell index.
    pub fn key(level: u8, index: u64) -> Result<CellKey, SpatialError> {
        if level > RADIAL_MAX_LEVEL || index >= (1_u64 << level) {
            return Err(SpatialError::InvalidCellKey);
        }
        Ok(CellKey((1_u64 << level) | index))
    }

    /// Decodes a radial shell key.
    pub fn decode(key: CellKey) -> Result<(u8, u64), SpatialError> {
        if key.0 == 0 || key.0 >= (1_u64 << (RADIAL_MAX_LEVEL + 1)) {
            return Err(SpatialError::InvalidCellKey);
        }
        let level = (63 - key.0.leading_zeros()) as u8;
        if level > RADIAL_MAX_LEVEL {
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
        if level == RADIAL_MAX_LEVEL {
            return Err(SpatialError::InvalidLevel);
        }
        Ok(vec![Self::key(level + 1, index * 2)?, Self::key(level + 1, index * 2 + 1)?])
    }

    fn tile_key(&self, key: CellKey, tile_log2: u8) -> Result<TileKey, SpatialError> {
        if tile_log2 > RADIAL_MAX_LEVEL {
            return Err(SpatialError::InvalidTileLog2);
        }
        let (level, index) = Self::decode(key)?;
        Ok(TileKey { level, address: Self::key(level, index >> tile_log2.min(level))? })
    }

    fn locate_point(&self, point: TopologyPoint, level: u8) -> Result<CellKey, SpatialError> {
        if level > RADIAL_MAX_LEVEL {
            return Err(SpatialError::InvalidLevel);
        }
        let TopologyPoint::RadialFraction(fraction) = point else {
            return Err(SpatialError::InvalidPosition);
        };
        if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
            return Err(SpatialError::InvalidPosition);
        }
        let count = 1_u64 << level;
        let index = libm::floor(fraction * count as f64) as u64;
        Self::key(level, index.min(count - 1))
    }

    fn interpolation_stencil(
        &self,
        point: TopologyPoint,
        level: u8,
        mode: InterpolationMode,
    ) -> Result<Vec<WeightedCell>, SpatialError> {
        if mode == InterpolationMode::Nearest {
            return Ok(vec![WeightedCell { key: self.locate_point(point, level)?, weight: 1.0 }]);
        }
        if mode != InterpolationMode::Linear || level > RADIAL_MAX_LEVEL {
            return Err(SpatialError::InvalidPosition);
        }
        let TopologyPoint::RadialFraction(fraction) = point else {
            return Err(SpatialError::InvalidPosition);
        };
        if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
            return Err(SpatialError::InvalidPosition);
        }
        let count = (1_u64 << level) as f64;
        let coordinate = fraction * count - 0.5;
        if coordinate <= 0.0 {
            return Ok(vec![WeightedCell { key: Self::key(level, 0)?, weight: 1.0 }]);
        }
        if coordinate >= count - 1.0 {
            return Ok(vec![WeightedCell {
                key: Self::key(level, (count as u64) - 1)?,
                weight: 1.0,
            }]);
        }
        let lower = libm::floor(coordinate) as u64;
        let blend = coordinate - lower as f64;
        Ok(vec![
            WeightedCell { key: Self::key(level, lower)?, weight: 1.0 - blend },
            WeightedCell { key: Self::key(level, lower + 1)?, weight: blend },
        ])
    }

    fn point_for_cell(&self, key: CellKey) -> Result<TopologyPoint, SpatialError> {
        let (level, index) = Self::decode(key)?;
        let count = (1_u64 << level) as f64;
        Ok(TopologyPoint::RadialFraction((index as f64 + 0.5) / count))
    }

    fn tile_layout_for(&self, tile: TileKey, tile_log2: u8) -> Result<TileLayout, SpatialError> {
        self.validate_tile_key(tile, tile_log2)?;
        let (_, tile_index) = Self::decode(tile.address)?;
        let edge = 1_u64 << tile.level.min(tile_log2);
        Ok(TileLayout { face: 0, i_start: tile_index * edge, j_start: 0, edge })
    }

    fn tile_raster_layout(
        &self,
        tile: TileKey,
        tile_log2: u8,
        halo: u8,
    ) -> Result<TileRasterLayout, SpatialError> {
        let layout = self.tile_layout_for(tile, tile_log2)?;
        let edge = i64::try_from(layout.edge).map_err(|_| SpatialError::InvalidTileKey)?;
        let halo = i64::from(halo);
        Ok(TileRasterLayout {
            dim_i: u64::try_from(edge + 2 * halo).map_err(|_| SpatialError::InvalidTileKey)?,
            dim_j: 1,
            offset_i: -halo,
            offset_j: 0,
        })
    }

    fn tile_cell(
        &self,
        tile: TileKey,
        tile_log2: u8,
        i: u64,
        j: u64,
    ) -> Result<CellKey, SpatialError> {
        let layout = self.tile_layout_for(tile, tile_log2)?;
        if i >= layout.edge || j != 0 {
            return Err(SpatialError::InvalidCellKey);
        }
        Self::key(tile.level, layout.i_start + i)
    }

    fn tile_halo_cells(
        &self,
        tile: TileKey,
        tile_log2: u8,
        i: i64,
        j: i64,
    ) -> Result<Vec<WeightedCell>, SpatialError> {
        if j != 0 {
            return Err(SpatialError::InvalidCellKey);
        }
        let layout = self.tile_layout_for(tile, tile_log2)?;
        let count = 1_i64 << tile.level;
        let index = (layout.i_start as i64 + i).clamp(0, count - 1) as u64;
        Ok(vec![WeightedCell { key: Self::key(tile.level, index)?, weight: 1.0 }])
    }

    fn tile_offset(&self, key: CellKey, tile_log2: u8) -> Result<(u64, u64), SpatialError> {
        let tile = self.tile_key(key, tile_log2)?;
        let layout = self.tile_layout_for(tile, tile_log2)?;
        let (level, index) = Self::decode(key)?;
        if level != tile.level {
            return Err(SpatialError::InvalidTileKey);
        }
        Ok((index - layout.i_start, 0))
    }

    fn validate_tile_key(&self, tile: TileKey, tile_log2: u8) -> Result<(), SpatialError> {
        if tile_log2 > RADIAL_MAX_LEVEL {
            return Err(SpatialError::InvalidTileLog2);
        }
        if tile.level > RADIAL_MAX_LEVEL {
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
        radial_shell_measure(self.extent_m, level, index)
    }

    fn neighbors(&self, key: CellKey) -> Result<Vec<CellKey>, SpatialError> {
        Ok(vec![Self::halo_neighbor(key, -1)?, Self::halo_neighbor(key, 1)?])
    }
}

fn radial_shell_measure(extent_m: f64, level: u8, index: u64) -> Result<f64, SpatialError> {
    let count = (1_u64 << level) as f64;
    let inner = index as f64 / count;
    let outer = (index + 1) as f64 / count;
    // Factor outer^3 - inner^3 to avoid cancellation for thin outer shells.
    let shell_fraction = (outer - inner) * (outer * outer + outer * inner + inner * inner);
    let extent_cubed = extent_m * extent_m * extent_m;
    if !extent_cubed.is_finite() {
        return Err(SpatialError::InvalidExtent);
    }
    let measure = 4.0 * core::f64::consts::PI / 3.0 * extent_cubed * shell_fraction;
    if !measure.is_finite() || measure <= 0.0 {
        return Err(SpatialError::InvalidExtent);
    }
    Ok(measure)
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
    /// Point does not belong to the topology coordinate chart or declared range.
    InvalidPosition,
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
    /// Radial extent is nonpositive, nonfinite, or yields unrepresentable V1 shell measures.
    InvalidExtent,
}

impl fmt::Display for SpatialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidDirection => "direction must be finite and nonzero",
            Self::InvalidPosition => "position is invalid for this topology",
            Self::InvalidQuaternion => "frame orientation must be a finite unit quaternion",
            Self::InvalidLevel => "topology level is outside the V1 range",
            Self::InvalidCellKey => "cell key is malformed for its topology",
            Self::InvalidFace => "face number is outside the V1 range",
            Self::InvalidTileLog2 => "tile_log2 is outside the V1 range",
            Self::InvalidTileKey => "tile key is not canonical for its topology",
            Self::InvalidExtent => {
                "radial extent must yield finite positive measures for all V1 shells"
            }
        })
    }
}

impl std::error::Error for SpatialError {}

#[cfg(test)]
mod tests {
    use super::{
        CellKey, Dir, DirCube, FACE_ADJACENCY, FaceEdge, InterpolationMode, RADIAL_MAX_LEVEL,
        Radial1d, SpatialError, Topology, TopologyPoint,
    };

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
    fn topology_interpolation_stencils_cover_all_cube_edges_and_corners() {
        let topology = DirCube;
        for face in 0..6 {
            for edge_position in [-1.0, 0.0, 1.0] {
                for (u, v) in [
                    (-1.0, edge_position),
                    (1.0, edge_position),
                    (edge_position, -1.0),
                    (edge_position, 1.0),
                ] {
                    let (x, y, z) = super::face_to_xyz(face, u, v).unwrap();
                    let point = TopologyPoint::Direction(Dir::new(x, y, z).unwrap());
                    let stencil = topology
                        .interpolation_stencil(point, 4, InterpolationMode::Bilinear)
                        .unwrap();
                    assert!(!stencil.is_empty());
                    assert!(stencil.iter().all(|cell| cell.weight >= 0.0 && cell.weight <= 1.0));
                    assert!(
                        (stencil.iter().map(|cell| cell.weight).sum::<f64>() - 1.0).abs() < 1.0e-12
                    );
                    for cell in stencil {
                        assert_eq!(topology.level(cell.key).unwrap(), 4);
                    }
                }
            }
        }
        for x in [-1.0, 1.0] {
            for y in [-1.0, 1.0] {
                for z in [-1.0, 1.0] {
                    let point = TopologyPoint::Direction(Dir::new(x, y, z).unwrap());
                    let stencil = topology
                        .interpolation_stencil(point, 5, InterpolationMode::Bilinear)
                        .unwrap();
                    assert!(
                        (stencil.iter().map(|cell| cell.weight).sum::<f64>() - 1.0).abs() < 1.0e-12
                    );
                    assert!(stencil.len() >= 2, "corner stencil applies the two-edge rule");
                }
            }
        }
    }

    #[test]
    fn topology_radial_linear_stencils_and_tile_offsets_are_canonical() {
        let radial = Radial1d::with_extent(8.0).unwrap();
        let point = TopologyPoint::RadialFraction(0.25);
        let stencil = radial.interpolation_stencil(point, 2, InterpolationMode::Linear).unwrap();
        assert_eq!(stencil.len(), 2);
        assert_eq!(stencil[0].weight, 0.5);
        assert_eq!(stencil[1].weight, 0.5);
        assert_eq!(
            radial
                .interpolation_stencil(
                    TopologyPoint::RadialFraction(0.0),
                    2,
                    InterpolationMode::Linear
                )
                .unwrap()
                .len(),
            1
        );

        for face in 0..6 {
            for i in 0..16 {
                for j in 0..16 {
                    let key = DirCube::key(face, i, j, 4).unwrap();
                    let cube_tile = DirCube.tile_key(key, 2).unwrap();
                    let layout = DirCube.tile_layout_for(cube_tile, 2).unwrap();
                    let (offset_i, offset_j) = DirCube.tile_offset(key, 2).unwrap();
                    assert!(offset_i < layout.edge && offset_j < layout.edge);
                    assert_eq!(DirCube.tile_cell(cube_tile, 2, offset_i, offset_j).unwrap(), key);
                }
            }
        }
        for i in 0..32 {
            let key = Radial1d::key(5, i).unwrap();
            let radial_tile = radial.tile_key(key, 2).unwrap();
            let layout = radial.tile_layout_for(radial_tile, 2).unwrap();
            let (offset, column) = radial.tile_offset(key, 2).unwrap();
            assert_eq!(column, 0);
            assert!(offset < layout.edge);
            assert_eq!(radial.tile_cell(radial_tile, 2, offset, 0).unwrap(), key);
        }
        let cube_tile = DirCube.tile_key(DirCube::key(0, 0, 0, 4).unwrap(), 2).unwrap();
        let cube_raster = DirCube.tile_raster_layout(cube_tile, 2, 1).unwrap();
        assert_eq!((cube_raster.dim_i, cube_raster.dim_j), (6, 6));
        assert_eq!((cube_raster.offset_i, cube_raster.offset_j), (-1, -1));
        let radial_tile = radial.tile_key(Radial1d::key(5, 0).unwrap(), 2).unwrap();
        let radial_raster = radial.tile_raster_layout(radial_tile, 2, 1).unwrap();
        assert_eq!((radial_raster.dim_i, radial_raster.dim_j), (6, 1));
        assert_eq!((radial_raster.offset_i, radial_raster.offset_j), (-1, 0));
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
    fn level_zero_bilinear_and_halo_corners_preserve_crossed_sides() {
        let topology = DirCube;
        for x in [-1.0, 1.0] {
            for y in [-1.0, 1.0] {
                for z in [-1.0, 1.0] {
                    let direction = Dir::new(x, y, z).unwrap();
                    let (face, u, v) = super::project_direction(direction);
                    let edge_i = if u < 0.0 { FaceEdge::UMinus } else { FaceEdge::UPlus };
                    let edge_j = if v < 0.0 { FaceEdge::VMinus } else { FaceEdge::VPlus };
                    let base = DirCube::key(face, 0, 0, 0).unwrap();
                    let stencil = topology
                        .interpolation_stencil(
                            TopologyPoint::Direction(direction),
                            0,
                            InterpolationMode::Bilinear,
                        )
                        .unwrap();
                    let weight = |key| {
                        stencil
                            .iter()
                            .find(|entry| entry.key == key)
                            .map_or(0.0, |entry| entry.weight)
                    };
                    assert!((weight(base) - 0.25).abs() < 1.0e-12);
                    assert!(
                        (weight(topology.neighbor(base, edge_i).unwrap()) - 0.375).abs() < 1.0e-12
                    );
                    assert!(
                        (weight(topology.neighbor(base, edge_j).unwrap()) - 0.375).abs() < 1.0e-12
                    );
                }
            }
        }

        for face in 0..6 {
            let base = DirCube::key(face, 0, 0, 0).unwrap();
            let tile = topology.tile_key(base, 0).unwrap();
            for (i, j, edge_i, edge_j) in [
                (-1, -1, FaceEdge::UMinus, FaceEdge::VMinus),
                (-1, 1, FaceEdge::UMinus, FaceEdge::VPlus),
                (1, -1, FaceEdge::UPlus, FaceEdge::VMinus),
                (1, 1, FaceEdge::UPlus, FaceEdge::VPlus),
            ] {
                let halo = topology.tile_halo_cells(tile, 0, i, j).unwrap();
                assert_eq!(halo.len(), 2);
                assert_eq!(halo[0].key, topology.neighbor(base, edge_i).unwrap());
                assert_eq!(halo[1].key, topology.neighbor(base, edge_j).unwrap());
                assert_eq!(halo[0].weight, 0.5);
                assert_eq!(halo[1].weight, 0.5);
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
        assert!(Radial1d::with_extent(1.0).is_ok());
    }

    #[test]
    fn radial_extent_rejects_nonrepresentable_whole_sphere_or_finest_shell() {
        let overflowing_extent: f64 = 4.0e102;
        let overflowing_cube = overflowing_extent * overflowing_extent * overflowing_extent;
        assert!(overflowing_cube.is_finite());
        assert!((4.0 * core::f64::consts::PI / 3.0 * overflowing_cube).is_infinite());
        assert!(matches!(
            Radial1d::with_extent(overflowing_extent),
            Err(SpatialError::InvalidExtent)
        ));

        let underflowing_extent: f64 = 1.0e-105;
        let underflowing_cube = underflowing_extent * underflowing_extent * underflowing_extent;
        let finest_shell_count = (1_u64 << RADIAL_MAX_LEVEL) as f64;
        let minimum_shell_fraction =
            1.0 / (finest_shell_count * finest_shell_count * finest_shell_count);
        assert_eq!(
            4.0 * core::f64::consts::PI / 3.0 * underflowing_cube * minimum_shell_fraction,
            0.0
        );
        assert!(matches!(
            Radial1d::with_extent(underflowing_extent),
            Err(SpatialError::InvalidExtent)
        ));

        for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(matches!(Radial1d::with_extent(invalid), Err(SpatialError::InvalidExtent)));
        }

        let invalid_internal = Radial1d { extent_m: overflowing_extent };
        assert!(matches!(
            invalid_internal.cell_measure(Radial1d::key(0, 0).unwrap()),
            Err(SpatialError::InvalidExtent)
        ));
    }

    #[test]
    fn accepted_radial_extents_have_finite_positive_measures_across_v1_levels() {
        for extent in [1.0e-80, 1.0, 2.0, 1.0e100] {
            let topology = Radial1d::with_extent(extent).unwrap();
            let whole = topology.cell_measure(Radial1d::key(0, 0).unwrap()).unwrap();
            assert!(whole.is_finite() && whole > 0.0, "extent {extent}");

            for level in [0, 1, 15, 29, RADIAL_MAX_LEVEL] {
                let count = 1_u64 << level;
                let mut indices = vec![0, count / 2, count - 1];
                if count > 1 {
                    indices.push(1);
                }
                if count > 2 {
                    indices.push(count - 2);
                }
                indices.sort_unstable();
                indices.dedup();
                for index in indices {
                    let key = Radial1d::key(level, index).unwrap();
                    let measure = topology.cell_measure(key).unwrap();
                    assert!(
                        measure.is_finite() && measure > 0.0,
                        "extent {extent}, level {level}, index {index}"
                    );
                }
            }

            let deepest_inner = Radial1d::key(RADIAL_MAX_LEVEL, 0).unwrap();
            let smallest_shell = topology.cell_measure(deepest_inner).unwrap();
            assert!(smallest_shell.is_finite() && smallest_shell > 0.0);
            let center = topology.center_radius(deepest_inner).unwrap();
            assert!(center.is_finite() && center > 0.0);
        }
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
    fn cube_location_rejects_invalid_directions_before_projection() {
        let invalid = [
            Dir { x: 0.0, y: 0.0, z: 0.0 },
            Dir { x: f64::NAN, y: 0.0, z: 0.0 },
            Dir { x: 0.0, y: f64::NAN, z: 0.0 },
            Dir { x: 0.0, y: 0.0, z: f64::NAN },
            Dir { x: f64::INFINITY, y: 0.0, z: 0.0 },
            Dir { x: f64::NEG_INFINITY, y: 0.0, z: 0.0 },
            Dir { x: 1.0, y: f64::INFINITY, z: -2.0 },
        ];
        for direction in invalid {
            for level in [0, 30] {
                assert_eq!(
                    DirCube.locate(direction, level),
                    Err(super::SpatialError::InvalidDirection),
                    "direction {direction:?} at level {level}"
                );
            }
        }

        // The fields are private to external callers. This internal mutation simulates
        // corrupted state and verifies locate still checks its trust boundary.
        let mut corrupted = Dir::new(1.0, 0.0, 0.0).unwrap();
        corrupted.y = f64::NAN;
        assert_eq!(DirCube.locate(corrupted, 0), Err(super::SpatialError::InvalidDirection));

        let mut non_unit = Dir::new(1.0, 0.0, 0.0).unwrap();
        non_unit.x = 2.0;
        assert_eq!(DirCube.locate(non_unit, 30), Err(super::SpatialError::InvalidDirection));
    }

    #[test]
    fn direction_and_quaternion_constructors_keep_invariant_fields_readable() {
        let direction = Dir::new(2.0, 0.0, 0.0).unwrap();
        assert_eq!((direction.x(), direction.y(), direction.z()), (1.0, 0.0, 0.0));
        assert!(Dir::new(0.0, 0.0, 0.0).is_err());
        assert!(Dir::new(f64::NAN, 0.0, 0.0).is_err());

        let orientation = super::Quaternion::new(1.0, 0.0, 0.0, 0.0).unwrap();
        assert_eq!(
            (orientation.w(), orientation.x(), orientation.y(), orientation.z()),
            (1.0, 0.0, 0.0, 0.0)
        );
        assert!(super::Quaternion::new(0.0, 0.0, 0.0, 0.0).is_err());
        assert!(super::Quaternion::new(f64::INFINITY, 0.0, 0.0, 0.0).is_err());
    }

    #[test]
    fn direction_normalization_handles_extreme_finite_scales() {
        let large = Dir::new(1.0e308, 1.0e308, 0.0).unwrap();
        let small = Dir::new(1.0e-300, 0.0, 0.0).unwrap();
        assert!((large.x() - large.y()).abs() < 1.0e-15);
        assert_eq!((small.x(), small.y(), small.z()), (1.0, 0.0, 0.0));
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
