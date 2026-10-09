//! Topology-driven field sampling over verified body content.

use core::fmt;
use core::mem::size_of;

use serde_json::Value;

use crate::body::{FieldDescriptor, FieldId, parse_hash};
use crate::canon::blob::{BlobKind, CanonicalBlobView, DType};
use crate::canon::hash;
use crate::canon::index::{IndexEntry, IndexValue};
use crate::ids::Hash32;
use crate::io::{Body, Need};
use crate::spatial::{
    CellKey, Dir, DirCube, InterpolationMode, LocalPos, Pos30, Radial1d, SpatialError, TileKey,
    Topology, TopologyPoint,
};
use crate::time::DecimalString;

/// Maximum memory reserved for one materialized [`TileData::values`] result.
///
/// This API-only output budget does not restrict canonical tile sizes or body resolutions. A
/// production 128 × 128 tile with a one-cell halo requires about 264 KiB when represented as
/// `Option<f64>`, leaving substantial headroom under this 16 MiB per-result cap.
pub const MAX_MATERIALIZED_TILE_BYTES: usize = 16 * 1024 * 1024;

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
    /// Arithmetic mean of available scalar periodic slices; discrete fields reject reductions.
    Mean,
    /// Minimum of available scalar periodic slices; discrete fields reject reductions.
    Min,
    /// Maximum of available scalar periodic slices; discrete fields reject reductions.
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
    /// A tile combined values with more than one contributing source class.
    Mixed,
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

/// One field-independent request for a topology-derived view tile.
#[derive(Clone, Debug, PartialEq)]
pub struct TopologyTileRequest {
    /// Spatial domain that owns the topology view.
    pub domain: String,
    /// Canonical tile address in the domain.
    pub key: TileKey,
    /// Number of assembled halo cells; V1 currently permits zero or one.
    pub halo: u8,
    /// Topology-derived view ID from [`Body::views`].
    pub view: String,
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
    /// Inclusive lower edge, or the exact discrete label when `lower == upper`.
    pub lower: f64,
    /// Exclusive upper edge, except for the final numeric bin; discrete bins use a singleton label.
    pub upper: f64,
    /// Sum of cell measures in the bin.
    pub weight: f64,
    /// Number of contributing cells in the bin.
    pub cells: u64,
}

/// Measure-weighted histogram of a field view.
#[derive(Clone, Debug, PartialEq)]
pub struct Histogram {
    /// Minimum observed numeric value; absent for nominal discrete fields.
    pub minimum: Option<f64>,
    /// Maximum observed numeric value; absent for nominal discrete fields.
    pub maximum: Option<f64>,
    /// Equal-width numeric bins or one exact-label bin per discrete value.
    pub bins: Vec<HistogramBin>,
}

#[derive(Clone, Copy, Debug)]
struct WeightedCell {
    value: Option<f64>,
    raw: Option<RawValue>,
    measure: f64,
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
    /// Different discrete values cannot be averaged at a cube-corner halo.
    UnsupportedDiscreteCornerHalo,
    /// The requested tile result exceeds the bounded materialization budget.
    TileOutputLimitExceeded,
    /// A bounded tile result could not reserve its output buffer.
    AllocationFailed,
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
            Self::UnsupportedDiscreteCornerHalo => {
                formatter.write_str("discrete values cannot be averaged at a cube-corner halo")
            }
            Self::TileOutputLimitExceeded => {
                formatter.write_str("materialized tile exceeds the core output limit")
            }
            Self::AllocationFailed => {
                formatter.write_str("could not reserve the bounded tile output buffer")
            }
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
        let (_, stencil) = self.stencil_for_position(
            &query.pos,
            &field.domain,
            topology.as_topology(),
            level,
            mode,
        )?;
        let (slices, reduction) = select_slices(field, query.time)?;
        ensure_discrete_reduction(field, reduction)?;
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
        let (primary_cell, stencil) = self.stencil_for_position(
            &query.pos,
            &field.domain,
            topology.as_topology(),
            level,
            mode,
        )?;
        let needs = self.plan(query)?;
        if !needs.is_empty() {
            return Err(SampleError::Missing(needs));
        }
        let index = self.indexes.get(&field.id).ok_or(SampleError::MissingTile)?;
        let (slices, reduction) = select_slices(field, query.time)?;
        ensure_discrete_reduction(field, reduction)?;
        let mut values = Vec::new();
        let mut all_const = true;
        let mut primary_raw = None;
        for slice in slices {
            let mut weighted_value = 0.0;
            let mut total_weight = 0.0;
            for weighted_cell in &stencil {
                let (raw, source) =
                    self.raw_at(field, index, topology.as_topology(), weighted_cell.key, slice)?;
                if weighted_cell.key == primary_cell && primary_raw.is_none() {
                    primary_raw = Some(raw);
                }
                if is_nodata(field, raw) {
                    continue;
                }
                all_const &= source == StoredSource::Const;
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
        let category =
            if value.is_some() && matches!(field.semantic.as_str(), "category" | "feature_ref") {
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
        let blob = CanonicalBlobView::decode(canonical).map_err(|_| SampleError::InvalidRaster)?;
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
                    .find(|view| {
                        view.id == *id
                            && view.domain == field.domain
                            && view.accepts_field(field.id)
                    })
                    .ok_or(SampleError::UnsupportedSelection)?;
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
        if !topology_view {
            let (_, reduction) = select_slices(field, request.time)?;
            ensure_discrete_reduction(field, reduction)?;
        }
        topology.validate_tile_key(request.key, domain.tile_log2)?;
        if request.key.level > domain.max_level {
            return Err(SampleError::UnsupportedResolution);
        }
        let layout = topology.tile_raster_layout(request.key, domain.tile_log2, request.halo)?;
        let output_len = materialized_tile_cell_count(layout.dim_i, layout.dim_j)?;
        let dim_i =
            u16::try_from(layout.dim_i).map_err(|_| SampleError::TileOutputLimitExceeded)?;
        let dim_j =
            u16::try_from(layout.dim_j).map_err(|_| SampleError::TileOutputLimitExceeded)?;
        let mut values = Vec::new();
        values.try_reserve_exact(output_len).map_err(|_| SampleError::AllocationFailed)?;
        let mut source = None;
        let discrete_output = is_discrete_tile_output(field, derived_view.as_ref());
        for j in 0..i64::from(dim_j) {
            for i in 0..i64::from(dim_i) {
                let local_i = i + layout.offset_i;
                let local_j = j + layout.offset_j;
                let cells =
                    topology.tile_halo_cells(request.key, domain.tile_log2, local_i, local_j)?;
                let mut sum = 0.0;
                let mut weight = 0.0;
                let mut discrete_value = None;
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
                        if discrete_output {
                            discrete_value = merge_discrete_corner_value(discrete_value, value)?;
                        } else {
                            sum += value * cell.weight;
                            weight += cell.weight;
                        }
                        source = merge_contributing_source(source, Some(value), sampled_source);
                    }
                }
                values.push(if discrete_output {
                    discrete_value
                } else {
                    (weight > 0.0).then_some(sum / weight)
                });
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

    /// Returns a topology-derived view tile without requiring a registered field.
    pub fn topology_tile(&self, request: &TopologyTileRequest) -> Result<TileData, SampleError> {
        if request.halo > 1 {
            return Err(SampleError::UnsupportedSelection);
        }
        let domain = self.domain(&request.domain)?;
        let topology = topology_for(domain)?;
        let topology = topology.as_topology();
        let view = self
            .views()
            .into_iter()
            .find(|view| {
                view.domain == request.domain
                    && view.id == request.view
                    && view.field.is_none()
                    && view.capability.is_none()
                    && view.operator.as_deref() == Some(request.view.as_str())
            })
            .ok_or(SampleError::UnsupportedSelection)?;
        let operator = view.operator.as_deref().ok_or(SampleError::UnsupportedSelection)?;
        topology.validate_tile_key(request.key, domain.tile_log2)?;
        if request.key.level > domain.max_level {
            return Err(SampleError::UnsupportedResolution);
        }
        let layout = topology.tile_raster_layout(request.key, domain.tile_log2, request.halo)?;
        let output_len = materialized_tile_cell_count(layout.dim_i, layout.dim_j)?;
        let dim_i =
            u16::try_from(layout.dim_i).map_err(|_| SampleError::TileOutputLimitExceeded)?;
        let dim_j =
            u16::try_from(layout.dim_j).map_err(|_| SampleError::TileOutputLimitExceeded)?;
        let discrete_output = is_discrete_topology_view(operator);
        let mut values = Vec::new();
        values.try_reserve_exact(output_len).map_err(|_| SampleError::AllocationFailed)?;
        for j in 0..i64::from(dim_j) {
            for i in 0..i64::from(dim_i) {
                let cells = topology.tile_halo_cells(
                    request.key,
                    domain.tile_log2,
                    i + layout.offset_i,
                    j + layout.offset_j,
                )?;
                let mut sum = 0.0;
                let mut weight = 0.0;
                let mut discrete_value = None;
                for cell in cells {
                    let value =
                        topology_derived_value(operator, topology, cell.key, request.key.level)?;
                    if discrete_output {
                        discrete_value = merge_discrete_corner_value(discrete_value, value)?;
                    } else {
                        sum += value * cell.weight;
                        weight += cell.weight;
                    }
                }
                values.push(if discrete_output {
                    discrete_value
                } else {
                    (weight > 0.0).then_some(sum / weight)
                });
            }
        }
        Ok(TileData {
            key: request.key,
            dim_i,
            dim_j,
            slices: 1,
            values,
            source: SampleSource::Derived,
        })
    }

    fn derived_value(&self, query: DerivedTileQuery<'_>) -> Result<Option<f64>, SampleError> {
        let DerivedTileQuery { view, field, domain, topology, cell, level, time } = query;
        let operator = view.operator.as_deref().ok_or(SampleError::UnsupportedSelection)?;
        if operator.starts_with("topology.") {
            return topology_derived_value(operator, topology, cell, level).map(Some);
        }
        match operator {
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
        validate_derived_view_field("core.slope/1", field, domain)?;
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
        validate_derived_view_field("core.threshold_partition/1", field, domain)?;
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
        if is_discrete_semantic(&self.field(field_id)?.semantic) {
            return Err(SampleError::UnsupportedSelection);
        }
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
        for cell in &values {
            let Some(value) = cell.value else {
                stats.nodata_cells += 1;
                continue;
            };
            stats.valid_measure += cell.measure;
            weighted_sum += value * cell.measure;
            stats.minimum = Some(stats.minimum.map_or(value, |minimum| minimum.min(value)));
            stats.maximum = Some(stats.maximum.map_or(value, |maximum| maximum.max(value)));
        }
        if !stats.valid_measure.is_finite() {
            return Err(SampleError::InvalidScale);
        }
        if stats.valid_measure > 0.0 {
            let mean = weighted_sum / stats.valid_measure;
            stats.mean = if mean.is_finite() { Some(mean) } else { stable_weighted_mean(&values)? };
        }
        Ok(stats)
    }

    /// Builds a topology-measure-weighted histogram.
    ///
    /// Continuous fields use equal-width numeric bins. Discrete fields use one exact raw-code bin
    /// per observed label in canonical cell traversal order; `bins` is their maximum category
    /// count, so labels are never combined or numerically ordered.
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
        let field = self.field(field_id)?;
        let values = self.weighted_cells(field_id, level, time)?;
        if is_discrete_semantic(&field.semantic) {
            // The map only resolves exact labels to their first-seen output slot. Iteration and
            // emitted order remain the canonical cell traversal order, not numeric code order.
            let mut category_indices = std::collections::BTreeMap::new();
            let mut counts: Vec<(i64, f64, u64)> = Vec::new();
            for cell in values {
                if cell.value.is_none() {
                    continue;
                }
                let Some(RawValue::Integer(code)) = cell.raw else {
                    return Err(SampleError::UnsupportedField);
                };
                let index = if let Some(index) = category_indices.get(&code) {
                    *index
                } else {
                    if counts.len() == bins {
                        return Err(SampleError::UnsupportedSelection);
                    }
                    let index = counts.len();
                    category_indices.insert(code, index);
                    counts.push((code, 0.0, 0));
                    index
                };
                let (_, weight, cells) = &mut counts[index];
                *weight += cell.measure;
                *cells += 1;
            }
            let bins = counts
                .into_iter()
                .map(|(code, weight, cells)| {
                    let label = code as f64;
                    HistogramBin { lower: label, upper: label, weight, cells }
                })
                .collect();
            return Ok(Histogram { minimum: None, maximum: None, bins });
        }
        let minimum = values.iter().filter_map(|cell| cell.value).reduce(f64::min);
        let maximum = values.iter().filter_map(|cell| cell.value).reduce(f64::max);
        let (Some(minimum), Some(maximum)) = (minimum, maximum) else {
            return Ok(Histogram { minimum: None, maximum: None, bins: Vec::new() });
        };
        let range = maximum - minimum;
        let width = if range.is_finite() { range / bins as f64 } else { 0.0 };
        let mut output: Vec<HistogramBin> = (0..bins)
            .map(|index| HistogramBin {
                lower: numeric_histogram_edge(minimum, maximum, range, width, bins, index),
                upper: numeric_histogram_edge(minimum, maximum, range, width, bins, index + 1),
                weight: 0.0,
                cells: 0,
            })
            .collect();
        for cell in values {
            let Some(value) = cell.value else { continue };
            let index = numeric_histogram_index(value, minimum, maximum, &output);
            output[index].weight += cell.measure;
            output[index].cells += 1;
        }
        Ok(Histogram { minimum: Some(minimum), maximum: Some(maximum), bins: output })
    }

    fn weighted_cells(
        &self,
        field_id: FieldId,
        selection: LevelSel,
        time: TimeSel,
    ) -> Result<Vec<WeightedCell>, SampleError> {
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
                    output.push(WeightedCell { value: sample.value, raw: sample.raw, measure });
                }
            }
        }
        Ok(output)
    }

