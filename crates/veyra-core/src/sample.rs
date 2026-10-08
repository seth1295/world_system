//! Topology-driven field sampling over verified body content.

use core::fmt;

use serde_json::Value;

use crate::body::{FieldDescriptor, FieldId, parse_hash};
use crate::canon::blob::{BlobKind, CanonicalBlob, DType};
use crate::canon::hash;
use crate::canon::index::{IndexEntry, IndexValue};
use crate::ids::Hash32;
use crate::io::{Body, Need};
use crate::spatial::{
    CellKey, Dir, DirCube, InterpolationMode, LocalPos, Pos30, Radial1d, SpatialError, TileKey,
    Topology, TopologyPoint,
};
use crate::time::DecimalString;

/// Position accepted by the V1 body sampler.
#[derive(Clone, Debug, PartialEq)]
pub enum Position {
    /// Unit direction in a body-fixed frame.
    Direction(Dir),
    /// Axial display latitude and longitude in radians.
    AxialLatLon { lat_rad: f64, lon_rad: f64 },
    /// Addressed cell center. The domain name must match the field domain.
    Cell { domain: String, key: CellKey },
    /// Body-centered radial distance in metres.
    Radial { r_m: f64 },
    /// Canonical level-thirty direction chart coordinate.
    Pos30(Pos30),
    /// Cell-local position using Q0.32 chart coordinates.
    Local(LocalPos),
}

/// Requested sample resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LevelSel {
    /// The field's declared native level.
    Native,
    /// A specific supported level.
    Exact(u8),
    /// The declared canonical resolution when available, otherwise native.
    Canonical,
}

/// Requested temporal slice or reduction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimeSel {
    /// Static field value or the temporal descriptor's default reduction.
    Static,
    /// One periodic slice.
    Slice(u16),
    /// Arithmetic mean of available periodic slices.
    Mean,
    /// Minimum of available periodic slices.
    Min,
    /// Maximum of available periodic slices.
    Max,
    /// Normalized phase; periodic values wrap at whole cycles.
    Phase(f64),
}

/// One request for a field value.
#[derive(Clone, Debug, PartialEq)]
pub struct SampleQuery {
    /// Canonical field identity.
    pub field: FieldId,
    /// Requested point in the field's domain.
    pub pos: Position,
    /// Requested spatial resolution.
    pub level: LevelSel,
    /// Requested time selection.
    pub time: TimeSel,
}

/// Primitive stored value before scale and offset are applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RawValue {
    /// Integer storage value.
    Integer(i64),
    /// IEEE-754 binary32 storage value.
    Float(f32),
}

impl RawValue {
    /// Returns the numeric raw value as binary64.
    pub fn as_f64(self) -> f64 {
        match self {
            Self::Integer(value) => value as f64,
            Self::Float(value) => f64::from(value),
        }
    }
}

/// Source class used to produce a sample.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SampleSource {
    /// Directly read from a stored tile.
    Stored,
    /// Read from a stored below-native pyramid level.
    Pyramid,
    /// Read from a constant index entry.
    Const,
    /// Value inherited from the native cell.
    Inherited,
    /// No contributing non-nodata values were present.
    Nodata,
    /// A periodic-time reduction produced the value.
    TimeReduced,
    /// A descriptor-declared or topology-derived view produced the value.
    Derived,
}

/// Result of a canonical field query.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    /// Decoded physical value, absent when every contributing value is nodata.
    pub value: Option<f64>,
    /// Raw storage value when the query selected exactly one stored value.
    pub raw: Option<RawValue>,
    /// Category or feature ordinal for discrete semantics.
    pub category: Option<i64>,
    /// Resolution actually used after inheritance or smoothing rules.
    pub level_used: u8,
    /// Stored or derived source classification.
    pub source: SampleSource,
    /// Addressed cell used as the result's primary location.
    pub cell: CellKey,
}

/// A field tile and its optional assembled halo.
#[derive(Clone, Debug, PartialEq)]
pub struct TileData {
    /// Requested canonical tile key.
    pub key: TileKey,
    /// Raster width including halo columns.
    pub dim_i: u16,
    /// Raster height including halo rows.
    pub dim_j: u16,
    /// Number of periodic slices in the tile.
    pub slices: u16,
    /// Decoded values in `[slice][j][i]` order; `None` marks nodata.
    pub values: Vec<Option<f64>>,
    /// Spatial and temporal source class.
    pub source: SampleSource,
}

/// Requested tile output interpretation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TileView {
    /// Return decoded stored field values.
    Raw,
    /// Apply the descriptor's temporal reduction before returning one slice.
    TimeReduce,
    /// A declared capability-derived or topology-derived view ID.
    Derived(String),
}

/// One tile request with an optional one-cell topology halo.
#[derive(Clone, Debug, PartialEq)]
pub struct TileRequest {
    /// Canonical field identity.
    pub field: FieldId,
    /// Canonical tile address and field level.
    pub key: TileKey,
    /// Requested temporal slice or reduction.
    pub time: TimeSel,
    /// Number of assembled halo cells; V1 currently permits zero or one.
    pub halo: u8,
    /// Raw or time-reduced output view.
    pub view: TileView,
}

/// Measure-weighted summary of values in one field view.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldStats {
    /// Cells visited at the requested level.
    pub cells: u64,
    /// Cells whose values were nodata.
    pub nodata_cells: u64,
    /// Sum of valid cell measures.
    pub valid_measure: f64,
    /// Minimum decoded value.
    pub minimum: Option<f64>,
    /// Maximum decoded value.
    pub maximum: Option<f64>,
    /// Measure-weighted mean decoded value.
    pub mean: Option<f64>,
}

/// One measure-weighted histogram bin.
#[derive(Clone, Debug, PartialEq)]
pub struct HistogramBin {
    /// Inclusive lower edge.
    pub lower: f64,
    /// Exclusive upper edge, except for the final bin which includes its upper edge.
    pub upper: f64,
    /// Sum of cell measures in the bin.
    pub weight: f64,
    /// Number of contributing cells in the bin.
    pub cells: u64,
}

/// Measure-weighted histogram of a field view.
#[derive(Clone, Debug, PartialEq)]
pub struct Histogram {
    /// Minimum observed value.
    pub minimum: Option<f64>,
    /// Maximum observed value.
    pub maximum: Option<f64>,
    /// Equal-width bins.
    pub bins: Vec<HistogramBin>,
}

/// One field's actual value at an inspection point.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldInspection {
    /// Canonical field identity.
    pub field: FieldId,
    /// Stable field name.
    pub name: String,
    /// Declared field semantic.
    pub semantic: String,
    /// Optional physical unit.
    pub unit: Option<String>,
    /// Sampled value and provenance when the reader can interpret the descriptor.
    pub sample: Option<Sample>,
    /// Compatibility or selection issue that prevented a value sample.
    pub issue: Option<String>,
}

/// Field report for one requested position.
#[derive(Clone, Debug, PartialEq)]
pub struct PointReport {
    /// Requested body position.
    pub position: Position,
    /// Actual field samples available at the position.
    pub fields: Vec<FieldInspection>,
}