    fn field(&self, id: FieldId) -> Result<&FieldDescriptor, SampleError> {
        self.registry.fields.iter().find(|field| field.id == id).ok_or(SampleError::UnknownField)
    }

    pub(crate) fn supports_derived_view_field(
        &self,
        operator: &str,
        field: &FieldDescriptor,
        domain: &crate::body::Domain,
    ) -> bool {
        if validate_derived_view_field(operator, field, domain).is_err()
            || !self.indexes.get(&field.id).is_some_and(|index| !index.entries.is_empty())
            || self.resolve_level(field, domain, LevelSel::Native).is_err()
        {
            return false;
        }
        if operator == "core.threshold_partition/1" {
            let field_surface = field
                .extra
                .get("reference")
                .and_then(|reference| reference.get("surface"))
                .and_then(Value::as_str)
                .or_else(|| self.capability_reference(&field.capability, "reference_surface"));
            if field_surface.is_none_or(|surface_id| {
                !self
                    .root
                    .reference_surfaces
                    .iter()
                    .any(|surface| surface.get("id").and_then(Value::as_str) == Some(surface_id))
            }) {
                return false;
            }
        }
        let time = if field.temporal.get("kind").and_then(Value::as_str) == Some("periodic_slices")
        {
            TimeSel::Slice(0)
        } else {
            TimeSel::Static
        };
        select_slices(field, time).is_ok()
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
        if is_discrete_semantic(&field.semantic) {
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
                let coordinate_count = 1_u32 << 30;
                if position.face > 5
                    || position.i30 >= coordinate_count
                    || position.j30 >= coordinate_count
                {
                    return Err(SampleError::InvalidPosition);
                }
                let coordinate_scale = f64::from(coordinate_count);
                TopologyPoint::Direction(DirCube.direction_at_face_st(
                    position.face,
                    f64::from(position.i30) / coordinate_scale,
                    f64::from(position.j30) / coordinate_scale,
                )?)
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

    fn stencil_for_position(
        &self,
        position: &Position,
        domain_id: &str,
        topology: &dyn Topology,
        level: u8,
        mode: InterpolationMode,
    ) -> Result<(CellKey, Vec<crate::spatial::WeightedCell>), SampleError> {
        let point = self.topology_point(position, domain_id, topology)?;
        if let Position::Cell { key, .. } = position
            && topology.level(*key)? == level
        {
            // Position::Cell carries the canonical identity directly. Avoid projecting its
            // computed center to a direction and back, which can leave tiny bilinear weights.
            return Ok((*key, vec![crate::spatial::WeightedCell { key: *key, weight: 1.0 }]));
        }
        let primary = topology.locate_point(point, level)?;
        let stencil = topology.interpolation_stencil(point, level, mode)?;
        Ok((primary, stencil))
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
                    CanonicalBlobView::decode(canonical).map_err(|_| SampleError::InvalidRaster)?;
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
                Ok((decode_raw(blob.dtype, blob.payload, pixel)?, StoredSource::Blob))
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
    let Some(dtype) = crate::body::field_dtype(field) else {
        return Err(SampleError::UnsupportedField);
    };
    if !(field.semantic.starts_with("scalar.") || is_discrete_semantic(&field.semantic))
        || (is_discrete_semantic(&field.semantic) && dtype == DType::F32)
    {
        return Err(SampleError::UnsupportedField);
    }
    Ok(())
}

pub(crate) fn validate_derived_view_field(
    operator: &str,
    field: &FieldDescriptor,
    domain: &crate::body::Domain,
) -> Result<(), SampleError> {
    ensure_supported_field(field)?;
    let dtype = crate::body::field_dtype(field).ok_or(SampleError::UnsupportedField)?;
    crate::body::validate_decimal_storage(&field.storage)
        .map_err(|_| SampleError::UnsupportedField)?;
    if crate::body::field_nodata(field)
        .map_err(|_| SampleError::UnsupportedField)?
        .is_some_and(|raw| !crate::body::raw_storage_value_fits(dtype, raw))
    {
        return Err(SampleError::UnsupportedField);
    }
    decode_physical(field, RawValue::Integer(0))?;
    match operator {
        "core.slope/1"
            if domain.topology == "veyra.topo.dir_cube/1"
                && field.semantic == "scalar.height"
                && field.extra.get("unit").and_then(Value::as_str) == Some("m") =>
        {
            Ok(())
        }
        "core.threshold_partition/1"
            if domain.topology == "veyra.topo.dir_cube/1"
                && field.semantic == "scalar.height"
                && field.extra.get("unit").and_then(Value::as_str) == Some("m") =>
        {
            Ok(())
        }
        "core.slope/1" if domain.topology != "veyra.topo.dir_cube/1" => {
            Err(SampleError::UnsupportedSelection)
        }
        "core.slope/1" => Err(SampleError::UnsupportedField),
        "core.threshold_partition/1" if domain.topology != "veyra.topo.dir_cube/1" => {
            Err(SampleError::UnsupportedSelection)
        }
        "core.threshold_partition/1" => Err(SampleError::UnsupportedField),
        _ => Err(SampleError::UnsupportedSelection),
    }
}

fn is_discrete_semantic(semantic: &str) -> bool {
    matches!(semantic, "category" | "feature_ref" | "flags")
}

fn ensure_discrete_reduction(
    field: &FieldDescriptor,
    reduction: Reduction,
) -> Result<(), SampleError> {
    if is_discrete_semantic(&field.semantic) && reduction != Reduction::Single {
        return Err(SampleError::UnsupportedSelection);
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

fn materialized_tile_cell_count(dim_i: u64, dim_j: u64) -> Result<usize, SampleError> {
    if dim_i == 0 || dim_j == 0 {
        return Err(SampleError::InvalidRaster);
    }
    let dim_i = usize::try_from(dim_i).map_err(|_| SampleError::TileOutputLimitExceeded)?;
    let dim_j = usize::try_from(dim_j).map_err(|_| SampleError::TileOutputLimitExceeded)?;
    let cells = dim_i.checked_mul(dim_j).ok_or(SampleError::TileOutputLimitExceeded)?;
    let bytes =
        cells.checked_mul(size_of::<Option<f64>>()).ok_or(SampleError::TileOutputLimitExceeded)?;
    if bytes > MAX_MATERIALIZED_TILE_BYTES {
        return Err(SampleError::TileOutputLimitExceeded);
    }
    Ok(cells)
}

fn is_discrete_tile_output(
    field: &FieldDescriptor,
    view: Option<&crate::views::ViewDescriptor>,
) -> bool {
    is_discrete_semantic(&field.semantic)
        || view.and_then(|descriptor| descriptor.operator.as_deref()).is_some_and(|operator| {
            is_discrete_topology_view(operator) || operator == "core.threshold_partition/1"
        })
}

fn is_discrete_topology_view(operator: &str) -> bool {
    matches!(operator, "topology.cube_face" | "topology.tile_level" | "topology.axial_latitude")
}

fn topology_derived_value(
    operator: &str,
    topology: &dyn Topology,
    cell: CellKey,
    level: u8,
) -> Result<f64, SampleError> {
    match operator {
        "topology.cube_face" => {
            let (face, _, _, _) = DirCube::decode(cell)?;
            Ok(f64::from(face))
        }
        "topology.tile_level" => Ok(f64::from(level)),
        "topology.axial_latitude" => {
            let TopologyPoint::Direction(direction) = topology.point_for_cell(cell)? else {
                return Err(SampleError::UnsupportedSelection);
            };
            let (latitude, _) = direction.axial_lat_lon();
            Ok(libm::floor(
                (latitude + core::f64::consts::FRAC_PI_2) / (core::f64::consts::PI / 12.0),
            )
            .clamp(0.0, 11.0))
        }
        "topology.radial_profile" => match topology.point_for_cell(cell)? {
            TopologyPoint::RadialFraction(fraction) => Ok(fraction),
            TopologyPoint::Direction(_) => Err(SampleError::UnsupportedSelection),
        },
        _ => Err(SampleError::UnsupportedSelection),
    }
}

// V1 defines a mean for corner contributors, but category-like codes have no
// arithmetic mean; equal contributors remain representable and unequal ones refuse.
fn merge_discrete_corner_value(
    previous: Option<f64>,
    next: f64,
) -> Result<Option<f64>, SampleError> {
    if !next.is_finite() {
        return Err(SampleError::InvalidScale);
    }
    match previous {
        None => Ok(Some(next)),
        Some(previous) if previous == next => Ok(Some(previous)),
        Some(_) => Err(SampleError::UnsupportedDiscreteCornerHalo),
    }
}

fn merge_contributing_source(
    previous: Option<SampleSource>,
    value: Option<f64>,
    next: SampleSource,
) -> Option<SampleSource> {
    if value.is_none() {
        return previous;
    }
    match previous {
        None => Some(next),
        Some(previous) if previous == next => Some(previous),
        Some(_) => Some(SampleSource::Mixed),
    }
}

fn stable_weighted_mean(values: &[WeightedCell]) -> Result<Option<f64>, SampleError> {
    let mut total_measure = 0.0;
    let mut mean: Option<f64> = None;
    for cell in values {
        let Some(value) = cell.value else { continue };
        if !value.is_finite() || !cell.measure.is_finite() || cell.measure <= 0.0 {
            return Err(SampleError::InvalidScale);
        }
        let next_measure = total_measure + cell.measure;
        if !next_measure.is_finite() || next_measure <= 0.0 {
            return Err(SampleError::InvalidScale);
        }
        mean = Some(match mean {
            None => value,
            Some(previous) => {
                let share = cell.measure / next_measure;
                let updated =
                    if (previous >= 0.0 && value >= 0.0) || (previous <= 0.0 && value <= 0.0) {
                        previous + (value - previous) * share
                    } else {
                        previous * (1.0 - share) + value * share
                    };
                if !updated.is_finite() {
                    return Err(SampleError::InvalidScale);
                }
                updated
            }
        });
        total_measure = next_measure;
    }
    Ok(mean)
}

fn numeric_histogram_edge(
    minimum: f64,
    maximum: f64,
    range: f64,
    width: f64,
    bins: usize,
    edge: usize,
) -> f64 {
    if edge == 0 {
        return minimum;
    }
    if edge == bins {
        return maximum;
    }
    if range.is_finite() {
        return minimum + edge as f64 * width;
    }

    let scale = (-minimum).max(maximum);
    let fraction = edge as f64 / bins as f64;
    let normalized = (minimum / scale) * (1.0 - fraction) + (maximum / scale) * fraction;
    normalized * scale
}

fn numeric_histogram_index(value: f64, minimum: f64, maximum: f64, bins: &[HistogramBin]) -> usize {
    if value <= minimum {
        return 0;
    }
    if value >= maximum {
        return bins.len() - 1;
    }
    // Compare against the represented edges so exact internal boundaries always belong to the
    // following bin, independent of quotient rounding or overflow in the full numeric range.
    bins.partition_point(|bin| bin.upper <= value).min(bins.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::{
        LevelSel, Position, SampleQuery, TimeSel, angular_distance, is_discrete_tile_output,
        materialized_tile_cell_count, merge_contributing_source, merge_discrete_corner_value,
        slope_from_cardinal_samples, stable_weighted_mean, temporal_mean,
    };
    use crate::body::{FieldDescriptor, FieldId};
    use crate::canon::blob::{BlobKind, CanonicalBlob, DType};
    use crate::canon::hash;
    use crate::canon::index::{IndexBlob, IndexEntry, IndexValue, TopologyTag};
    use crate::ids::Hash32;
    use crate::io::{Body, Need};
    use crate::spatial::{Dir, DirCube, Radial1d, TileKey, Topology};
    use serde_json::json;

    fn weighted_cell(value: Option<f64>, measure: f64) -> super::WeightedCell {
        super::WeightedCell { value, raw: None, measure }
    }

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

        let raster_tile = body
            .tile(&super::TileRequest {
                field: field_id,
                key: tile_key,
                time: TimeSel::Static,
                halo: 0,
                view: super::TileView::Raw,
            })
            .unwrap();
        assert_eq!(raster_tile.values, vec![Some(42.0)]);
        let ordinary_stats = body.stats(field_id, LevelSel::Exact(0), TimeSel::Static).unwrap();
        assert_eq!(ordinary_stats.mean, Some(42.0));
        body.indexes.get_mut(&field_id).unwrap().entries[0].value = IndexValue::Const(1);
        body.registry.fields[0].storage["scale"] = json!("1e308");
        let extreme_stats = body.stats(field_id, LevelSel::Exact(0), TimeSel::Static).unwrap();
        assert_eq!(extreme_stats.mean, Some(1.0e308));
        assert_eq!(extreme_stats.minimum, Some(1.0e308));
        assert_eq!(extreme_stats.maximum, Some(1.0e308));
        body.registry.fields[0].storage["nodata"] = json!(1);
        let nodata_stats = body.stats(field_id, LevelSel::Exact(0), TimeSel::Static).unwrap();
        assert_eq!(nodata_stats.nodata_cells, 1);
        assert_eq!(nodata_stats.valid_measure, 0.0);
        assert_eq!(nodata_stats.mean, None);

        body.registry.fields[0].storage.as_object_mut().unwrap().remove("nodata");
        body.registry.fields[0].storage["scale"] = json!("1");
        body.indexes.get_mut(&field_id).unwrap().entries = (0..6_u8)
            .map(|face| IndexEntry {
                level: 0,
                key: DirCube::key(face, 0, 0, 0).unwrap().0,
                value: if face == 5 { IndexValue::Const(7) } else { IndexValue::Blob(blob_hash) },
            })
            .collect();
        let blob_first_tile = body
            .tile(&super::TileRequest {
                field: field_id,
                key: DirCube.tile_key(DirCube::key(0, 0, 0, 0).unwrap(), 0).unwrap(),
                time: TimeSel::Static,
                halo: 1,
                view: super::TileView::Raw,
            })
            .unwrap();
        let const_first_tile = body
            .tile(&super::TileRequest {
                field: field_id,
                key: DirCube.tile_key(DirCube::key(5, 0, 0, 0).unwrap(), 0).unwrap(),
                time: TimeSel::Static,
                halo: 1,
                view: super::TileView::Raw,
            })
            .unwrap();
        assert_eq!(blob_first_tile.source, super::SampleSource::Mixed);
        assert_eq!(const_first_tile.source, super::SampleSource::Mixed);
    }

    fn fieldless_topology_body(topology: &str) -> Body {
        let topology_id = match topology {
            "dir_cube" => "veyra.topo.dir_cube/1",
            "radial_1d" => "veyra.topo.radial_1d/1",
            _ => panic!("unknown topology fixture"),
        };
        let vertical = if topology == "dir_cube" {
            json!({"kind":"none"})
        } else {
            json!({"kind":"radius","extent_m":"1"})
        };
        let mut root_value: serde_json::Value = serde_json::from_str(include_str!(
            "../../../conformance/worlds/cb9-minimal-void/body.json"
        ))
        .unwrap();
        root_value["required_features"].as_array_mut().unwrap().push(json!(topology_id));
        root_value["domains"] = json!([{
            "id":"empty-domain","topology":topology_id,"frame":"body_fixed",
            "vertical":vertical,"tile_log2":0,"max_level":2
        }]);
        let root = serde_json::from_value(root_value.clone()).unwrap();
        let registry = serde_json::from_value(json!({
            "schema":"veyra.field_registry/1","fields":[]
        }))
        .unwrap();
        let body = Body {
            root,
            root_value,
            baseline_id: Hash32([0; 32]),
            registry,
            sections: std::collections::BTreeMap::new(),
            indexes: std::collections::BTreeMap::new(),
            blobs: std::collections::BTreeMap::new(),
            ledgers: std::collections::BTreeMap::new(),
        };
        assert!(body.root.validate().is_ok());
        assert!(body.root.validate_registry(&body.registry).is_ok());
        body
    }

    #[test]
    fn fieldless_topology_views_are_addressable_by_domain() {
        let cube = fieldless_topology_body("dir_cube");
        assert!(cube.fields().is_empty());
        for view in ["topology.cube_face", "topology.tile_level", "topology.axial_latitude"] {
            assert!(cube.views().iter().any(|descriptor| descriptor.id == view));
        }
        let cube_cell = DirCube::key(4, 0, 0, 1).unwrap();
        let cube_tile = DirCube.tile_key(cube_cell, 0).unwrap();
        for (view, expected) in [("topology.cube_face", 4.0), ("topology.tile_level", 1.0)] {
            let tile = cube
                .topology_tile(&super::TopologyTileRequest {
                    domain: "empty-domain".to_owned(),
                    key: cube_tile,
                    halo: 0,
                    view: view.to_owned(),
                })
                .unwrap();
            assert_eq!(tile.values, vec![Some(expected)]);
            assert_eq!(tile.source, super::SampleSource::Derived);
        }
        let latitude = cube
            .topology_tile(&super::TopologyTileRequest {
                domain: "empty-domain".to_owned(),
                key: cube_tile,
                halo: 0,
                view: "topology.axial_latitude".to_owned(),
            })
            .unwrap();
        assert!(latitude.values[0].is_some_and(|value| (0.0..=11.0).contains(&value)));

        let radial = fieldless_topology_body("radial_1d");
        assert!(radial.fields().is_empty());
        assert!(radial.views().iter().any(|view| view.id == "topology.radial_profile"));
        let radial_tile = Radial1d::default().tile_key(Radial1d::key(1, 1).unwrap(), 0).unwrap();
        let profile = radial
            .topology_tile(&super::TopologyTileRequest {
                domain: "empty-domain".to_owned(),
                key: radial_tile,
                halo: 0,
                view: "topology.radial_profile".to_owned(),
            })
            .unwrap();
        assert_eq!(profile.values, vec![Some(0.75)]);
        assert_eq!(profile.source, super::SampleSource::Derived);
    }

    fn radial_const_nodata_body(
        const_shell: u64,
        slice_count: u16,
        semantic: &str,
        reduce_default: &str,
    ) -> (Body, FieldId) {
        let field_id = FieldId::new(0x7ffe, 1);
        let raster = CanonicalBlob::new(
            BlobKind::RasterTile,
            DType::U8,
            1,
            1,
            slice_count,
            vec![255; usize::from(slice_count)],
        )
        .unwrap()
        .encode();
        let raster_hash = hash::hash(&raster);
        let entries = (0..2_u64)
            .map(|shell| IndexEntry {
                level: 1,
                key: crate::spatial::Radial1d::key(1, shell).unwrap().0,
                value: if shell == const_shell {
                    IndexValue::Const(5)
                } else {
                    IndexValue::Blob(raster_hash)
                },
            })
            .collect();
        let index = IndexBlob {
            field_id: field_id.0,
            topology: TopologyTag::Radial1d,
            tile_log2: 0,
            entries,
        };
        let index_hash = hash::hash(&index.encode().unwrap());
        let mut root_value: serde_json::Value = serde_json::from_str(include_str!(
            "../../../conformance/worlds/cb9-minimal-void/body.json"
        ))
        .unwrap();
        root_value["required_features"]
            .as_array_mut()
            .unwrap()
            .push(json!("veyra.topo.radial_1d/1"));
        root_value["domains"] = json!([{
            "id":"interior","topology":"veyra.topo.radial_1d/1","frame":"body_fixed",
            "vertical":{"kind":"radius","extent_m":"1"},"tile_log2":0,"max_level":1
        }]);
        root_value["capabilities"] =
            json!([{"id":"veyra.cap.conformance_probe/1","params":{},"compat":"ancillary"}]);
        root_value["indexes"] = json!({field_id.to_string():index_hash.to_string()});
        let root = serde_json::from_value(root_value.clone()).unwrap();
        let temporal = if slice_count == 1 {
            json!({"kind":"static"})
        } else {
            json!({
                "kind":"periodic_slices","count":slice_count,
                "period_ref":"rotation.period","origin_ref":"rotation.epoch",
                "reduce_default":reduce_default
            })
        };
        let registry = serde_json::from_value(json!({
            "schema":"veyra.field_registry/1",
            "fields":[{
                "id":field_id.to_string(),"name":"scalar.value",
                "capability":"veyra.cap.conformance_probe/1","domain":"interior",
                "semantic":semantic,"persistence":"invariant","unit":"K",
                "storage":{"dtype":"u8","scale":"1","offset":"0","nodata":255},
                "native_level":1,"temporal":temporal,
                "sampling":{"interp":"linear"},"downsample":"mean","compat":"ancillary"
            }]
        }))
        .unwrap();
        (
            Body {
                root,
                root_value,
                baseline_id: Hash32([0; 32]),
                registry,
                sections: std::collections::BTreeMap::new(),
                indexes: std::collections::BTreeMap::from([(field_id, index)]),
                blobs: std::collections::BTreeMap::from([(raster_hash, raster)]),
                ledgers: std::collections::BTreeMap::new(),
            },
            field_id,
        )
    }

    fn radial_scalar_values_body(values: [i64; 2], scale: &str) -> (Body, FieldId) {
        let (mut body, field) = radial_const_nodata_body(0, 1, "scalar.value", "mean");
        body.registry.fields[0].storage["dtype"] = json!("i8");
        body.registry.fields[0].storage["scale"] = json!(scale);
        body.registry.fields[0].storage["offset"] = json!("0");
        body.registry.fields[0].storage["nodata"] = serde_json::Value::Null;
        let index = body.indexes.get_mut(&field).unwrap();
        index.entries[0].value = IndexValue::Const(values[0]);
        index.entries[1].value = IndexValue::Const(values[1]);
        (body, field)
    }

    fn replace_radial_entry_with_values(
        body: &mut Body,
        field: FieldId,
        shell: u64,
        values: &[u8],
    ) {
        let bytes = CanonicalBlob::new(
            BlobKind::RasterTile,
            DType::U8,
            1,
            1,
            u16::try_from(values.len()).unwrap(),
            values.to_vec(),
        )
        .unwrap()
        .encode();
        let hash = hash::hash(&bytes);
        body.blobs.insert(hash, bytes);
        let entry = body
            .indexes
            .get_mut(&field)
            .unwrap()
            .entries
            .iter_mut()
            .find(|entry| entry.key == Radial1d::key(1, shell).unwrap().0)
            .unwrap();
        entry.value = IndexValue::Blob(hash);
    }

    fn radial_categories_with_nodata() -> (Body, FieldId) {
        let (mut body, field) = radial_const_nodata_body(0, 1, "category", "mean");
        body.root.domains[0].max_level = 2;
        body.registry.fields[0].native_level = 2;
        let nodata = CanonicalBlob::new(BlobKind::RasterTile, DType::U8, 1, 1, 1, vec![255])
            .unwrap()
            .encode();
        let nodata_hash = hash::hash(&nodata);
        body.blobs.clear();
        body.blobs.insert(nodata_hash, nodata);
        body.indexes.get_mut(&field).unwrap().entries = vec![
            IndexEntry {
                level: 2,
                key: Radial1d::key(2, 0).unwrap().0,
                value: IndexValue::Const(100),
            },
            IndexEntry {
                level: 2,
                key: Radial1d::key(2, 1).unwrap().0,
                value: IndexValue::Const(1),
            },
            IndexEntry {
                level: 2,
                key: Radial1d::key(2, 2).unwrap().0,
                value: IndexValue::Const(2),
            },
            IndexEntry {
                level: 2,
                key: Radial1d::key(2, 3).unwrap().0,
                value: IndexValue::Blob(nodata_hash),
            },
        ];
        (body, field)
    }

    fn cube_constant_body(tile_log2: u8) -> (Body, FieldId) {
        let (mut body, field) = radial_const_nodata_body(0, 1, "scalar.value", "mean");
        let domain = &mut body.root.domains[0];
        domain.topology = "veyra.topo.dir_cube/1".to_owned();
        domain.tile_log2 = tile_log2;
        domain.max_level = tile_log2;
        domain.vertical = json!({"kind":"none"});
        body.root.required_features.retain(|feature| feature != "veyra.topo.radial_1d/1");
        if !body.root.required_features.iter().any(|feature| feature == "veyra.topo.dir_cube/1") {
            body.root.required_features.push("veyra.topo.dir_cube/1".to_owned());
        }
        body.registry.fields[0].native_level = tile_log2;
        body.registry.fields[0].sampling["interp"] = json!("nearest");
        let index = body.indexes.get_mut(&field).unwrap();
        index.topology = TopologyTag::DirCube;
        index.tile_log2 = tile_log2;
        index.entries = (0..6_u8)
            .map(|face| {
                let cell = DirCube::key(face, 0, 0, tile_log2).unwrap();
                let tile = DirCube.tile_key(cell, tile_log2).unwrap();
                IndexEntry { level: tile.level, key: tile.address.0, value: IndexValue::Const(5) }
            })
            .collect();
        let index_hash = hash::hash(&index.encode().unwrap());
        body.root.indexes.insert(field.to_string(), index_hash.to_string());
        body.root_value["required_features"] = json!(body.root.required_features);
        body.root_value["domains"] = json!(body.root.domains);
        body.root_value["indexes"][field.to_string()] = json!(index_hash.to_string());
        (body, field)
    }

    fn cube_pos30_boundary_body() -> (Body, FieldId) {
        let (mut body, field) = cube_constant_body(30);
        body.root.domains[0].tile_log2 = 0;
        body.root_value["domains"][0]["tile_log2"] = json!(0);
        body.registry.fields[0].sampling["interp"] = json!("bilinear");
        let lower = (1_u64 << 29) - 1;
        let upper = 1_u64 << 29;
        let index = body.indexes.get_mut(&field).unwrap();
        index.tile_log2 = 0;
        index.entries =
            [(lower, lower, 0), (upper, lower, 10), (lower, upper, 20), (upper, upper, 30)]
                .into_iter()
                .map(|(i, j, value)| IndexEntry {
                    level: 30,
                    key: DirCube::key(0, i, j, 30).unwrap().0,
                    value: IndexValue::Const(value),
                })
                .collect();
        index.entries.sort_by_key(|entry| (entry.level, entry.key));
        let index_hash = hash::hash(&index.encode().unwrap());
        body.root.indexes.insert(field.to_string(), index_hash.to_string());
        body.root_value["indexes"][field.to_string()] = json!(index_hash.to_string());
        (body, field)
    }

    fn cube_face_value_body(tile_log2: u8) -> (Body, FieldId) {
        let (mut body, field) = cube_constant_body(tile_log2);
        let index = body.indexes.get_mut(&field).unwrap();
        for entry in &mut index.entries {
            let (face, _, _, _) = DirCube::decode(crate::spatial::CellKey(entry.key)).unwrap();
            entry.value = IndexValue::Const(i64::from(face) + 1);
        }
        let index_hash = hash::hash(&index.encode().unwrap());
        body.root.indexes.insert(field.to_string(), index_hash.to_string());
        body.root_value["indexes"][field.to_string()] = json!(index_hash.to_string());
        (body, field)
    }

    fn cube_bilinear_cell_values_body() -> (Body, FieldId) {
        let (mut body, field) = cube_constant_body(0);
        body.root.domains[0].max_level = 2;
        body.root_value["domains"] = json!(body.root.domains);
        body.registry.fields[0].native_level = 2;
        body.registry.fields[0].sampling["interp"] = json!("bilinear");
        let index = body.indexes.get_mut(&field).unwrap();
        index.tile_log2 = 0;
        index.entries = (0..6_u8)
            .flat_map(|face| {
                (0..4_u64).flat_map(move |i| {
                    (0..4_u64).map(move |j| IndexEntry {
                        level: 2,
                        key: DirCube::key(face, i, j, 2).unwrap().0,
                        value: IndexValue::Const(i64::from(face) * 16 + (i * 4 + j) as i64 + 1),
                    })
                })
            })
            .collect();
        index.entries.sort_by_key(|entry| (entry.level, entry.key));
        let index_hash = hash::hash(&index.encode().unwrap());
        body.root.indexes.insert(field.to_string(), index_hash.to_string());
        body.root_value["indexes"][field.to_string()] = json!(index_hash.to_string());
        (body, field)
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

    #[test]
    fn tile_materialization_limit_uses_checked_target_sized_arithmetic() {
        assert_eq!(materialized_tile_cell_count(128, 128).unwrap(), 128 * 128);
        assert!(matches!(
            materialized_tile_cell_count(0, 1),
            Err(super::SampleError::InvalidRaster)
        ));
        assert!(matches!(
            materialized_tile_cell_count(u64::MAX, u64::MAX),
            Err(super::SampleError::TileOutputLimitExceeded)
        ));
    }

    #[test]
    fn tile_materialization_rejects_massive_valid_requests_and_keeps_production_tiles() {
        let (production_body, field) = cube_constant_body(7);
        let production_key = DirCube.tile_key(DirCube::key(0, 0, 0, 7).unwrap(), 7).unwrap();
        let production = production_body
            .tile(&super::TileRequest {
                field,
                key: production_key,
                time: TimeSel::Static,
                halo: 0,
                view: super::TileView::Raw,
            })
            .unwrap();
        assert_eq!((production.dim_i, production.dim_j), (128, 128));
        assert_eq!(production.values.len(), 128 * 128);
        assert!(production.values.iter().all(|value| *value == Some(5.0)));

        let production_halo = production_body
            .tile(&super::TileRequest {
                field,
                key: production_key,
                time: TimeSel::Static,
                halo: 1,
                view: super::TileView::Raw,
            })
            .unwrap();
        assert_eq!((production_halo.dim_i, production_halo.dim_j), (130, 130));
        assert_eq!(production_halo.values.len(), 130 * 130);
        assert!(production_halo.values.iter().all(|value| *value == Some(5.0)));

        for tile_log2 in [12, 30] {
            let (body, field) = cube_constant_body(tile_log2);
            assert!(body.root.validate().is_ok());
            assert!(body.root.validate_registry(&body.registry).is_ok());
            let key =
                DirCube.tile_key(DirCube::key(0, 0, 0, tile_log2).unwrap(), tile_log2).unwrap();
            let sample = body
                .sample(&SampleQuery {
                    field,
                    pos: Position::Cell {
                        domain: "interior".to_owned(),
                        key: DirCube::key(0, 0, 0, tile_log2).unwrap(),
                    },
                    level: LevelSel::Exact(tile_log2),
                    time: TimeSel::Static,
                })
                .unwrap();
            assert_eq!(sample.value, Some(5.0));
            for halo in [0, 1] {
                assert!(matches!(
                    body.tile(&super::TileRequest {
                        field,
                        key,
                        time: TimeSel::Static,
                        halo,
                        view: super::TileView::Raw,
                    }),
                    Err(super::SampleError::TileOutputLimitExceeded)
                ));
            }
        }
    }

    #[test]
    fn sample_reports_the_containing_cell_for_bilinear_stencils() {
        let (mut body, field) = cube_face_value_body(2);
        body.registry.fields[0].sampling["interp"] = json!("bilinear");
        let edge = Dir::new(1.0, 1.0, 0.0).unwrap();
        assert_eq!(DirCube.locate(edge, 2).unwrap(), crate::spatial::CellKey(0x1d00000000000000));
        let interior = DirCube.direction_at_face_st(0, 0.37, 0.61).unwrap();
        for direction in [edge, interior] {
            let expected_cell = DirCube.locate(direction, 2).unwrap();
            let sample = body
                .sample(&SampleQuery {
                    field,
                    pos: Position::Direction(direction),
                    level: LevelSel::Exact(2),
                    time: TimeSel::Static,
                })
                .unwrap();
            assert_eq!(sample.cell, expected_cell);
            assert_eq!(sample.raw, None);
            assert_eq!(sample.category, None);

            let report = body
                .inspect(&Position::Direction(direction), LevelSel::Exact(2), TimeSel::Static)
                .unwrap();
            assert_eq!(report.fields[0].sample.as_ref().unwrap().cell, expected_cell);
        }

        let mut discrete_body = cube_face_value_body(2).0;
        discrete_body.registry.fields[0].semantic = "category".to_owned();
        discrete_body.registry.fields[0].sampling["interp"] = json!("bilinear");
        let discrete_sample = discrete_body
            .sample(&SampleQuery {
                field,
                pos: Position::Direction(edge),
                level: LevelSel::Exact(2),
                time: TimeSel::Static,
            })
            .unwrap();
        assert_eq!(discrete_sample.cell, DirCube.locate(edge, 2).unwrap());
        assert_eq!(discrete_sample.raw, Some(super::RawValue::Integer(1)));
        assert_eq!(discrete_sample.category, Some(1));
    }

    #[test]
    fn cell_center_bilinear_reads_preserve_raw_values_across_faces_and_stats() {
        let (body, field) = cube_bilinear_cell_values_body();
        let affected_key = DirCube::key(0, 0, 0, 2).unwrap();
        let roundtrip_stencil = DirCube
            .interpolation_stencil(
                crate::spatial::TopologyPoint::Direction(
                    DirCube.cell_center(affected_key).unwrap(),
                ),
                2,
                crate::spatial::InterpolationMode::Bilinear,
            )
            .unwrap();
        assert_eq!(roundtrip_stencil.len(), 4);
        assert!(roundtrip_stencil.iter().any(|cell| {
            cell.key != affected_key && cell.weight > 0.0 && cell.weight < 1.0e-14
        }));
        let (legacy_weighted_value, legacy_weight) =
            roundtrip_stencil.iter().fold((0.0, 0.0), |(sum, weight), cell| {
                let (face, i, j, _) = DirCube::decode(cell.key).unwrap();
                let raw = i64::from(face) * 16 + (i * 4 + j) as i64 + 1;
                (sum + raw as f64 * cell.weight, weight + cell.weight)
            });
        assert_ne!(legacy_weighted_value / legacy_weight, 1.0);
        let mut expected_weighted_sum = 0.0;
        let mut total_measure = 0.0;
        for face in 0..6_u8 {
            for i in 0..4_u64 {
                for j in 0..4_u64 {
                    let key = DirCube::key(face, i, j, 2).unwrap();
                    let raw = i64::from(face) * 16 + (i * 4 + j) as i64 + 1;
                    let query = SampleQuery {
                        field,
                        pos: Position::Cell { domain: "interior".to_owned(), key },
                        level: LevelSel::Exact(2),
                        time: TimeSel::Static,
                    };
                    assert!(body.plan(&query).unwrap().is_empty());
                    let sample = body.sample(&query).unwrap();
                    assert_eq!(sample.cell, key);
                    assert_eq!(sample.raw, Some(super::RawValue::Integer(raw)));
                    assert_eq!(sample.value, Some(raw as f64));

                    let report =
                        body.inspect(&query.pos, LevelSel::Exact(2), TimeSel::Static).unwrap();
                    assert_eq!(report.fields[0].sample.as_ref().unwrap().raw, sample.raw);

                    let tile_key = DirCube.tile_key(key, 0).unwrap();
                    let tile = body
                        .tile(&super::TileRequest {
                            field,
                            key: tile_key,
                            time: TimeSel::Static,
                            halo: 0,
                            view: super::TileView::Raw,
                        })
                        .unwrap();
                    assert_eq!(tile.values, vec![Some(raw as f64)]);

                    let measure = DirCube.cell_measure(key).unwrap();
                    expected_weighted_sum += raw as f64 * measure;
                    total_measure += measure;
                }
            }
        }
        let stats = body.stats(field, LevelSel::Exact(2), TimeSel::Static).unwrap();
        assert_eq!(stats.cells, 96);
        assert_eq!(stats.minimum, Some(1.0));
        assert_eq!(stats.maximum, Some(96.0));
        assert!((stats.mean.unwrap() - expected_weighted_sum / total_measure).abs() < 1.0e-12);

        let off_center = DirCube.direction_at_face_st(0, 0.5, 0.375).unwrap();
        let mixed = body
            .sample(&SampleQuery {
                field,
                pos: Position::Direction(off_center),
                level: LevelSel::Exact(2),
                time: TimeSel::Static,
            })
            .unwrap();
        assert!((mixed.value.unwrap() - 8.0).abs() < 1.0e-12);
        assert_eq!(mixed.raw, None);
    }

    #[test]
    fn pos30_sampling_preserves_chart_boundaries_interior_and_face_edges() {
        let (body, field) = cube_pos30_boundary_body();
        assert!(body.root.validate().is_ok());
        assert!(body.root.validate_registry(&body.registry).is_ok());
        let boundary = super::Pos30 { face: 0, i30: 1 << 29, j30: 1 << 29 };
        let sample = body
            .sample(&SampleQuery {
                field,
                pos: Position::Pos30(boundary),
                level: LevelSel::Exact(30),
                time: TimeSel::Static,
            })
            .unwrap();
        assert_eq!(sample.value, Some(15.0));

        let (body, field) = cube_face_value_body(30);
        let coordinate_count = f64::from(1_u32 << 30);
        for position in [
            super::Pos30 { face: 0, i30: 123_456_789, j30: 876_543_210 },
            super::Pos30 { face: 0, i30: 0, j30: 1 << 29 },
            super::Pos30 { face: 2, i30: 1 << 29, j30: (1 << 30) - 1 },
        ] {
            let direction = DirCube
                .direction_at_face_st(
                    position.face,
                    f64::from(position.i30) / coordinate_count,
                    f64::from(position.j30) / coordinate_count,
                )
                .unwrap();
            let from_pos30 = body
                .sample(&SampleQuery {
                    field,
                    pos: Position::Pos30(position),
                    level: LevelSel::Exact(30),
                    time: TimeSel::Static,
                })
                .unwrap();
            let from_direction = body
                .sample(&SampleQuery {
                    field,
                    pos: Position::Direction(direction),
                    level: LevelSel::Exact(30),
                    time: TimeSel::Static,
                })
                .unwrap();
            assert_eq!(from_pos30.value, from_direction.value);
            assert_eq!(from_pos30.cell, from_direction.cell);
        }

        for invalid in [
            super::Pos30 { face: 6, i30: 0, j30: 0 },
            super::Pos30 { face: 0, i30: 1 << 30, j30: 0 },
            super::Pos30 { face: 0, i30: 0, j30: 1 << 30 },
        ] {
            assert!(matches!(
                body.sample(&SampleQuery {
                    field,
                    pos: Position::Pos30(invalid),
                    level: LevelSel::Exact(30),
                    time: TimeSel::Static,
                }),
                Err(super::SampleError::InvalidPosition)
            ));
        }
    }

    #[test]
    fn nodata_does_not_change_constant_sample_provenance_in_either_stencil_order() {
        for const_shell in [0, 1] {
            let (body, field) = radial_const_nodata_body(const_shell, 1, "scalar.value", "mean");
            let sample = body
                .sample(&SampleQuery {
                    field,
                    pos: Position::Radial { r_m: 0.5 },
                    level: LevelSel::Exact(1),
                    time: TimeSel::Static,
                })
                .unwrap();
            assert_eq!(sample.value, Some(5.0));
            assert_eq!(sample.source, super::SampleSource::Const);

            let (body, field) = radial_const_nodata_body(const_shell, 2, "scalar.value", "mean");
            let slice = body
                .sample(&SampleQuery {
                    field,
                    pos: Position::Radial { r_m: 0.5 },
                    level: LevelSel::Exact(1),
                    time: TimeSel::Slice(0),
                })
                .unwrap();
            assert_eq!(slice.value, Some(5.0));
            assert_eq!(slice.source, super::SampleSource::Const);
            let mean = body
                .sample(&SampleQuery {
                    field,
                    pos: Position::Radial { r_m: 0.5 },
                    level: LevelSel::Exact(1),
                    time: TimeSel::Mean,
                })
                .unwrap();
            assert_eq!(mean.value, Some(5.0));
            assert_eq!(mean.source, super::SampleSource::TimeReduced);
        }
    }

    #[test]
    fn discrete_periodic_semantics_allow_single_slice_queries_only() {
        let invalid_reductions = [TimeSel::Static, TimeSel::Mean, TimeSel::Min, TimeSel::Max];
        let single_slices = [TimeSel::Slice(0), TimeSel::Slice(1), TimeSel::Phase(0.75)];
        for semantic in ["category", "feature_ref", "flags"] {
            let (mut body, field) = radial_const_nodata_body(0, 2, semantic, "mean");
            replace_radial_entry_with_values(&mut body, field, 0, &[1, 2]);
            let query_position = Position::Radial { r_m: 0.25 };
            let tile_key = Radial1d::default().tile_key(Radial1d::key(1, 0).unwrap(), 0).unwrap();

            for time in invalid_reductions {
                let query = SampleQuery {
                    field,
                    pos: query_position.clone(),
                    level: LevelSel::Exact(1),
                    time,
                };
                assert!(matches!(body.plan(&query), Err(super::SampleError::UnsupportedSelection)));
                assert!(matches!(
                    body.sample(&query),
                    Err(super::SampleError::UnsupportedSelection)
                ));
                assert!(matches!(
                    body.stats(field, LevelSel::Exact(1), time),
                    Err(super::SampleError::UnsupportedSelection)
                ));
                assert!(matches!(
                    body.histogram(field, 2, LevelSel::Exact(1), time),
                    Err(super::SampleError::UnsupportedSelection)
                ));
                for view in [super::TileView::Raw, super::TileView::TimeReduce] {
                    assert!(matches!(
                        body.tile(&super::TileRequest {
                            field,
                            key: tile_key,
                            time,
                            halo: 0,
                            view: view.clone(),
                        }),
                        Err(super::SampleError::UnsupportedSelection)
                    ));
                }
                let report = body.inspect(&query_position, LevelSel::Exact(1), time).unwrap();
                assert!(report.fields[0].sample.is_none());
                assert!(report.fields[0].issue.is_some());
            }

            for (time, expected) in
                [(single_slices[0], 1.0), (single_slices[1], 2.0), (single_slices[2], 2.0)]
            {
                let query = SampleQuery {
                    field,
                    pos: query_position.clone(),
                    level: LevelSel::Exact(1),
                    time,
                };
                assert!(body.plan(&query).unwrap().is_empty());
                let sample = body.sample(&query).unwrap();
                assert_eq!(sample.value, Some(expected));
                assert_eq!(
                    sample.category,
                    matches!(semantic, "category" | "feature_ref").then_some(expected as i64)
                );
                assert_eq!(sample.raw, Some(super::RawValue::Integer(expected as i64)));
                let inspected = body.inspect(&query_position, LevelSel::Exact(1), time).unwrap();
                assert_eq!(inspected.fields[0].sample.as_ref(), Some(&sample));
                let tile = body
                    .tile(&super::TileRequest {
                        field,
                        key: tile_key,
                        time,
                        halo: 0,
                        view: super::TileView::Raw,
                    })
                    .unwrap();
                assert_eq!(tile.values, vec![Some(expected)]);
                assert_eq!(tile.source, super::SampleSource::Stored);
                let histogram = body.histogram(field, 2, LevelSel::Exact(1), time).unwrap();
                assert_eq!(histogram.minimum, None);
                assert_eq!(histogram.maximum, None);
                assert_eq!(histogram.bins.len(), 1);
                assert_eq!(histogram.bins[0].lower, expected);
                assert_eq!(histogram.bins[0].upper, expected);
                assert_eq!(histogram.bins[0].cells, 1);
            }

            assert!(matches!(
                body.stats(field, LevelSel::Exact(1), TimeSel::Slice(0)),
                Err(super::SampleError::UnsupportedSelection)
            ));

            let nodata_position = Position::Radial { r_m: 0.75 };
            let nodata = body
                .sample(&SampleQuery {
                    field,
                    pos: nodata_position.clone(),
                    level: LevelSel::Exact(1),
                    time: TimeSel::Slice(0),
                })
                .unwrap();
            assert_eq!(nodata.value, None);
            assert_eq!(nodata.raw, Some(super::RawValue::Integer(255)));
            assert_eq!(nodata.category, None);
            assert_eq!(nodata.source, super::SampleSource::Nodata);
            let inspected =
                body.inspect(&nodata_position, LevelSel::Exact(1), TimeSel::Slice(0)).unwrap();
            assert_eq!(inspected.fields[0].sample.as_ref(), Some(&nodata));
            let nodata_tile = body
                .tile(&super::TileRequest {
                    field,
                    key: Radial1d::default().tile_key(Radial1d::key(1, 1).unwrap(), 0).unwrap(),
                    time: TimeSel::Slice(0),
                    halo: 0,
                    view: super::TileView::Raw,
                })
                .unwrap();
            assert_eq!(nodata_tile.values, vec![None]);
            assert_eq!(nodata_tile.source, super::SampleSource::Nodata);

            let static_body = radial_const_nodata_body(0, 1, semantic, "mean").0;
            let static_position = Position::Radial { r_m: 0.25 };
            for time in [TimeSel::Static, TimeSel::Mean, TimeSel::Min, TimeSel::Max] {
                let query = SampleQuery {
                    field,
                    pos: static_position.clone(),
                    level: LevelSel::Exact(1),
                    time,
                };
                assert!(static_body.plan(&query).unwrap().is_empty());
                let sample = static_body.sample(&query).unwrap();
                assert_eq!(sample.value, Some(5.0));
                assert_eq!(
                    sample.category,
                    matches!(semantic, "category" | "feature_ref").then_some(5)
                );
                assert!(matches!(
                    static_body.stats(field, LevelSel::Exact(1), time),
                    Err(super::SampleError::UnsupportedSelection)
                ));
            }
            for default in ["min", "max"] {
                let (default_body, default_field) =
                    radial_const_nodata_body(0, 2, semantic, default);
                let query = SampleQuery {
                    field: default_field,
                    pos: static_position.clone(),
                    level: LevelSel::Exact(1),
                    time: TimeSel::Static,
                };
                assert!(matches!(
                    default_body.plan(&query),
                    Err(super::SampleError::UnsupportedSelection)
                ));
            }
        }

        let (category_body, category_field) = radial_categories_with_nodata();
        let histogram = category_body
            .histogram(category_field, 3, LevelSel::Exact(2), TimeSel::Static)
            .unwrap();
        assert_eq!(histogram.minimum, None);
        assert_eq!(histogram.maximum, None);
        assert_eq!(
            histogram.bins.iter().map(|bin| bin.lower).collect::<Vec<_>>(),
            vec![100.0, 1.0, 2.0]
        );
        assert!(histogram.bins.iter().all(|bin| bin.lower == bin.upper && bin.cells == 1));
        for (bin, shell) in histogram.bins.iter().zip(0..3) {
            assert_eq!(
                bin.weight,
                Radial1d::default().cell_measure(Radial1d::key(2, shell).unwrap()).unwrap()
            );
        }
        assert!(matches!(
            category_body.histogram(category_field, 2, LevelSel::Exact(2), TimeSel::Static),
            Err(super::SampleError::UnsupportedSelection)
        ));

        let (mut malformed_category, malformed_field) =
            radial_const_nodata_body(0, 1, "category", "mean");
        malformed_category.registry.fields[0].storage["dtype"] = json!("f32");
        let malformed_query = SampleQuery {
            field: malformed_field,
            pos: Position::Radial { r_m: 0.25 },
            level: LevelSel::Exact(1),
            time: TimeSel::Static,
        };
        assert!(matches!(
            malformed_category.sample(&malformed_query),
            Err(super::SampleError::UnsupportedField)
        ));
        assert!(matches!(
            malformed_category.histogram(malformed_field, 1, LevelSel::Exact(1), TimeSel::Static),
            Err(super::SampleError::UnsupportedField)
        ));
    }

    #[test]
    fn discrete_corner_values_are_preserved_or_refused_without_averaging() {
        let mut field: FieldDescriptor = serde_json::from_value(json!({
            "id":"0x7ffe0001","name":"discrete","capability":"veyra.cap.conformance_probe/1",
            "domain":"surface","semantic":"category","persistence":"invariant",
            "storage":{"dtype":"u8","scale":"1","offset":"0"},"native_level":0,
            "temporal":{"kind":"static"},"sampling":{"interp":"nearest"},
            "downsample":"mode_lowest_tiebreak","compat":"ancillary"
        }))
        .unwrap();
        for semantic in ["category", "feature_ref", "flags"] {
            field.semantic = semantic.to_owned();
            assert!(is_discrete_tile_output(&field, None));
        }
        field.semantic = "scalar.value".to_owned();
        let cube_face = crate::views::ViewDescriptor {
            id: "topology.cube_face".to_owned(),
            domain: "surface".to_owned(),
            field: None,
            field_name: None,
            required_fields: Vec::new(),
            capability: None,
            operator: Some("topology.cube_face".to_owned()),
            group: "Spatial".to_owned(),
            display_order: None,
            label: "Cube face".to_owned(),
            display: None,
        };
        assert!(is_discrete_tile_output(&field, Some(&cube_face)));

        assert_eq!(merge_discrete_corner_value(None, 2.0).unwrap(), Some(2.0));
        assert_eq!(merge_discrete_corner_value(Some(2.0), 2.0).unwrap(), Some(2.0));
        assert!(matches!(
            merge_discrete_corner_value(Some(2.0), 4.0),
            Err(super::SampleError::UnsupportedDiscreteCornerHalo)
        ));
    }

    #[test]
    fn scalar_derived_views_refuse_discrete_sources() {
        let (body, field_id) = radial_const_nodata_body(0, 1, "category", "mean");
        let mut field = body.registry.fields[0].clone();
        field.semantic = "category".to_owned();
        let domain: crate::body::Domain = serde_json::from_value(json!({
            "id":"surface","topology":"veyra.topo.dir_cube/1","frame":"body_fixed",
            "tile_log2":0,"max_level":0,"vertical":{"kind":"none"}
        }))
        .unwrap();
        let cell = DirCube::key(0, 0, 0, 0).unwrap();
        assert!(matches!(
            body.slope_at(&field, &domain, &DirCube, cell, 0, TimeSel::Static),
            Err(super::SampleError::UnsupportedField)
        ));
        let view = crate::views::ViewDescriptor {
            id: "derived.test".to_owned(),
            domain: "surface".to_owned(),
            field: Some(field_id),
            field_name: Some(field.name.clone()),
            required_fields: vec![field_id],
            capability: Some(field.capability.clone()),
            operator: Some("core.threshold_partition/1".to_owned()),
            group: "test".to_owned(),
            display_order: None,
            label: "test".to_owned(),
            display: None,
        };
        assert!(matches!(
            body.threshold_partition_at(&view, &field, &domain, cell, 0, TimeSel::Static),
            Err(super::SampleError::UnsupportedField)
        ));
    }

    #[test]
    fn tile_source_aggregation_is_order_independent_and_ignores_nodata() {
        let const_then_stored = merge_contributing_source(
            merge_contributing_source(None, Some(2.0), super::SampleSource::Const),
            Some(4.0),
            super::SampleSource::Stored,
        );
        let stored_then_const = merge_contributing_source(
            merge_contributing_source(None, Some(4.0), super::SampleSource::Stored),
            Some(2.0),
            super::SampleSource::Const,
        );
        assert_eq!(const_then_stored, Some(super::SampleSource::Mixed));
        assert_eq!(stored_then_const, const_then_stored);
        assert_eq!(
            merge_contributing_source(
                Some(super::SampleSource::Const),
                None,
                super::SampleSource::Nodata,
            ),
            Some(super::SampleSource::Const)
        );
        let const_then_reduced = merge_contributing_source(
            merge_contributing_source(None, Some(2.0), super::SampleSource::Const),
            Some(4.0),
            super::SampleSource::TimeReduced,
        );
        let reduced_then_const = merge_contributing_source(
            merge_contributing_source(None, Some(4.0), super::SampleSource::TimeReduced),
            Some(2.0),
            super::SampleSource::Const,
        );
        assert_eq!(const_then_reduced, Some(super::SampleSource::Mixed));
        assert_eq!(reduced_then_const, const_then_reduced);
        assert_eq!(
            merge_contributing_source(None, Some(10.0), super::SampleSource::TimeReduced,),
            Some(super::SampleSource::TimeReduced)
        );
        assert_eq!(
            merge_contributing_source(
                Some(super::SampleSource::TimeReduced),
                None,
                super::SampleSource::Nodata,
            ),
            Some(super::SampleSource::TimeReduced)
        );
    }

    #[test]
    fn weighted_mean_stays_finite_for_extreme_values_and_ignores_nodata() {
        let ordinary =
            stable_weighted_mean(&[weighted_cell(Some(1.0), 1.0), weighted_cell(Some(3.0), 3.0)])
                .unwrap();
        assert_eq!(ordinary, Some(2.5));

        let extreme = stable_weighted_mean(&[
            weighted_cell(Some(1.0e308), 1.0),
            weighted_cell(Some(1.0e308), 2.0),
        ])
        .unwrap();
        assert_eq!(extreme, Some(1.0e308));
        let mixed_signs = stable_weighted_mean(&[
            weighted_cell(Some(-1.0e308), 1.0),
            weighted_cell(Some(1.0e308), 1.0),
        ])
        .unwrap();
        assert_eq!(mixed_signs, Some(0.0));

        let with_nodata =
            stable_weighted_mean(&[weighted_cell(None, 100.0), weighted_cell(Some(7.0), 2.0)])
                .unwrap();
        assert_eq!(with_nodata, Some(7.0));
        assert_eq!(stable_weighted_mean(&[weighted_cell(None, 1.0)]).unwrap(), None);
    }

    #[test]
    fn numeric_histograms_handle_wide_nearby_negative_uniform_and_ordinary_ranges() {
        let (wide_body, wide_field) = radial_scalar_values_body([-1, 1], "1e308");
        let wide = wide_body.histogram(wide_field, 4, LevelSel::Exact(1), TimeSel::Static).unwrap();
        assert_eq!(wide.minimum, Some(-1.0e308));
        assert_eq!(wide.maximum, Some(1.0e308));
        assert_eq!(wide.bins.first().unwrap().lower, -1.0e308);
        assert_eq!(wide.bins.last().unwrap().upper, 1.0e308);
        assert_eq!(wide.bins[1].upper, 0.0);
        assert_eq!(wide.bins[2].lower, 0.0);
        assert_eq!(super::numeric_histogram_index(0.0, -1.0e308, 1.0e308, &wide.bins), 2);
        assert_eq!(super::numeric_histogram_index(-5.0e307, -1.0e308, 1.0e308, &wide.bins), 1);
        assert_eq!(wide.bins.iter().map(|bin| bin.cells).collect::<Vec<_>>(), vec![1, 0, 0, 1]);

        let (nearby_body, nearby_field) = radial_scalar_values_body([100, 101], "1");
        let nearby =
            nearby_body.histogram(nearby_field, 2, LevelSel::Exact(1), TimeSel::Static).unwrap();
        assert_eq!(nearby.bins[0].lower, 100.0);
        assert_eq!(nearby.bins[0].upper, 100.5);
        assert_eq!(nearby.bins[1].lower, 100.5);
        assert_eq!(nearby.bins[1].upper, 101.0);
        assert_eq!(nearby.bins.iter().map(|bin| bin.cells).collect::<Vec<_>>(), vec![1, 1]);

        let (negative_body, negative_field) = radial_scalar_values_body([-4, -2], "1");
        let negative = negative_body
            .histogram(negative_field, 2, LevelSel::Exact(1), TimeSel::Static)
            .unwrap();
        assert_eq!(negative.bins[0].lower, -4.0);
        assert_eq!(negative.bins[0].upper, -3.0);
        assert_eq!(negative.bins[1].lower, -3.0);
        assert_eq!(negative.bins[1].upper, -2.0);
        assert_eq!(negative.bins.iter().map(|bin| bin.cells).collect::<Vec<_>>(), vec![1, 1]);

        let (uniform_body, uniform_field) = radial_scalar_values_body([7, 7], "1");
        let uniform =
            uniform_body.histogram(uniform_field, 3, LevelSel::Exact(1), TimeSel::Static).unwrap();
        assert_eq!(uniform.minimum, Some(7.0));
        assert_eq!(uniform.maximum, Some(7.0));
        assert!(uniform.bins.iter().all(|bin| bin.lower == 7.0 && bin.upper == 7.0));
        assert_eq!(uniform.bins.iter().map(|bin| bin.cells).collect::<Vec<_>>(), vec![2, 0, 0]);

        let (ordinary_body, ordinary_field) = radial_scalar_values_body([0, 4], "1");
        let ordinary = ordinary_body
            .histogram(ordinary_field, 4, LevelSel::Exact(1), TimeSel::Static)
            .unwrap();
        assert_eq!(
            ordinary.bins.iter().map(|bin| bin.lower).collect::<Vec<_>>(),
            vec![0.0, 1.0, 2.0, 3.0]
        );
        assert_eq!(ordinary.bins.last().unwrap().upper, 4.0);
        assert_eq!(ordinary.bins.iter().map(|bin| bin.cells).sum::<u64>(), 2);
    }

    #[test]
    fn internal_histogram_edges_are_inclusive_in_the_following_bin() {
        let minimum = 0.1;
        let maximum = 0.4;
        let bins = 3;
        let range = maximum - minimum;
        let width = range / bins as f64;
        let edges: Vec<_> = (0..=bins)
            .map(|edge| super::numeric_histogram_edge(minimum, maximum, range, width, bins, edge))
            .collect();
        let histogram_bins: Vec<_> = (0..bins)
            .map(|index| super::HistogramBin {
                lower: edges[index],
                upper: edges[index + 1],
                weight: 0.0,
                cells: 0,
            })
            .collect();
        let represented_edge = histogram_bins[0].upper;
        assert_eq!(represented_edge, 0.2);
        assert_eq!(super::numeric_histogram_index(0.2, minimum, maximum, &histogram_bins), 1);
        assert_eq!(
            super::numeric_histogram_index(
                f64::from_bits(represented_edge.to_bits() - 1),
                minimum,
                maximum,
                &histogram_bins
            ),
            0
        );
        assert_eq!(
            super::numeric_histogram_index(
                f64::from_bits(represented_edge.to_bits() + 1),
                minimum,
                maximum,
                &histogram_bins
            ),
            1
        );
    }
}