/// Failure while resolving a canonical sample request.
#[derive(Clone, Debug, PartialEq)]
pub enum SampleError {
    /// Field ID is absent from the body registry.
    UnknownField,
    /// The field uses ancillary semantics that this reader cannot evaluate.
    UnsupportedField,
    /// Requested level or declared policy is not supported by this V1 core.
    UnsupportedResolution,
    /// Normative refinement is required but belongs to a later implementation stage.
    UnsupportedRefinement,
    /// Requested interpolation or time selection is not valid for this descriptor.
    UnsupportedSelection,
    /// The body does not declare the field's domain.
    UnknownDomain,
    /// A query position does not belong to the field domain.
    InvalidPosition,
    /// An indexed tile is absent from the field index.
    MissingTile,
    /// Content is unavailable and must be supplied through the sans-IO contract.
    Missing(Vec<Need>),
    /// Raster content is malformed or inconsistent with its descriptor.
    InvalidRaster,
    /// Canonical blob identity does not match the requested content ID.
    HashMismatch(Hash32),
    /// Numeric metadata cannot be interpreted as a finite scale or offset.
    InvalidScale,
    /// Core spatial operation rejected a coordinate or key.
    Spatial(SpatialError),
}

impl fmt::Display for SampleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownField => formatter.write_str("field is not registered"),
            Self::UnsupportedField => formatter.write_str("field semantics are not supported"),
            Self::UnsupportedResolution => {
                formatter.write_str("requested resolution is unsupported")
            }
            Self::UnsupportedRefinement => {
                formatter.write_str("normative refinement is not implemented")
            }
            Self::UnsupportedSelection => formatter.write_str("sample selection is unsupported"),
            Self::UnknownDomain => formatter.write_str("field domain is not declared"),
            Self::InvalidPosition => {
                formatter.write_str("position is invalid for the field domain")
            }
            Self::MissingTile => {
                formatter.write_str("field index has no tile for the addressed cell")
            }
            Self::Missing(needs) => {
                write!(formatter, "sample requires {} more resource(s)", needs.len())
            }
            Self::InvalidRaster => formatter.write_str("raster tile is invalid for the field"),
            Self::HashMismatch(hash) => {
                write!(formatter, "sample content hash mismatch for {hash}")
            }
            Self::InvalidScale => formatter.write_str("field scale or offset is invalid"),
            Self::Spatial(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SampleError {}

impl From<SpatialError> for SampleError {
    fn from(value: SpatialError) -> Self {
        Self::Spatial(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Reduction {
    Single,
    Mean,
    Min,
    Max,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StoredSource {
    Blob,
    Const,
}

enum TopologyInstance {
    DirCube(DirCube),
    Radial(Radial1d),
}

struct DerivedTileQuery<'a> {
    view: &'a crate::views::ViewDescriptor,
    field: &'a FieldDescriptor,
    domain: &'a crate::body::Domain,
    topology: &'a dyn Topology,
    cell: CellKey,
    level: u8,
    time: TimeSel,
}

impl TopologyInstance {
    fn as_topology(&self) -> &dyn Topology {
        match self {
            Self::DirCube(topology) => topology,
            Self::Radial(topology) => topology,
        }
    }
}

impl Body {
    /// Returns the topology-independent requests needed to evaluate a field query.
    pub fn plan(&self, query: &SampleQuery) -> Result<Vec<Need>, SampleError> {
        let field = self.field(query.field)?;
        ensure_supported_field(field)?;
        let domain = self.domain(&field.domain)?;
        let topology = topology_for(domain)?;
        let (level, _, mode) = self.resolve_level(field, domain, query.level)?;
        let point = self.topology_point(&query.pos, &field.domain, topology.as_topology())?;
        let stencil = topology.as_topology().interpolation_stencil(point, level, mode)?;
        let (slices, _) = select_slices(field, query.time)?;
        let index = if let Some(index) = self.indexes.get(&field.id) {
            index
        } else {
            let Some(hash_text) = self.root.indexes.get(&field.id.to_string()) else {
                return Err(SampleError::MissingTile);
            };
            let expected = parse_hash(hash_text).map_err(|_| SampleError::InvalidRaster)?;
            return Ok(vec![Need::Index {
                field_id: field.id,
                path: format!("index/{}.idx", field.id),
                hash: expected,
            }]);
        };
        let mut needs = std::collections::BTreeSet::new();
        for cell in stencil {
            let tile = topology.as_topology().tile_key(cell.key, index.tile_log2)?;
            let entry =
                find_entry(index.entries.as_slice(), tile).ok_or(SampleError::MissingTile)?;
            if let IndexValue::Blob(hash) = entry.value
                && !self.blobs.contains_key(&hash)
            {
                needs.insert(Need::Blob { hash });
            }
        }
        let _ = slices;
        Ok(needs.into_iter().collect())
    }

    /// Samples a field using the descriptor's topology, storage, time, and compatibility rules.
    pub fn sample(&self, query: &SampleQuery) -> Result<Sample, SampleError> {
        let field = self.field(query.field)?;
        ensure_supported_field(field)?;
        let domain = self.domain(&field.domain)?;
        let topology = topology_for(domain)?;
        let (level, source_override, mode) = self.resolve_level(field, domain, query.level)?;
        let point = self.topology_point(&query.pos, &field.domain, topology.as_topology())?;
        let stencil = topology.as_topology().interpolation_stencil(point, level, mode)?;
        let needs = self.plan(query)?;
        if !needs.is_empty() {
            return Err(SampleError::Missing(needs));
        }
        let index = self.indexes.get(&field.id).ok_or(SampleError::MissingTile)?;
        let (slices, reduction) = select_slices(field, query.time)?;
        let mut values = Vec::new();
        let mut all_const = true;
        let mut primary_raw = None;
        let mut primary_cell = stencil.first().ok_or(SampleError::MissingTile)?.key;
        for slice in slices {
            let mut weighted_value = 0.0;
            let mut total_weight = 0.0;
            for (cell_index, weighted_cell) in stencil.iter().enumerate() {
                let (raw, source) =
                    self.raw_at(field, index, topology.as_topology(), weighted_cell.key, slice)?;
                all_const &= source == StoredSource::Const;
                if cell_index == 0 && primary_raw.is_none() {
                    primary_raw = Some(raw);
                    primary_cell = weighted_cell.key;
                }
                if is_nodata(field, raw) {
                    continue;
                }
                let physical = decode_physical(field, raw)?;
                weighted_value += physical * weighted_cell.weight;
                total_weight += weighted_cell.weight;
            }
            if total_weight > 0.0 {
                let result = weighted_value / total_weight;
                if !result.is_finite() {
                    return Err(SampleError::InvalidScale);
                }
                values.push(Some(result));
            } else {
                values.push(None);
            }
        }
        let valid: Vec<f64> = values.iter().filter_map(|value| *value).collect();
        let value = match reduction {
            Reduction::Single => valid.first().copied(),
            Reduction::Mean if !valid.is_empty() => Some(temporal_mean(&valid)?),
            Reduction::Min => valid.iter().copied().reduce(f64::min),
            Reduction::Max => valid.iter().copied().reduce(f64::max),
            _ => None,
        };
        let source = if value.is_none() {
            SampleSource::Nodata
        } else if reduction != Reduction::Single {
            SampleSource::TimeReduced
        } else if source_override == Some(SampleSource::Inherited) {
            SampleSource::Inherited
        } else if all_const {
            SampleSource::Const
        } else if let Some(source) = source_override {
            source
        } else if query_level_is_pyramid(field, query.level, level) {
            SampleSource::Pyramid
        } else {
            SampleSource::Stored
        };
        let category = if field.semantic == "category" || field.semantic == "feature_ref" {
            primary_raw.and_then(raw_to_i64)
        } else {
            None
        };
        let raw =
            if stencil.len() == 1 && reduction == Reduction::Single { primary_raw } else { None };
        Ok(Sample { value, raw, category, level_used: level, source, cell: primary_cell })
    }

    /// Samples every field whose domain accepts the supplied position.
    pub fn inspect(
        &self,
        position: &Position,
        level: LevelSel,
        time: TimeSel,
    ) -> Result<PointReport, SampleError> {
        let selected_domain = match position {
            Position::Cell { domain, .. } => Some(domain.as_str()),
            _ => None,
        };
        let mut fields = Vec::new();
        for field in &self.registry.fields {
            if selected_domain.is_some_and(|domain| domain != field.domain) {
                continue;
            }
            match self.sample(&SampleQuery { field: field.id, pos: position.clone(), level, time })
            {
                Ok(sample) => fields.push(FieldInspection {
                    field: field.id,
                    name: field.name.clone(),
                    semantic: field.semantic.clone(),
                    unit: field.extra.get("unit").and_then(Value::as_str).map(str::to_owned),
                    sample: Some(sample),
                    issue: None,
                }),
                Err(SampleError::InvalidPosition) => continue,
                Err(
                    error @ (SampleError::UnsupportedField
                    | SampleError::UnsupportedSelection
                    | SampleError::UnsupportedResolution
                    | SampleError::UnsupportedRefinement),
                ) => fields.push(FieldInspection {
                    field: field.id,
                    name: field.name.clone(),
                    semantic: field.semantic.clone(),
                    unit: field.extra.get("unit").and_then(Value::as_str).map(str::to_owned),
                    sample: None,
                    issue: Some(error.to_string()),
                }),
                Err(error) => return Err(error),
            }
        }
        Ok(PointReport { position: position.clone(), fields })
    }

    /// Supplies canonical uncompressed content required by an indexed field.
    pub fn provide_blob(&mut self, expected: Hash32, canonical: &[u8]) -> Result<(), SampleError> {
        if hash::hash(canonical) != expected {
            return Err(SampleError::HashMismatch(expected));
        }
        let blob = CanonicalBlob::decode(canonical).map_err(|_| SampleError::InvalidRaster)?;
        if blob.kind != BlobKind::RasterTile {
            return Err(SampleError::InvalidRaster);
        }
        let mut references = self.indexes.values().flat_map(|index| {
            index.entries.iter().filter_map(move |entry| {
                (entry.value == IndexValue::Blob(expected)).then_some((index, entry))
            })
        });
        let Some((index, first_entry)) = references.next() else {
            return Err(SampleError::MissingTile);
        };
        for (index, entry) in std::iter::once((index, first_entry)).chain(references) {
            let field_id = FieldId(index.field_id);
            let field = self.field(field_id)?;
            let domain = self.domain(&field.domain)?;
            if crate::body::field_dtype(field) != Some(blob.dtype) {
                return Err(SampleError::InvalidRaster);
            }
            let edge = 1_u64 << entry.level.min(index.tile_log2);
            let expected_dimensions = if domain.topology == "veyra.topo.radial_1d/1" {
                (edge, 1)
            } else if domain.topology == "veyra.topo.dir_cube/1" {
                (edge, edge)
            } else {
                return Err(SampleError::InvalidRaster);
            };
            if u64::from(blob.dim_i) != expected_dimensions.0
                || u64::from(blob.dim_j) != expected_dimensions.1
                || crate::body::temporal_slice_count(field)
                    .is_some_and(|count| count != blob.slices)
            {
                return Err(SampleError::InvalidRaster);
            }
        }
        self.blobs.insert(expected, canonical.to_vec());
        Ok(())
    }

    /// Returns a topology-aware tile, assembling its halo through the topology contract.
    pub fn tile(&self, request: &TileRequest) -> Result<TileData, SampleError> {
        if request.halo > 1 {
            return Err(SampleError::UnsupportedSelection);
        }
        let field = self.field(request.field)?;
        let domain = self.domain(&field.domain)?;
        let topology = topology_for(domain)?;
        let topology = topology.as_topology();
        let derived_view = match &request.view {
            TileView::Derived(id) => {
                let descriptor = self
                    .views()
                    .into_iter()
                    .find(|view| view.id == *id && view.domain == field.domain)
                    .ok_or(SampleError::UnsupportedSelection)?;
                if descriptor.display.as_ref().and_then(|display| display.get("needs")).is_some_and(
                    |needs| {
                        !needs.as_array().is_some_and(|needs| {
                            needs.iter().any(|name| name.as_str() == Some(&field.name))
                        })
                    },
                ) {
                    return Err(SampleError::UnsupportedSelection);
                }
                Some(descriptor)
            }
            _ => None,
        };
        let topology_view = matches!(
            &request.view,
            TileView::Derived(id) if id.starts_with("topology.")
        );
        if !topology_view {
            ensure_supported_field(field)?;
        }
        let periodic =
            field.temporal.get("kind").and_then(Value::as_str) == Some("periodic_slices");
        match (&request.view, periodic, request.time) {
            (TileView::Raw, true, TimeSel::Slice(_) | TimeSel::Phase(_))
            | (
                TileView::Raw,
                false,
                TimeSel::Static | TimeSel::Mean | TimeSel::Min | TimeSel::Max,
            )
            | (TileView::Derived(_), _, _)
            | (
                TileView::TimeReduce,
                true,
                TimeSel::Static | TimeSel::Mean | TimeSel::Min | TimeSel::Max,
            ) => {}
            _ => return Err(SampleError::UnsupportedSelection),
        }
        topology.validate_tile_key(request.key, domain.tile_log2)?;
        if request.key.level > domain.max_level {
            return Err(SampleError::UnsupportedResolution);
        }
        let layout = topology.tile_raster_layout(request.key, domain.tile_log2, request.halo)?;
        let dim_i = layout.dim_i;
        let dim_j = layout.dim_j;
        let dim_i = u16::try_from(dim_i).map_err(|_| SampleError::InvalidRaster)?;
        let dim_j = u16::try_from(dim_j).map_err(|_| SampleError::InvalidRaster)?;
        let mut values = Vec::with_capacity(usize::from(dim_i) * usize::from(dim_j));
        let mut source = None;
        for j in 0..i64::from(dim_j) {
            for i in 0..i64::from(dim_i) {
                let local_i = i + layout.offset_i;
                let local_j = j + layout.offset_j;
                let cells =
                    topology.tile_halo_cells(request.key, domain.tile_log2, local_i, local_j)?;
                let mut sum = 0.0;
                let mut weight = 0.0;
                for cell in cells {
                    let (sampled_value, sampled_source) = if let Some(view) = derived_view.as_ref()
                    {
                        (
                            self.derived_value(DerivedTileQuery {
                                view,
                                field,
                                domain,
                                topology,
                                cell: cell.key,
                                level: request.key.level,
                                time: request.time,
                            })?,
                            SampleSource::Derived,
                        )
                    } else {
                        let sample = self.sample(&SampleQuery {
                            field: request.field,
                            pos: Position::Cell { domain: field.domain.clone(), key: cell.key },
                            level: LevelSel::Exact(request.key.level),
                            time: request.time,
                        })?;
                        (sample.value, sample.source)
                    };
                    if let Some(value) = sampled_value {
                        sum += value * cell.weight;
                        weight += cell.weight;
                    }
                    source = Some(match (source, sampled_source) {
                        (_, SampleSource::TimeReduced) => SampleSource::TimeReduced,
                        (None, value) => value,
                        (Some(SampleSource::Nodata), value) => value,
                        (Some(value), _) => value,
                    });
                }
                values.push((weight > 0.0).then_some(sum / weight));
            }
        }
        let final_source = if values.iter().all(Option::is_none) {
            SampleSource::Nodata
        } else if derived_view.is_some() {
            SampleSource::Derived
        } else {
            source.unwrap_or(SampleSource::Nodata)
        };
        Ok(TileData { key: request.key, dim_i, dim_j, slices: 1, values, source: final_source })
    }

    fn derived_value(&self, query: DerivedTileQuery<'_>) -> Result<Option<f64>, SampleError> {
        let DerivedTileQuery { view, field, domain, topology, cell, level, time } = query;
        match view.operator.as_deref().ok_or(SampleError::UnsupportedSelection)? {
            "topology.cube_face" => {
                let (face, _, _, _) = DirCube::decode(cell)?;
                Ok(Some(f64::from(face)))
            }
            "topology.tile_level" => Ok(Some(f64::from(level))),
            "topology.axial_latitude" => {
                let TopologyPoint::Direction(direction) = topology.point_for_cell(cell)? else {
                    return Err(SampleError::UnsupportedSelection);
                };
                let (latitude, _) = direction.axial_lat_lon();
                Ok(Some(
                    libm::floor(
                        (latitude + core::f64::consts::FRAC_PI_2) / (core::f64::consts::PI / 12.0),
                    )
                    .clamp(0.0, 11.0),
                ))
            }
            "topology.radial_profile" => match topology.point_for_cell(cell)? {
                TopologyPoint::RadialFraction(fraction) => Ok(Some(fraction)),
                TopologyPoint::Direction(_) => Err(SampleError::UnsupportedSelection),
            },
            "core.slope/1" => self.slope_at(field, domain, topology, cell, level, time),
            "core.threshold_partition/1" => {
                self.threshold_partition_at(view, field, domain, cell, level, time)
            }
            _ => Err(SampleError::UnsupportedSelection),
        }
    }

    fn slope_at(
        &self,
        field: &FieldDescriptor,
        domain: &crate::body::Domain,
        topology: &dyn Topology,
        cell: CellKey,
        level: u8,
        time: TimeSel,
    ) -> Result<Option<f64>, SampleError> {
        if domain.topology != "veyra.topo.dir_cube/1" {
            return Err(SampleError::UnsupportedSelection);
        }
        let center = self.sample(&SampleQuery {
            field: field.id,
            pos: Position::Cell { domain: field.domain.clone(), key: cell },
            level: LevelSel::Exact(level),
            time,
        })?;
        let Some(_center_value) = center.value else { return Ok(None) };
        let center_direction = DirCube.cell_center(cell)?;
        let center_radius = self.figure_radius_at(center_direction, domain, level)?;
        let neighbors = topology.neighbors(cell)?;
        if neighbors.len() != 4 {
            return Err(SampleError::UnsupportedSelection);
        }
        let mut values = Vec::with_capacity(4);
        let mut distances = Vec::with_capacity(4);
        for neighbor in neighbors {
            let sample = self.sample(&SampleQuery {
                field: field.id,
                pos: Position::Cell { domain: field.domain.clone(), key: neighbor },
                level: LevelSel::Exact(level),
                time,
            })?;
            let Some(value) = sample.value else { return Ok(None) };
            let neighbor_direction = DirCube.cell_center(neighbor)?;
            let neighbor_radius = self.figure_radius_at(neighbor_direction, domain, level)?;
            let angle = angular_distance(center_direction, neighbor_direction)?;
            let mean_radius = center_radius * 0.5 + neighbor_radius * 0.5;
            let distance = angle * mean_radius;
            if !distance.is_finite() || distance <= 0.0 {
                return Err(SampleError::InvalidScale);
            }
            values.push(value);
            distances.push(distance);
        }
        Ok(Some(slope_from_cardinal_samples(&values, &distances)?))
    }

    fn threshold_partition_at(
        &self,
        view: &crate::views::ViewDescriptor,
        field: &FieldDescriptor,
        domain: &crate::body::Domain,
        cell: CellKey,
        level: u8,
        time: TimeSel,
    ) -> Result<Option<f64>, SampleError> {
        if field.extra.get("unit").and_then(Value::as_str) != Some("m") {
            return Err(SampleError::UnsupportedField);
        }
        let capability = view.capability.as_deref().ok_or(SampleError::UnsupportedSelection)?;
        let threshold_surface = self
            .capability_reference(capability, "reference_surface")
            .ok_or(SampleError::UnsupportedSelection)?;
        let field_surface = field
            .extra
            .get("reference")
            .and_then(|value| value.get("surface"))
            .and_then(Value::as_str)
            .or_else(|| self.capability_reference(&field.capability, "reference_surface"))
            .ok_or(SampleError::UnsupportedSelection)?;
        let TopologyPoint::Direction(direction) = DirCube.point_for_cell(cell)? else {
            return Err(SampleError::UnsupportedSelection);
        };
        let base_radius =
            self.reference_surface_radius(field_surface, direction, domain, level, 0)?;
        let threshold_radius =
            self.reference_surface_radius(threshold_surface, direction, domain, level, 0)?;
        let sample = self.sample(&SampleQuery {
            field: field.id,
            pos: Position::Cell { domain: field.domain.clone(), key: cell },
            level: LevelSel::Exact(level),
            time,
        })?;
        let Some(height) = sample.value else { return Ok(None) };
        let absolute_radius = base_radius + height;
        if !absolute_radius.is_finite() || !threshold_radius.is_finite() {
            return Err(SampleError::InvalidScale);
        }
        Ok(Some(if absolute_radius >= threshold_radius { 1.0 } else { 0.0 }))
    }

    fn capability_reference<'a>(&'a self, capability_id: &str, target: &str) -> Option<&'a str> {
        let capability = self.capabilities().iter().find(|item| item.id == capability_id)?;
        let definitions = crate::capability::definitions().ok()?;
        let contract = definitions.contracts.get(capability_id)?;
        let schema = contract.get("params_schema")?;
        let annotations = crate::capability::reference_annotations(schema).ok()?;
        let parameter = annotations
            .iter()
            .find_map(|(parameter, annotation)| (annotation == target).then_some(parameter))?;
        capability.params.get(parameter)?.as_str()
    }

    fn figure_radius_at(
        &self,
        direction: Dir,
        domain: &crate::body::Domain,
        level: u8,
    ) -> Result<f64, SampleError> {
        match self.root.figure.kind.as_str() {
            "sphere" => decimal_property(&self.root_value["figure"], "radius_m", f64::NAN),
            "star_convex_radial" => {
                let name = self.root_value["figure"]["radius_field"]
                    .as_str()
                    .ok_or(SampleError::UnsupportedField)?;
                let field = self
                    .fields()
                    .iter()
                    .find(|field| field.name == name && field.domain == domain.id)
                    .ok_or(SampleError::UnsupportedField)?;
                self.sample(&SampleQuery {
                    field: field.id,
                    pos: Position::Direction(direction),
                    level: LevelSel::Exact(level),
                    time: TimeSel::Static,
                })?
                .value
                .filter(|radius| radius.is_finite() && *radius > 0.0)
                .ok_or(SampleError::InvalidRaster)
            }
            _ => Err(SampleError::UnsupportedSelection),
        }
    }

    fn reference_surface_radius(
        &self,
        surface_id: &str,
        direction: Dir,
        domain: &crate::body::Domain,
        level: u8,
        depth: u8,
    ) -> Result<f64, SampleError> {
        if depth > 8 {
            return Err(SampleError::UnsupportedSelection);
        }
        let surface = self
            .root
            .reference_surfaces
            .iter()
            .find(|surface| surface.get("id").and_then(Value::as_str) == Some(surface_id))
            .ok_or(SampleError::UnsupportedSelection)?;
        match surface.get("kind").and_then(Value::as_str) {
            Some("sphere") => decimal_property(surface, "radius_m", f64::NAN),
            Some("figure_surface") => self.figure_radius_at(direction, domain, level),
            Some("offset_of") => {
                let base = surface
                    .get("base")
                    .and_then(Value::as_str)
                    .ok_or(SampleError::UnsupportedSelection)?;
                let offset = decimal_property(surface, "offset_m", 0.0)?;
                Ok(self.reference_surface_radius(base, direction, domain, level, depth + 1)?
                    + offset)
            }
            _ => Err(SampleError::UnsupportedSelection),
        }
    }

    /// Computes measure-weighted statistics for every indexed cell at a selected level.
    pub fn stats(
        &self,
        field_id: FieldId,
        level: LevelSel,
        time: TimeSel,
    ) -> Result<FieldStats, SampleError> {
        let values = self.weighted_cells(field_id, level, time)?;
        let mut stats = FieldStats {
            cells: u64::try_from(values.len()).unwrap_or(u64::MAX),
            nodata_cells: 0,
            valid_measure: 0.0,
            minimum: None,
            maximum: None,
            mean: None,
        };
        let mut weighted_sum = 0.0;
        for (value, measure) in values {
            let Some(value) = value else {
                stats.nodata_cells += 1;
                continue;
            };
            stats.valid_measure += measure;
            weighted_sum += value * measure;
            stats.minimum = Some(stats.minimum.map_or(value, |minimum| minimum.min(value)));
            stats.maximum = Some(stats.maximum.map_or(value, |maximum| maximum.max(value)));
        }
        if !stats.valid_measure.is_finite() || !weighted_sum.is_finite() {
            return Err(SampleError::InvalidScale);
        }
        if stats.valid_measure > 0.0 {
            stats.mean = Some(weighted_sum / stats.valid_measure);
        }
        Ok(stats)
    }

    /// Builds an equal-width histogram weighted by the topology's cell measure.
    pub fn histogram(
        &self,
        field_id: FieldId,
        bins: usize,
        level: LevelSel,
        time: TimeSel,
    ) -> Result<Histogram, SampleError> {
        if bins == 0 || bins > 65_536 {
            return Err(SampleError::UnsupportedSelection);
        }
        let values = self.weighted_cells(field_id, level, time)?;
        let minimum = values.iter().filter_map(|(value, _)| *value).reduce(f64::min);
        let maximum = values.iter().filter_map(|(value, _)| *value).reduce(f64::max);
        let (Some(minimum), Some(maximum)) = (minimum, maximum) else {
            return Ok(Histogram { minimum: None, maximum: None, bins: Vec::new() });
        };
        let width = if minimum == maximum { 0.0 } else { (maximum - minimum) / bins as f64 };
        if !width.is_finite() {
            return Err(SampleError::InvalidScale);
        }
        let mut output: Vec<HistogramBin> = (0..bins)
            .map(|index| HistogramBin {
                lower: minimum + index as f64 * width,
                upper: if index + 1 == bins {
                    maximum
                } else {
                    minimum + (index + 1) as f64 * width
                },
                weight: 0.0,
                cells: 0,
            })
            .collect();
        for (value, measure) in values {
            let Some(value) = value else { continue };
            let index = if width == 0.0 {
                0
            } else {
                libm::floor((value - minimum) / width).max(0.0) as usize
            }
            .min(bins - 1);
            output[index].weight += measure;
            output[index].cells += 1;
        }
        Ok(Histogram { minimum: Some(minimum), maximum: Some(maximum), bins: output })
    }

    fn weighted_cells(
        &self,
        field_id: FieldId,
        selection: LevelSel,
        time: TimeSel,
    ) -> Result<Vec<(Option<f64>, f64)>, SampleError> {
        let field = self.field(field_id)?;
        let domain = self.domain(&field.domain)?;
        let topology = topology_for(domain)?;
        let (level, _, _) = self.resolve_level(field, domain, selection)?;
        let index = self.indexes.get(&field_id).ok_or(SampleError::MissingTile)?;
        let mut output = Vec::new();
        for entry in index.entries.iter().filter(|entry| entry.level == level) {
            let tile = TileKey { level: entry.level, address: CellKey(entry.key) };
            let layout = topology.as_topology().tile_layout_for(tile, index.tile_log2)?;
            for j in 0..if domain.topology == "veyra.topo.radial_1d/1" { 1 } else { layout.edge } {
                for i in 0..layout.edge {
                    let key = topology.as_topology().tile_cell(tile, index.tile_log2, i, j)?;
                    let sample = self.sample(&SampleQuery {
                        field: field_id,
                        pos: Position::Cell { domain: field.domain.clone(), key },
                        level: LevelSel::Exact(level),
                        time,
                    })?;
                    let measure = topology.as_topology().cell_measure(key)?;
                    output.push((sample.value, measure));
                }
            }
        }
        Ok(output)
    }

    fn field(&self, id: FieldId) -> Result<&FieldDescriptor, SampleError> {
        self.registry.fields.iter().find(|field| field.id == id).ok_or(SampleError::UnknownField)
    }

    fn domain(&self, id: &str) -> Result<&crate::body::Domain, SampleError> {
        self.root.domains.iter().find(|domain| domain.id == id).ok_or(SampleError::UnknownDomain)
    }

    fn resolve_level(
        &self,
        field: &FieldDescriptor,
        domain: &crate::body::Domain,
        selection: LevelSel,
    ) -> Result<(u8, Option<SampleSource>, InterpolationMode), SampleError> {
        let interpolation = field
            .sampling
            .get("interp")
            .and_then(Value::as_str)
            .ok_or(SampleError::UnsupportedField)?;
        let mut mode = match (domain.topology.as_str(), interpolation) {
            ("veyra.topo.dir_cube/1", "nearest") => InterpolationMode::Nearest,
            ("veyra.topo.dir_cube/1", "bilinear") => InterpolationMode::Bilinear,
            ("veyra.topo.radial_1d/1", "nearest") => InterpolationMode::Nearest,
            ("veyra.topo.radial_1d/1", "linear") => InterpolationMode::Linear,
            _ => return Err(SampleError::UnsupportedSelection),
        };
        if matches!(field.semantic.as_str(), "category" | "feature_ref" | "flags") {
            mode = InterpolationMode::Nearest;
        }
        let target = match selection {
            LevelSel::Native => field.native_level,
            LevelSel::Exact(level) => level,
            LevelSel::Canonical => {
                let declared = field
                    .extra
                    .get("refinement")
                    .and_then(|value| value.get("params"))
                    .and_then(|value| value.get("canonical_max_level"))
                    .and_then(Value::as_u64)
                    .and_then(|value| u8::try_from(value).ok());
                if declared.is_none()
                    && field.sampling.get("above_native").and_then(Value::as_str) == Some("refine")
                {
                    return Err(SampleError::UnsupportedRefinement);
                }
                declared.unwrap_or(field.native_level)
            }
        };
        if target > domain.max_level || target > 30 {
            return Err(SampleError::UnsupportedResolution);
        }
        if target < field.native_level {
            if field.sampling.get("below_native").and_then(Value::as_str) != Some("pyramid") {
                return Err(SampleError::UnsupportedResolution);
            }
            return Ok((target, Some(SampleSource::Pyramid), mode));
        }
        if target == field.native_level {
            return Ok((target, None, mode));
        }
        let above = field.sampling.get("above_native").and_then(Value::as_str).unwrap_or("none");
        match above {
            "inherit" => {
                Ok((field.native_level, Some(SampleSource::Inherited), InterpolationMode::Nearest))
            }
            "smooth_only" => Ok((field.native_level, Some(SampleSource::Stored), mode)),
            "none" => Err(SampleError::UnsupportedResolution),
            "refine" => Err(SampleError::UnsupportedRefinement),
            _ => Err(SampleError::UnsupportedSelection),
        }
    }

    fn topology_point(
        &self,
        position: &Position,
        domain_id: &str,
        topology: &dyn Topology,
    ) -> Result<TopologyPoint, SampleError> {
        let domain = self.domain(domain_id)?;
        let point = match position {
            Position::Direction(direction) => TopologyPoint::Direction(*direction),
            Position::AxialLatLon { lat_rad, lon_rad } => {
                if !lat_rad.is_finite() || lat_rad.abs() > core::f64::consts::FRAC_PI_2 {
                    return Err(SampleError::InvalidPosition);
                }
                TopologyPoint::Direction(Dir::from_axial_lat_lon(*lat_rad, *lon_rad)?)
            }
            Position::Cell { domain, key } => {
                if domain != domain_id {
                    return Err(SampleError::InvalidPosition);
                }
                topology.point_for_cell(*key)?
            }
            Position::Radial { r_m } => {
                if domain.topology != "veyra.topo.radial_1d/1" || !r_m.is_finite() || *r_m < 0.0 {
                    return Err(SampleError::InvalidPosition);
                }
                let extent = radial_extent(domain)?;
                TopologyPoint::RadialFraction(r_m / extent)
            }
            Position::Pos30(position) => {
                if domain.topology != "veyra.topo.dir_cube/1" {
                    return Err(SampleError::InvalidPosition);
                }
                let key = DirCube::key(
                    position.face,
                    u64::from(position.i30),
                    u64::from(position.j30),
                    30,
                )?;
                topology.point_for_cell(key)?
            }
            Position::Local(local) => {
                if domain.topology == "veyra.topo.radial_1d/1" {
                    let extent = radial_extent(domain)?;
                    let (cell_level, cell_index) = Radial1d::decode(local.cell)?;
                    if !local.radial_m.is_finite() || local.radial_m < 0.0 {
                        return Err(SampleError::InvalidPosition);
                    }
                    let cell_count = (1_u64 << cell_level) as f64;
                    let minimum = cell_index as f64 / cell_count * extent;
                    let maximum = (cell_index + 1) as f64 / cell_count * extent;
                    if local.radial_m < minimum || local.radial_m > maximum {
                        return Err(SampleError::InvalidPosition);
                    }
                    TopologyPoint::RadialFraction(local.radial_m / extent)
                } else if domain.topology == "veyra.topo.dir_cube/1" {
                    DirCube::decode(local.cell)?;
                    let direction = DirCube.direction_at_local(local.cell, local.du, local.dv)?;
                    TopologyPoint::Direction(direction)
                } else {
                    return Err(SampleError::InvalidPosition);
                }
            }
        };
        match (domain.topology.as_str(), point) {
            ("veyra.topo.dir_cube/1", TopologyPoint::Direction(_))
            | ("veyra.topo.radial_1d/1", TopologyPoint::RadialFraction(_)) => Ok(point),
            _ => Err(SampleError::InvalidPosition),
        }
    }

    fn raw_at(
        &self,
        field: &FieldDescriptor,
        index: &crate::canon::index::IndexBlob,
        topology: &dyn Topology,
        cell: CellKey,
        slice: u16,
    ) -> Result<(RawValue, StoredSource), SampleError> {
        let tile = topology.tile_key(cell, index.tile_log2)?;
        let entry = find_entry(&index.entries, tile).ok_or(SampleError::MissingTile)?;
        match entry.value {
            IndexValue::Const(raw) => {
                let raw = if crate::body::field_dtype(field) == Some(DType::F32) {
                    let bits = u32::try_from(raw).map_err(|_| SampleError::InvalidRaster)?;
                    RawValue::Float(f32::from_bits(bits))
                } else {
                    RawValue::Integer(raw)
                };
                Ok((raw, StoredSource::Const))
            }
            IndexValue::Blob(hash) => {
                let canonical =
                    self.blobs.get(&hash).ok_or(SampleError::Missing(vec![Need::Blob { hash }]))?;
                let blob =
                    CanonicalBlob::decode(canonical).map_err(|_| SampleError::InvalidRaster)?;
                if blob.kind != BlobKind::RasterTile
                    || crate::body::field_dtype(field) != Some(blob.dtype)
                {
                    return Err(SampleError::InvalidRaster);
                }
                let (i, j) = topology.tile_offset(cell, index.tile_log2)?;
                if i >= u64::from(blob.dim_i) || j >= u64::from(blob.dim_j) || slice >= blob.slices
                {
                    return Err(SampleError::InvalidRaster);
                }
                let pixel = (usize::from(slice) * usize::from(blob.dim_j) + j as usize)
                    * usize::from(blob.dim_i)
                    + i as usize;
                Ok((decode_raw(blob.dtype, &blob.payload, pixel)?, StoredSource::Blob))
            }
        }
    }
}

fn topology_for(domain: &crate::body::Domain) -> Result<TopologyInstance, SampleError> {
    match domain.topology.as_str() {
        "veyra.topo.dir_cube/1" => Ok(TopologyInstance::DirCube(DirCube)),
        "veyra.topo.radial_1d/1" => {
            Ok(TopologyInstance::Radial(Radial1d::with_extent(radial_extent(domain)?)?))
        }
        _ => Err(SampleError::UnsupportedField),
    }
}

fn radial_extent(domain: &crate::body::Domain) -> Result<f64, SampleError> {
    let value = domain
        .vertical
        .get("extent_m")
        .and_then(Value::as_str)
        .ok_or(SampleError::InvalidPosition)?;
    DecimalString::parse(value)
        .and_then(|decimal| decimal.to_f64())
        .map_err(|_| SampleError::InvalidPosition)
}

fn find_entry(entries: &[IndexEntry], tile: TileKey) -> Option<&IndexEntry> {
    entries
        .binary_search_by_key(&(tile.level, tile.address.0), |entry| (entry.level, entry.key))
        .ok()
        .map(|index| &entries[index])
}

fn ensure_supported_field(field: &FieldDescriptor) -> Result<(), SampleError> {
    if crate::body::field_dtype(field).is_none()
        || !(field.semantic.starts_with("scalar.")
            || matches!(field.semantic.as_str(), "category" | "feature_ref" | "flags"))
    {
        return Err(SampleError::UnsupportedField);
    }
    Ok(())
}

fn select_slices(
    field: &FieldDescriptor,
    selection: TimeSel,
) -> Result<(Vec<u16>, Reduction), SampleError> {
    let kind = field.temporal.get("kind").and_then(Value::as_str).unwrap_or("static");
    if kind == "static" {
        if !matches!(selection, TimeSel::Static | TimeSel::Mean | TimeSel::Min | TimeSel::Max) {
            return Err(SampleError::UnsupportedSelection);
        }
        return Ok((vec![0], Reduction::Single));
    }
    if kind != "periodic_slices" {
        return Err(SampleError::UnsupportedSelection);
    }
    let count = field
        .temporal
        .get("count")
        .and_then(Value::as_u64)
        .and_then(|count| u16::try_from(count).ok())
        .filter(|count| *count > 0)
        .ok_or(SampleError::UnsupportedSelection)?;
    match selection {
        TimeSel::Slice(slice) if slice < count => Ok((vec![slice], Reduction::Single)),
        TimeSel::Slice(_) => Err(SampleError::UnsupportedSelection),
        TimeSel::Phase(phase) if phase.is_finite() => {
            let cycle = phase.rem_euclid(1.0);
            let slice = libm::floor(cycle * f64::from(count)) as u16;
            Ok((vec![slice.min(count - 1)], Reduction::Single))
        }
        TimeSel::Phase(_) => Err(SampleError::UnsupportedSelection),
        TimeSel::Mean => Ok(((0..count).collect(), Reduction::Mean)),
        TimeSel::Min => Ok(((0..count).collect(), Reduction::Min)),
        TimeSel::Max => Ok(((0..count).collect(), Reduction::Max)),
        TimeSel::Static => {
            let default =
                field.temporal.get("reduce_default").and_then(Value::as_str).unwrap_or("mean");
            let reduction = match default {
                "mean" => Reduction::Mean,
                "min" => Reduction::Min,
                "max" => Reduction::Max,
                _ => return Err(SampleError::UnsupportedSelection),
            };
            Ok(((0..count).collect(), reduction))
        }
    }
}

fn decode_raw(dtype: DType, payload: &[u8], pixel: usize) -> Result<RawValue, SampleError> {
    let width = dtype.width().ok_or(SampleError::InvalidRaster)?;
    let start = pixel.checked_mul(width).ok_or(SampleError::InvalidRaster)?;
    let bytes = payload.get(start..start + width).ok_or(SampleError::InvalidRaster)?;
    match dtype {
        DType::U8 => Ok(RawValue::Integer(i64::from(bytes[0]))),
        DType::I8 => Ok(RawValue::Integer(i64::from(bytes[0] as i8))),
        DType::U16 => Ok(RawValue::Integer(i64::from(u16::from_le_bytes(
            bytes.try_into().map_err(|_| SampleError::InvalidRaster)?,
        )))),
        DType::I16 => Ok(RawValue::Integer(i64::from(i16::from_le_bytes(
            bytes.try_into().map_err(|_| SampleError::InvalidRaster)?,
        )))),
        DType::U32 => Ok(RawValue::Integer(i64::from(u32::from_le_bytes(
            bytes.try_into().map_err(|_| SampleError::InvalidRaster)?,
        )))),
        DType::I32 => Ok(RawValue::Integer(i64::from(i32::from_le_bytes(
            bytes.try_into().map_err(|_| SampleError::InvalidRaster)?,
        )))),
        DType::F32 => Ok(RawValue::Float(f32::from_bits(u32::from_le_bytes(
            bytes.try_into().map_err(|_| SampleError::InvalidRaster)?,
        )))),
        DType::Raw => Err(SampleError::InvalidRaster),
    }
}

fn is_nodata(field: &FieldDescriptor, raw: RawValue) -> bool {
    let Some(value) = field.storage.get("nodata") else {
        return false;
    };
    if value.is_null() {
        return false;
    }
    match raw {
        RawValue::Integer(raw) => {
            value.as_i64().or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
                == Some(raw)
        }
        RawValue::Float(raw) => value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
            .and_then(|bits| u32::try_from(bits).ok())
            .is_some_and(|bits| raw.to_bits() == bits),
    }
}

fn decode_physical(field: &FieldDescriptor, raw: RawValue) -> Result<f64, SampleError> {
    let scale = decimal_property(&field.storage, "scale", 1.0)?;
    let offset = decimal_property(&field.storage, "offset", 0.0)?;
    let value = raw.as_f64() * scale + offset;
    if !value.is_finite() {
        return Err(SampleError::InvalidScale);
    }
    Ok(value)
}

fn temporal_mean(values: &[f64]) -> Result<f64, SampleError> {
    if values.is_empty() || values.iter().any(|value| !value.is_finite()) {
        return Err(SampleError::InvalidScale);
    }
    let sum = values.iter().sum::<f64>();
    let mean = if sum.is_finite() {
        sum / values.len() as f64
    } else {
        let scale =
            values.iter().fold(0.0_f64, |largest, value| libm::fmax(largest, libm::fabs(*value)));
        if scale == 0.0 {
            0.0
        } else {
            values.iter().map(|value| value / scale).sum::<f64>() / values.len() as f64 * scale
        }
    };
    if !mean.is_finite() {
        return Err(SampleError::InvalidScale);
    }
    Ok(mean)
}

fn angular_distance(a: Dir, b: Dir) -> Result<f64, SampleError> {
    let (ax, ay, az) = (a.x(), a.y(), a.z());
    let (bx, by, bz) = (b.x(), b.y(), b.z());
    let norm_a = libm::sqrt(ax * ax + ay * ay + az * az);
    let norm_b = libm::sqrt(bx * bx + by * by + bz * bz);
    let normalization = norm_a * norm_b;
    if !normalization.is_finite() || normalization <= 0.0 {
        return Err(SampleError::InvalidScale);
    }
    let dot = ((ax * bx + ay * by + az * bz) / normalization).clamp(-1.0, 1.0);
    let cross_x = ay * bz - az * by;
    let cross_y = az * bx - ax * bz;
    let cross_z = ax * by - ay * bx;
    let sine =
        libm::sqrt(cross_x * cross_x + cross_y * cross_y + cross_z * cross_z) / normalization;
    let angle = libm::atan2(sine, dot);
    if !angle.is_finite() || angle <= 0.0 {
        return Err(SampleError::InvalidScale);
    }
    Ok(angle)
}

fn slope_from_cardinal_samples(values: &[f64], distances: &[f64]) -> Result<f64, SampleError> {
    if values.len() != 4
        || distances.len() != 4
        || values.iter().any(|value| !value.is_finite())
        || distances.iter().any(|distance| !distance.is_finite() || *distance <= 0.0)
    {
        return Err(SampleError::InvalidScale);
    }
    let u_span = distances[0] + distances[1];
    let v_span = distances[2] + distances[3];
    if !u_span.is_finite() || !v_span.is_finite() || u_span <= 0.0 || v_span <= 0.0 {
        return Err(SampleError::InvalidScale);
    }
    let u_gradient = (values[1] - values[0]) / u_span;
    let v_gradient = (values[3] - values[2]) / v_span;
    let slope = libm::atan(libm::sqrt(u_gradient * u_gradient + v_gradient * v_gradient));
    if !slope.is_finite() {
        return Err(SampleError::InvalidScale);
    }
    Ok(slope)
}

fn decimal_property(object: &Value, name: &str, default: f64) -> Result<f64, SampleError> {
    let Some(value) = object.get(name) else {
        return Ok(default);
    };
    let value = value.as_str().ok_or(SampleError::InvalidScale)?;
    let parsed = DecimalString::parse(value)
        .and_then(|decimal| decimal.to_f64())
        .map_err(|_| SampleError::InvalidScale)?;
    if !parsed.is_finite() {
        return Err(SampleError::InvalidScale);
    }
    Ok(parsed)
}

fn raw_to_i64(raw: RawValue) -> Option<i64> {
    match raw {
        RawValue::Integer(value) => Some(value),
        RawValue::Float(_) => None,
    }
}

fn query_level_is_pyramid(field: &FieldDescriptor, selection: LevelSel, level: u8) -> bool {
    selection != LevelSel::Native && level < field.native_level
}

#[cfg(test)]
mod tests {
    use super::{
        LevelSel, Position, SampleQuery, TimeSel, angular_distance, slope_from_cardinal_samples,
        temporal_mean,
    };
    use crate::body::FieldId;
    use crate::canon::blob::{BlobKind, CanonicalBlob, DType};
    use crate::canon::hash;
    use crate::canon::index::{IndexBlob, IndexEntry, IndexValue, TopologyTag};
    use crate::ids::Hash32;
    use crate::io::{Body, Need};
    use crate::spatial::{Dir, DirCube, TileKey, Topology};
    use serde_json::json;

    #[test]
    fn query_types_keep_topology_and_time_selection_explicit() {
        let query = SampleQuery {
            field: FieldId::new(0x0101, 1),
            pos: Position::Direction(Dir::new(1.0, 0.0, 0.0).unwrap()),
            level: LevelSel::Native,
            time: TimeSel::Static,
        };
        assert!(matches!(query.pos, Position::Direction(_)));
        assert_eq!(DirCube.locate(Dir::new(1.0, 0.0, 0.0).unwrap(), 2).unwrap().0 >> 61, 0);
    }

    #[test]
    fn sample_plan_reports_missing_blobs_and_accepts_caller_supplied_canonical_content() {
        let field_id = FieldId::new(0x7ffe, 1);
        let cell = DirCube::key(0, 0, 0, 0).unwrap();
        let canonical = CanonicalBlob::new(
            BlobKind::RasterTile,
            DType::I16,
            1,
            1,
            1,
            42_i16.to_le_bytes().to_vec(),
        )
        .unwrap()
        .encode();
        let blob_hash = hash::hash(&canonical);
        let index = IndexBlob {
            field_id: field_id.0,
            topology: TopologyTag::DirCube,
            tile_log2: 0,
            entries: vec![IndexEntry { level: 0, key: cell.0, value: IndexValue::Blob(blob_hash) }],
        };
        let mut root_value: serde_json::Value = serde_json::from_str(include_str!(
            "../../../conformance/worlds/cb9-minimal-void/body.json"
        ))
        .unwrap();
        root_value["required_features"]
            .as_array_mut()
            .unwrap()
            .push(json!("veyra.topo.dir_cube/1"));
        root_value["domains"] = json!([{"id":"surface","topology":"veyra.topo.dir_cube/1","frame":"body_fixed","vertical":{"kind":"none"},"tile_log2":0,"max_level":0}]);
        root_value["capabilities"] =
            json!([{"id":"veyra.cap.conformance_probe/1","params":{},"compat":"ancillary"}]);
        root_value["indexes"] = json!({field_id.to_string():blob_hash.to_string()});
        let root = serde_json::from_value(root_value.clone()).unwrap();
        let registry = serde_json::from_value(json!({
            "schema":"veyra.field_registry/1",
            "fields":[{
                "id":field_id.to_string(),"name":"x-value","capability":"veyra.cap.conformance_probe/1",
                "domain":"surface","semantic":"scalar.temperature","persistence":"invariant",
                "storage":{"dtype":"i16","scale":"1","offset":"0"},"unit":"K",
                "native_level":0,"temporal":{"kind":"static"},"sampling":{"interp":"nearest"},
                "downsample":"mean","compat":"ancillary"
            }]
        }))
        .unwrap();
        let mut body = Body {
            root,
            root_value,
            baseline_id: Hash32([0; 32]),
            registry,
            sections: std::collections::BTreeMap::new(),
            indexes: std::collections::BTreeMap::from([(field_id, index)]),
            blobs: std::collections::BTreeMap::new(),
            ledgers: std::collections::BTreeMap::new(),
        };
        let query = SampleQuery {
            field: field_id,
            pos: Position::Direction(Dir::new(1.0, 0.0, 0.0).unwrap()),
            level: LevelSel::Native,
            time: TimeSel::Static,
        };
        assert_eq!(body.plan(&query).unwrap(), vec![Need::Blob { hash: blob_hash }]);
        assert!(matches!(body.sample(&query), Err(super::SampleError::Missing(_))));
        body.provide_blob(blob_hash, &canonical).unwrap();
        assert!(body.plan(&query).unwrap().is_empty());
        assert_eq!(body.sample(&query).unwrap().value, Some(42.0));
        let tile_key = DirCube.tile_key(cell, 0).unwrap();
        assert_eq!(tile_key, TileKey { level: 0, address: cell });
    }

    #[test]
    fn angular_distance_and_slope_remain_stable_at_level_thirty() {
        let key = DirCube::key(0, 1 << 29, 1 << 29, 30).unwrap();
        let neighbor = DirCube.neighbor(key, crate::spatial::FaceEdge::UPlus).unwrap();
        let center = DirCube.cell_center(key).unwrap();
        let adjacent = DirCube.cell_center(neighbor).unwrap();
        let dot = center.x() * adjacent.x() + center.y() * adjacent.y() + center.z() * adjacent.z();
        assert_eq!(libm::acos(dot), 0.0, "the old acos path loses this separation");

        let angle = angular_distance(center, adjacent).unwrap();
        assert!(angle.is_finite() && angle > 0.0);
        let constant = slope_from_cardinal_samples(&[17.0; 4], &[angle; 4]).unwrap();
        assert_eq!(constant, 0.0);
        let gradient = slope_from_cardinal_samples(&[0.0, 2.0, 0.0, 2.0], &[angle; 4]).unwrap();
        assert!(gradient.is_finite() && gradient > 0.0);
    }

    #[test]
    fn temporal_mean_handles_large_finite_values_and_rejects_invalid_inputs() {
        assert_eq!(temporal_mean(&[1.0e308, 1.0e308]).unwrap(), 1.0e308);
        assert_eq!(temporal_mean(&[-1.0e308, -1.0e308]).unwrap(), -1.0e308);
        let mixed = temporal_mean(&[1.0e308, 1.0e308, -1.0e308]).unwrap();
        assert!(libm::fabs(mixed - (1.0e308 / 3.0)) < 1.0e292);
        assert_eq!(temporal_mean(&[1.0, 2.0, 3.0]).unwrap(), 2.0);
        assert!(temporal_mean(&[]).is_err());
        assert!(temporal_mean(&[f64::NAN]).is_err());
        assert!(temporal_mean(&[f64::INFINITY]).is_err());
    }
}
