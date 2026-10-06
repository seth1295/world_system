//! Sans-IO body loading and hash-verified content requests.

use core::fmt;

use serde_json::Value;

use crate::body::{
    BodyRoot, FieldDescriptor, FieldId, FieldRegistry, ModelError, NamedSectionRef, SectionRef,
    field_dtype, parse_hash, raw_storage_value_fits, temporal_slice_count,
};
use crate::canon::blob::{
    BlobError, BlobKind, CanonicalBlob, MAX_CANONICAL_BLOB_BYTES, decode_zstd_shuffle2_bounded,
};
use crate::canon::hash;
use crate::canon::index::IndexBlob;
use crate::canon::jcs;
use crate::canon::ledger;
use crate::ids::{Hash32, ObjectId};
use crate::path::validate_artifact_path;

/// A specific section or content item the caller must supply.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Need {
    /// Canonical JSON section referenced by the body root.
    Section {
        /// Relative path within the artifact.
        path: String,
        /// Hash of JCS-parsed section content.
        hash: Hash32,
    },
    /// Compressed index blob for a field.
    Index {
        /// Field ID whose tiles are indexed.
        field_id: FieldId,
        /// Relative index path.
        path: String,
        /// Hash of the canonical uncompressed VYB1 blob.
        hash: Hash32,
    },
    /// Compressed raster or opaque blob.
    Blob {
        /// Hash of the canonical uncompressed VYB1 blob.
        hash: Hash32,
    },
    /// Append-only ledger whose expected identity is its head hash.
    Ledger {
        /// Stable ledger name.
        name: String,
        /// Relative path within the artifact.
        path: String,
        /// Expected final chain hash.
        head: Hash32,
    },
}

/// An abstract source for caller-owned storage.
pub trait BlobSource {
    /// Loads bytes for one previously requested item.
    fn load(&self, need: &Need) -> Result<Vec<u8>, SourceError>;
}

/// Caller-provided storage could not satisfy a need.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceError(pub String);

impl fmt::Display for SourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SourceError {}

/// A sans-IO state machine that verifies a body and requests its content closure.
#[derive(Clone, Debug)]
pub struct BodyLoader {
    root: BodyRoot,
    root_value: Value,
    baseline_id: Hash32,
    pending: std::collections::BTreeSet<Need>,
    sections: std::collections::BTreeMap<String, Value>,
    section_bytes: std::collections::BTreeMap<String, Vec<u8>>,
    indexes: std::collections::BTreeMap<FieldId, IndexBlob>,
    blobs: std::collections::BTreeMap<Hash32, Vec<u8>>,
    blob_limits: std::collections::BTreeMap<Hash32, usize>,
    ledgers: std::collections::BTreeMap<String, Vec<u8>>,
    registry: Option<FieldRegistry>,
}

impl BodyLoader {
    /// Parses body root JSON, checks its canonical identity, and returns initial needs.
    pub fn begin(body_json: &[u8]) -> Result<(Self, Vec<Need>), LoaderError> {
        let canonical = jcs::canonicalize_json(body_json).map_err(LoaderError::Jcs)?;
        let root_value: Value =
            serde_json::from_slice(body_json).map_err(|_| LoaderError::InvalidBodyJson)?;
        let root: BodyRoot =
            serde_json::from_value(root_value.clone()).map_err(|_| LoaderError::InvalidBodyJson)?;
        root.validate().map_err(LoaderError::Model)?;
        let baseline_id = hash::hash(&canonical);
        let mut pending = std::collections::BTreeSet::new();

        add_section_need(&mut pending, &root.sections.registry)?;
        for section in root.sections.vocab.iter().chain(root.sections.features.iter()) {
            add_named_section_need(&mut pending, section)?;
        }
        if let Some(provenance) = &root.sections.provenance {
            add_section_need(&mut pending, &provenance.dag)?;
            add_section_need(&mut pending, &provenance.explain)?;
        }
        for value in root.sections.extra.values() {
            collect_extension_section_needs(value, &mut pending)?;
        }
        add_section_need(&mut pending, &root.dynamics.descriptor)?;
        add_section_need(&mut pending, &root.dynamics.origin_keyframe)?;
        if let Some(extension_ledger) = &root.extensions_ledger {
            validate_artifact_path(&extension_ledger.path).map_err(|_| LoaderError::InvalidPath)?;
            pending.insert(Need::Ledger {
                name: "extensions".to_owned(),
                path: extension_ledger.path.clone(),
                head: parse_hash(&extension_ledger.hash).map_err(LoaderError::Model)?,
            });
        }
        for (field_text, hash_text) in &root.indexes {
            let field_id = FieldId::parse(field_text).map_err(LoaderError::Model)?;
            let expected = parse_hash(hash_text).map_err(LoaderError::Model)?;
            pending.insert(Need::Index {
                field_id,
                path: format!("index/{field_id}.idx"),
                hash: expected,
            });
        }

        let loader = Self {
            root,
            root_value,
            baseline_id,
            pending,
            sections: std::collections::BTreeMap::new(),
            section_bytes: std::collections::BTreeMap::new(),
            indexes: std::collections::BTreeMap::new(),
            blobs: std::collections::BTreeMap::new(),
            blob_limits: std::collections::BTreeMap::new(),
            ledgers: std::collections::BTreeMap::new(),
            registry: None,
        };
        let needs = loader.needs();
        Ok((loader, needs))
    }

    /// Verifies one requested item and reports the remaining content closure.
    pub fn provide(&mut self, need: &Need, bytes: Vec<u8>) -> Result<Vec<Need>, LoaderError> {
        if !self.pending.contains(need) {
            return Err(LoaderError::UnexpectedNeed);
        }
        match need {
            Need::Section { path, hash: expected } => {
                let canonical = jcs::canonicalize_json(&bytes).map_err(LoaderError::Jcs)?;
                if hash::hash(&canonical) != *expected {
                    return Err(LoaderError::HashMismatch(*expected));
                }
                let value: Value =
                    serde_json::from_slice(&bytes).map_err(|_| LoaderError::InvalidSection)?;
                let registry = if path == &self.root.sections.registry.path {
                    let registry: FieldRegistry = serde_json::from_value(value.clone())
                        .map_err(|_| LoaderError::InvalidSection)?;
                    self.root.validate_registry(&registry).map_err(LoaderError::Model)?;
                    Some(registry)
                } else {
                    None
                };
                self.sections.insert(path.clone(), value);
                self.section_bytes.insert(path.clone(), bytes);
                if let Some(registry) = registry {
                    self.registry = Some(registry);
                    self.refresh_blob_needs();
                }
            }
            Need::Index { field_id, hash: expected, .. } => {
                let canonical = decode_zstd_shuffle2_bounded(&bytes, MAX_CANONICAL_BLOB_BYTES)
                    .map_err(map_blob_decode_error)?;
                if hash::hash(&canonical) != *expected {
                    return Err(LoaderError::HashMismatch(*expected));
                }
                let index = IndexBlob::decode(&canonical).map_err(|_| LoaderError::InvalidIndex)?;
                if index.field_id != field_id.0 {
                    return Err(LoaderError::IndexFieldMismatch);
                }
                self.indexes.insert(*field_id, index);
                self.refresh_blob_needs();
            }
            Need::Blob { hash: expected } => {
                let limit =
                    self.blob_limits.get(expected).copied().unwrap_or(MAX_CANONICAL_BLOB_BYTES);
                let canonical =
                    decode_zstd_shuffle2_bounded(&bytes, limit).map_err(map_blob_decode_error)?;
                if hash::hash(&canonical) != *expected {
                    return Err(LoaderError::HashMismatch(*expected));
                }
                CanonicalBlob::decode(&canonical).map_err(|_| LoaderError::InvalidBlob)?;
                self.blobs.insert(*expected, canonical);
            }
            Need::Ledger { name, head, .. } => {
                let verified = ledger::verify(&bytes).map_err(|_| LoaderError::InvalidLedger)?;
                if verified.hash != *head {
                    return Err(LoaderError::HashMismatch(*head));
                }
                self.ledgers.insert(name.clone(), bytes);
            }
        }
        self.pending.remove(need);
        Ok(self.needs())
    }

    fn refresh_blob_needs(&mut self) {
        self.blob_limits.clear();
        let indexes: Vec<IndexBlob> = self.indexes.values().cloned().collect();
        for index in indexes {
            for entry in &index.entries {
                let crate::canon::index::IndexValue::Blob(blob_hash) = entry.value else {
                    continue;
                };
                let limit = self.blob_limit(&index, entry.level);
                self.blob_limits
                    .entry(blob_hash)
                    .and_modify(|existing| *existing = (*existing).max(limit))
                    .or_insert(limit);
                if !self.blobs.contains_key(&blob_hash) {
                    self.pending.insert(Need::Blob { hash: blob_hash });
                }
            }
        }
    }

    fn blob_limit(&self, index: &IndexBlob, level: u8) -> usize {
        let Some(field) = self
            .registry
            .as_ref()
            .and_then(|registry| registry.fields.iter().find(|field| field.id.0 == index.field_id))
        else {
            return MAX_CANONICAL_BLOB_BYTES;
        };
        let Some(domain) = self.root.domains.iter().find(|domain| domain.id == field.domain) else {
            return MAX_CANONICAL_BLOB_BYTES;
        };
        let edge = 1_usize << usize::from(level.min(index.tile_log2));
        let (dim_i, dim_j) = match domain.topology.as_str() {
            "veyra.topo.dir_cube/1" => (edge, edge),
            "veyra.topo.radial_1d/1" => (edge, 1),
            _ => return MAX_CANONICAL_BLOB_BYTES,
        };
        let width = field_dtype(field).and_then(|dtype| dtype.width()).unwrap_or(4);
        let slices = temporal_slice_count(field).map(usize::from).unwrap_or(usize::from(u16::MAX));
        dim_i
            .checked_mul(dim_j)
            .and_then(|size| size.checked_mul(slices))
            .and_then(|size| size.checked_mul(width))
            .and_then(|size| size.checked_add(16))
            .map(|size| size.min(MAX_CANONICAL_BLOB_BYTES))
            .unwrap_or(MAX_CANONICAL_BLOB_BYTES)
    }

    /// Returns the ordered set of items that remain unavailable.
    pub fn needs(&self) -> Vec<Need> {
        self.pending.iter().cloned().collect()
    }

    /// Completes loading after every section, index, blob, and ledger is verified.
    pub fn finish(self) -> Result<Body, LoaderError> {
        if !self.pending.is_empty() {
            return Err(LoaderError::Missing(self.needs()));
        }
        let registry_bytes = self
            .section_bytes
            .get(&self.root.sections.registry.path)
            .ok_or(LoaderError::InvalidSection)?;
        let registry: FieldRegistry =
            serde_json::from_slice(registry_bytes).map_err(|_| LoaderError::InvalidSection)?;
        self.root.validate_registry(&registry).map_err(LoaderError::Model)?;
        if self.root.figure.kind == "star_convex_radial" {
            let radius_name = self
                .root
                .figure
                .parameters
                .get("radius_field")
                .and_then(Value::as_str)
                .ok_or(LoaderError::Model(ModelError::InvalidFigureField))?;
            let radius_field = registry
                .fields
                .iter()
                .find(|field| field.name == radius_name)
                .ok_or(LoaderError::Model(ModelError::InvalidFigureField))?;
            if self.indexes.get(&radius_field.id).is_none_or(|index| index.entries.is_empty()) {
                return Err(LoaderError::Model(ModelError::InvalidFigureField));
            }
        }
        for (field_id, index) in &self.indexes {
            let field = registry
                .fields
                .iter()
                .find(|field| field.id == *field_id)
                .ok_or(LoaderError::IndexFieldMismatch)?;
            let domain = self.root.domains.iter().find(|domain| domain.id == field.domain);
            let Some(domain) = domain else {
                if field.compat == crate::body::Compatibility::Ancillary {
                    continue;
                }
                return Err(LoaderError::IndexFieldMismatch);
            };
            let expected_topology = match domain.topology.as_str() {
                "veyra.topo.dir_cube/1" => crate::canon::index::TopologyTag::DirCube,
                "veyra.topo.radial_1d/1" => crate::canon::index::TopologyTag::Radial1d,
                _ => return Err(LoaderError::IndexFieldMismatch),
            };
            if index.topology != expected_topology
                || index.tile_log2 != domain.tile_log2
                || index
                    .entries
                    .iter()
                    .any(|entry| entry.level > domain.max_level || entry.level > field.native_level)
            {
                return Err(LoaderError::IndexFieldMismatch);
            }
            for entry in &index.entries {
                match entry.value {
                    crate::canon::index::IndexValue::Blob(expected) => {
                        let canonical = self.blobs.get(&expected).ok_or_else(|| {
                            LoaderError::Missing(vec![Need::Blob { hash: expected }])
                        })?;
                        let blob = CanonicalBlob::decode(canonical)
                            .map_err(|_| LoaderError::InvalidBlob)?;
                        if blob.kind != BlobKind::RasterTile {
                            return Err(LoaderError::RasterKindMismatch);
                        }
                        if field_dtype(field).is_some_and(|dtype| dtype != blob.dtype) {
                            return Err(LoaderError::RasterDTypeMismatch);
                        }
                        let edge = 1_u64 << entry.level.min(index.tile_log2);
                        let expected_dimensions = match expected_topology {
                            crate::canon::index::TopologyTag::DirCube => (edge, edge),
                            crate::canon::index::TopologyTag::Radial1d => (edge, 1),
                        };
                        if expected_dimensions.0 > u64::from(u16::MAX)
                            || expected_dimensions.1 > u64::from(u16::MAX)
                            || u64::from(blob.dim_i) != expected_dimensions.0
                            || u64::from(blob.dim_j) != expected_dimensions.1
                        {
                            return Err(LoaderError::RasterDimensionsMismatch);
                        }
                        if temporal_slice_count(field).is_some_and(|slices| slices != blob.slices) {
                            return Err(LoaderError::RasterSlicesMismatch);
                        }
                    }
                    crate::canon::index::IndexValue::Const(value) => {
                        validate_const_value(field, value)?;
                    }
                }
            }
        }
        Ok(Body {
            root: self.root,
            root_value: self.root_value,
            baseline_id: self.baseline_id,
            registry,
            sections: self.sections,
            indexes: self.indexes,
            blobs: self.blobs,
            ledgers: self.ledgers,
        })
    }
}

/// A completely verified, standalone logical body baseline.
#[derive(Clone, Debug)]
pub struct Body {
    root: BodyRoot,
    root_value: Value,
    baseline_id: Hash32,
    registry: FieldRegistry,
    sections: std::collections::BTreeMap<String, Value>,
    indexes: std::collections::BTreeMap<FieldId, IndexBlob>,
    blobs: std::collections::BTreeMap<Hash32, Vec<u8>>,
    ledgers: std::collections::BTreeMap<String, Vec<u8>>,
}

impl Body {
    /// Returns the stable object identity.
    pub fn object_id(&self) -> Result<ObjectId, LoaderError> {
        self.root.identity.object_id.parse().map_err(LoaderError::Model)
    }

    /// Returns the baseline identity derived from canonical `body.json`.
    pub const fn baseline_id(&self) -> Hash32 {
        self.baseline_id
    }

    /// Returns the generic body classification value.
    pub fn classification(&self) -> &Value {
        &self.root.classification
    }

    /// Returns the full preserved JSON root value.
    pub fn root_value(&self) -> &Value {
        &self.root_value
    }

    /// Returns the validated field registry.
    pub fn field_registry(&self) -> &FieldRegistry {
        &self.registry
    }

    /// Returns named vocabulary references from the body root.
    pub fn vocabularies(&self) -> &[NamedSectionRef] {
        &self.root.sections.vocab
    }

    /// Returns named feature-table references from the body root.
    pub fn feature_tables(&self) -> &[NamedSectionRef] {
        &self.root.sections.features
    }

    /// Returns field descriptors, including ancillary descriptors preserved without interpretation.
    pub fn fields(&self) -> &[crate::body::FieldDescriptor] {
        &self.registry.fields
    }

    /// Returns one verified index by field identifier.
    pub fn index(&self, field_id: FieldId) -> Option<&IndexBlob> {
        self.indexes.get(&field_id)
    }

    /// Returns one verified canonical uncompressed blob.
    pub fn blob(&self, hash: Hash32) -> Option<&[u8]> {
        self.blobs.get(&hash).map(Vec::as_slice)
    }

    /// Returns a parsed section by artifact path.
    pub fn section(&self, path: &str) -> Option<&Value> {
        self.sections.get(path)
    }

    /// Returns a verified ledger by name.
    pub fn ledger(&self, name: &str) -> Option<&[u8]> {
        self.ledgers.get(name).map(Vec::as_slice)
    }
}

fn add_section_need(
    pending: &mut std::collections::BTreeSet<Need>,
    section: &SectionRef,
) -> Result<(), LoaderError> {
    validate_artifact_path(&section.path).map_err(|_| LoaderError::InvalidPath)?;
    pending.insert(Need::Section {
        path: section.path.clone(),
        hash: parse_hash(&section.hash).map_err(LoaderError::Model)?,
    });
    Ok(())
}

fn add_named_section_need(
    pending: &mut std::collections::BTreeSet<Need>,
    section: &NamedSectionRef,
) -> Result<(), LoaderError> {
    validate_artifact_path(&section.path).map_err(|_| LoaderError::InvalidPath)?;
    pending.insert(Need::Section {
        path: section.path.clone(),
        hash: parse_hash(&section.hash).map_err(LoaderError::Model)?,
    });
    Ok(())
}

fn collect_extension_section_needs(
    value: &Value,
    pending: &mut std::collections::BTreeSet<Need>,
) -> Result<(), LoaderError> {
    match value {
        Value::Object(object) => {
            if let (Some(path), Some(expected)) = (
                object.get("path").and_then(Value::as_str),
                object.get("hash").and_then(Value::as_str),
            ) {
                validate_artifact_path(path).map_err(|_| LoaderError::InvalidPath)?;
                pending.insert(Need::Section {
                    path: path.to_owned(),
                    hash: parse_hash(expected).map_err(LoaderError::Model)?,
                });
            }
            for nested in object.values() {
                collect_extension_section_needs(nested, pending)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_extension_section_needs(item, pending)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn map_blob_decode_error(error: BlobError) -> LoaderError {
    match error {
        BlobError::OutputLimitExceeded => LoaderError::DecompressedContentTooLarge,
        BlobError::AllocationFailed => LoaderError::DecompressionAllocationFailed,
        _ => LoaderError::InvalidCodec,
    }
}

fn validate_const_value(field: &FieldDescriptor, value: i64) -> Result<(), LoaderError> {
    if field.compat == crate::body::Compatibility::Ancillary && field_dtype(field).is_none() {
        return Ok(());
    }
    if field_dtype(field).is_some_and(|dtype| !raw_storage_value_fits(dtype, value)) {
        return Err(LoaderError::ConstValueOutOfRange);
    }
    Ok(())
}

/// Sans-IO loader failure with stable semantic categories.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoaderError {
    /// Root JSON is invalid or does not match the typed V1 model.
    InvalidBodyJson,
    /// A section is invalid JSON or does not satisfy its section contract.
    InvalidSection,
    /// A referenced artifact path is not a safe relative path.
    InvalidPath,
    /// The caller supplied a need not requested by the loader.
    UnexpectedNeed,
    /// Required sections or content have not all been supplied.
    Missing(Vec<Need>),
    /// Canonical content does not match the expected digest.
    HashMismatch(Hash32),
    /// Codec bytes are invalid or cannot be decoded.
    InvalidCodec,
    /// Decompressed codec output exceeds the V1 reader resource limit.
    DecompressedContentTooLarge,
    /// Bounded decoder allocation failed.
    DecompressionAllocationFailed,
    /// Canonical blob header is invalid.
    InvalidBlob,
    /// Index bytes or metadata are invalid.
    InvalidIndex,
    /// Index field ID does not match the root mapping.
    IndexFieldMismatch,
    /// An indexed field references a non-raster blob.
    RasterKindMismatch,
    /// Raster storage dtype does not match the field descriptor.
    RasterDTypeMismatch,
    /// Raster dimensions do not match the tile topology and level.
    RasterDimensionsMismatch,
    /// Raster slice count does not match the field temporal contract.
    RasterSlicesMismatch,
    /// A constant raw value cannot be represented by its field storage dtype.
    ConstValueOutOfRange,
    /// A ledger chain is invalid.
    InvalidLedger,
    /// Canonical JSON validation failed.
    Jcs(crate::canon::jcs::JcsError),
    /// Root or field model validation failed.
    Model(ModelError),
}

impl fmt::Display for LoaderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBodyJson => formatter.write_str("invalid body.json"),
            Self::InvalidSection => formatter.write_str("invalid body section"),
            Self::InvalidPath => formatter.write_str("artifact path must be safe and relative"),
            Self::UnexpectedNeed => {
                formatter.write_str("content was provided without a matching need")
            }
            Self::Missing(needs) => {
                write!(formatter, "{} required artifact items are missing", needs.len())
            }
            Self::HashMismatch(hash) => write!(formatter, "content hash mismatch for {hash}"),
            Self::InvalidCodec => formatter.write_str("zstd-shuffle2 data is invalid"),
            Self::DecompressedContentTooLarge => {
                formatter.write_str("decompressed content exceeds the V1 size limit")
            }
            Self::DecompressionAllocationFailed => {
                formatter.write_str("could not allocate bounded decompressed content")
            }
            Self::InvalidBlob => formatter.write_str("canonical blob is invalid"),
            Self::InvalidIndex => formatter.write_str("canonical index is invalid"),
            Self::IndexFieldMismatch => {
                formatter.write_str("index field ID does not match its root entry")
            }
            Self::RasterKindMismatch => {
                formatter.write_str("indexed field content is not a raster tile")
            }
            Self::RasterDTypeMismatch => {
                formatter.write_str("raster tile dtype does not match its field descriptor")
            }
            Self::RasterDimensionsMismatch => {
                formatter.write_str("raster tile dimensions do not match its tile address")
            }
            Self::RasterSlicesMismatch => {
                formatter.write_str("raster tile slices do not match its temporal descriptor")
            }
            Self::ConstValueOutOfRange => {
                formatter.write_str("constant value is outside its field storage dtype")
            }
            Self::InvalidLedger => formatter.write_str("hash-chained ledger is invalid"),
            Self::Jcs(error) => error.fmt(formatter),
            Self::Model(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for LoaderError {}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{BodyLoader, LoaderError, Need};
    use crate::canon::blob::{BlobKind, CanonicalBlob, DType, shuffle2};
    use crate::canon::hash;
    use crate::canon::index::{IndexBlob, IndexEntry, IndexValue, TopologyTag};
    use crate::canon::jcs;
    use crate::ids::{ObjectAddress, ObjectId, UniverseId};
    use crate::spatial::{DirCube, Radial1d, Topology};

    fn compressed(canonical: &[u8]) -> Vec<u8> {
        zstd::stream::encode_all(shuffle2(canonical).as_slice(), 0).unwrap()
    }

    fn verify_raster_blob(
        indexed_blob: &[u8],
        supplied_blob: &[u8],
    ) -> Result<super::Body, LoaderError> {
        verify_raster_blob_on_topology(indexed_blob, supplied_blob, TopologyTag::DirCube)
    }

    fn verify_raster_blob_on_topology(
        indexed_blob: &[u8],
        supplied_blob: &[u8],
        topology: TopologyTag,
    ) -> Result<super::Body, LoaderError> {
        verify_index_value_on_topology(
            IndexValue::Blob(hash::hash(indexed_blob)),
            Some(supplied_blob),
            topology,
            None,
            1,
        )
    }

    fn verify_blob_slice_contract(
        indexed_blob: &[u8],
        expected_slices: u16,
    ) -> Result<super::Body, LoaderError> {
        verify_index_value_on_topology(
            IndexValue::Blob(hash::hash(indexed_blob)),
            Some(indexed_blob),
            TopologyTag::DirCube,
            Some("u8"),
            expected_slices,
        )
    }

    fn verify_const_entry(dtype: &str, raw: i64) -> Result<super::Body, LoaderError> {
        verify_index_value_on_topology(
            IndexValue::Const(raw),
            None,
            TopologyTag::DirCube,
            Some(dtype),
            1,
        )
    }

    fn verify_index_value_on_topology(
        index_value: IndexValue,
        supplied_blob: Option<&[u8]>,
        topology: TopologyTag,
        storage_dtype_override: Option<&str>,
        expected_slices: u16,
    ) -> Result<super::Body, LoaderError> {
        let (
            field_id,
            field_text,
            field_name,
            field_capability,
            field_semantic,
            dtype,
            scale,
            cell,
            topology_id,
            domain_id,
            vertical,
            interpolation,
            figure,
            reference_surfaces,
            capabilities,
        ) = match topology {
            TopologyTag::DirCube => (
                0x0101_0001,
                "0x01010001",
                "topography.height_m",
                "veyra.cap.topography/1",
                "scalar.height",
                "i16",
                "0.5",
                DirCube::key(2, 6, 3, 3).unwrap(),
                "veyra.topo.dir_cube/1",
                "surface",
                json!({"kind":"none"}),
                "bilinear",
                json!({"kind":"sphere","radius_m":"1"}),
                json!([{"id":"datum.mean","kind":"sphere","radius_m":"1"}]),
                json!([
                    {"id":"veyra.cap.solid_surface/1","params":{"figure_ref":"figure"}},
                    {"id":"veyra.cap.topography/1","params":{"reference_surface":"datum.mean","domain":"surface"}}
                ]),
            ),
            TopologyTag::Radial1d => (
                0x0130_0001,
                "0x01300001",
                "stellar.density",
                "veyra.cap.stellar_structure/1",
                "scalar.density",
                "u32",
                "0.01",
                Radial1d::key(3, 6).unwrap(),
                "veyra.topo.radial_1d/1",
                "interior",
                json!({"kind":"radius","extent_m":"2"}),
                "linear",
                json!({"kind":"radial_profile_sphere","extent_m":"2"}),
                json!([{"id":"photosphere","kind":"sphere","radius_m":"2"}]),
                json!([{"id":"veyra.cap.stellar_structure/1","params":{"domain":"interior"}}]),
            ),
        };
        let tile = match topology {
            TopologyTag::DirCube => DirCube.tile_key(cell, 2).unwrap(),
            TopologyTag::Radial1d => Radial1d::default().tile_key(cell, 2).unwrap(),
        };
        let index = IndexBlob {
            field_id,
            topology,
            tile_log2: 2,
            entries: vec![IndexEntry { level: 3, key: tile.address.0, value: index_value }],
        }
        .encode()
        .unwrap();
        let index_hash = hash::hash(&index);
        let storage_dtype = storage_dtype_override.unwrap_or(dtype);
        let temporal = if expected_slices == 1 {
            json!({"kind":"static"})
        } else {
            json!({
                "kind":"periodic_slices","count":expected_slices,
                "period_ref":"dynamics.period","origin_ref":"dynamics.epoch"
            })
        };
        let persistence = if expected_slices == 1 { "invariant" } else { "periodic_mean" };
        let registry = json!({
            "schema":"veyra.field_registry/1",
            "fields":[{
                "id":field_text,"name":field_name,"capability":field_capability,
                "domain":domain_id,"semantic":field_semantic,"persistence":persistence,
                "storage":{"dtype":storage_dtype,"scale":scale,"offset":"0"},"native_level":3,
                "temporal":temporal,
                "sampling":{"interp":interpolation,"below_native":"pyramid","above_native":"refine"},
                "downsample":"mean","compat":"critical"
            }]
        });
        let descriptor = json!({"schema":"veyra.dynamics_descriptor/1"});
        let origin = json!({"schema":"veyra.dynamics_origin/1"});
        let sections: Vec<(String, Vec<u8>)> = [
            ("registry/fields.json", registry),
            ("dynamics/descriptor.json", descriptor),
            ("dynamics/origin.json", origin),
        ]
        .into_iter()
        .map(|(path, value)| (path.to_owned(), serde_json::to_vec(&value).expect("section JSON")))
        .collect();
        let section_refs: serde_json::Map<String, serde_json::Value> = sections
            .iter()
            .map(|(path, bytes)| {
                let canonical = jcs::canonicalize_json(bytes).unwrap();
                (path.clone(), json!({"path":path,"hash":hash::hash(&canonical).to_string()}))
            })
            .collect();
        let object_id = ObjectId::derive(
            UniverseId::fixture_sentinel(),
            &ObjectAddress::Fixture { name: "raster-contract".to_owned() },
        )
        .unwrap();
        let body = json!({
            "schema":"veyra.body/1","format_version":{"major":1,"minor":0},
            "required_features":["veyra.body/1","veyra.canon.jcs/1","veyra.codec.zstd-shuffle2/1",topology_id],
            "identity":{"object_id":object_id.to_string(),"origin":{"kind":"fixture","name":"raster-contract"}},
            "classification":{},"physical":{"gm_m3_s2":"1"},"figure":figure,
            "frames":{"body_fixed":{"axes":"+Z is the positive rotation pole; +X is the prime meridian; right-handed","rotation":{"kind":"uniform","period_s":"86400","epoch":"0","orientation_q_at_epoch":["1","0","0","0"],"relative_to":"universe_inertial"}}},"reference_surfaces":reference_surfaces,
            "dynamics":{"descriptor":section_refs["dynamics/descriptor.json"],"origin_keyframe":section_refs["dynamics/origin.json"]},
            "capabilities":capabilities,
            "domains":[{"id":domain_id,"topology":topology_id,"frame":"body_fixed","vertical":vertical,"tile_log2":2,"max_level":5}],
            "codec":"zstd+shuffle2","sections":{"registry":section_refs["registry/fields.json"]},
            "indexes":{field_text:index_hash.to_string()}
        });
        let (mut loader, needs) = BodyLoader::begin(&serde_json::to_vec(&body).unwrap())?;
        let encoded_index = compressed(&index);
        for need in needs {
            let bytes = match &need {
                Need::Section { path, .. } => sections
                    .iter()
                    .find(|(section_path, _)| section_path == path)
                    .unwrap()
                    .1
                    .clone(),
                Need::Index { .. } => encoded_index.clone(),
                _ => unreachable!("initial needs contain sections and indexes only"),
            };
            loader.provide(&need, bytes)?;
        }
        for need in loader.needs() {
            if matches!(need, Need::Blob { .. }) {
                let supplied_blob = supplied_blob.ok_or(LoaderError::UnexpectedNeed)?;
                loader.provide(&need, compressed(supplied_blob))?;
            }
        }
        loader.finish()
    }

    fn raster_blob(kind: BlobKind, dtype: DType, dim_i: u16, dim_j: u16, slices: u16) -> Vec<u8> {
        let byte_count = dtype
            .width()
            .map(|width| usize::from(dim_i) * usize::from(dim_j) * usize::from(slices) * width)
            .unwrap_or_default();
        CanonicalBlob::new(kind, dtype, dim_i, dim_j, slices, vec![7; byte_count]).unwrap().encode()
    }

    fn fixture_root() -> (Vec<u8>, Vec<(Need, Vec<u8>)>) {
        let registry = json!({"schema":"veyra.field_registry/1","fields":[]});
        let descriptor = json!({"schema":"veyra.dynamics_descriptor/1"});
        let origin = json!({"schema":"veyra.dynamics_origin/1"});
        let sections = [
            ("registry/fields.json", registry),
            ("dynamics/descriptor.json", descriptor),
            ("dynamics/origin.json", origin),
        ];
        let refs: Vec<(String, String, Vec<u8>)> = sections
            .into_iter()
            .map(|(path, value)| {
                let bytes = serde_json::to_vec(&value).unwrap();
                let canonical = jcs::canonicalize_json(&bytes).unwrap();
                (path.to_owned(), hash::hash(&canonical).to_string(), bytes)
            })
            .collect();
        let object_id = ObjectId::derive(
            UniverseId::fixture_sentinel(),
            &ObjectAddress::Fixture { name: "loader-test".to_owned() },
        )
        .unwrap();
        let root = json!({
            "schema":"veyra.body/1","format_version":{"major":1,"minor":0},
            "required_features":["veyra.body/1","veyra.canon.jcs/1","veyra.codec.zstd-shuffle2/1"],
            "identity":{"object_id":object_id.to_string(),"origin":{"kind":"fixture","name":"loader-test"}},
            "classification":{},"physical":{"gm_m3_s2":"1"},"figure":{"kind":"sphere","radius_m":"1"},
            "frames":{"body_fixed":{"axes":"+Z is the positive rotation pole; +X is the prime meridian; right-handed","rotation":{"kind":"uniform","period_s":"86400","epoch":"0","orientation_q_at_epoch":["1","0","0","0"],"relative_to":"universe_inertial"}}},"reference_surfaces":[],
            "dynamics":{"descriptor":{"path":refs[1].0,"hash":refs[1].1},"origin_keyframe":{"path":refs[2].0,"hash":refs[2].1}},
            "capabilities":[],"domains":[],"codec":"zstd+shuffle2",
            "sections":{"registry":{"path":refs[0].0,"hash":refs[0].1}},"indexes":{}
        });
        let body_json = serde_json::to_vec(&root).unwrap();
        let needs = refs
            .into_iter()
            .map(|(path, section_hash, bytes)| {
                (
                    Need::Section { path, hash: crate::ids::Hash32::parse(&section_hash).unwrap() },
                    bytes,
                )
            })
            .collect();
        (body_json, needs)
    }

    #[test]
    fn loader_begin_provide_finish_verifies_a_minimal_body() {
        let (root, sections) = fixture_root();
        let (mut loader, needs) = BodyLoader::begin(&root).unwrap();
        assert_eq!(needs.len(), 3);
        for (need, bytes) in sections {
            loader.provide(&need, bytes).unwrap();
        }
        let body = loader.finish().unwrap();
        assert_eq!(body.fields().len(), 0);
        assert_eq!(
            body.object_id().unwrap().to_string(),
            ObjectId::derive(
                UniverseId::fixture_sentinel(),
                &ObjectAddress::Fixture { name: "loader-test".to_owned() },
            )
            .unwrap()
            .to_string()
        );
    }

    #[test]
    fn loader_rejects_wrong_hash_and_unsafe_path() {
        let (root, sections) = fixture_root();
        let (mut loader, _) = BodyLoader::begin(&root).unwrap();
        let (need, correct_bytes) = sections.first().unwrap();
        assert!(matches!(loader.provide(need, b"{}".to_vec()), Err(LoaderError::HashMismatch(_))));
        assert!(loader.needs().contains(need));
        loader.provide(need, correct_bytes.clone()).unwrap();
        for (other_need, bytes) in sections.iter().skip(1) {
            loader.provide(other_need, bytes.clone()).unwrap();
        }
        assert!(loader.finish().is_ok());

        let mut invalid: Value = serde_json::from_slice(&root).unwrap();
        invalid["sections"]["registry"]["path"] = Value::String("../escape.json".to_owned());
        assert_eq!(
            BodyLoader::begin(&serde_json::to_vec(&invalid).unwrap()).unwrap_err(),
            LoaderError::InvalidPath
        );
    }

    #[test]
    fn loader_requests_and_verifies_an_extension_ledger_head() {
        let (mut root, sections) = fixture_root();
        let (ledger_bytes, ledger_head) = crate::canon::ledger::append(
            &[],
            "sealed",
            serde_json::json!({"extension":"ext:b3:fixture"}),
            crate::time::UTime::from_nanos(0),
        )
        .unwrap();
        let mut root_value: Value = serde_json::from_slice(&root).unwrap();
        root_value["extensions_ledger"] = serde_json::json!({
            "path":"extensions/ledger.jsonl",
            "hash":ledger_head.to_string()
        });
        root = serde_json::to_vec(&root_value).unwrap();
        let (mut loader, needs) = BodyLoader::begin(&root).unwrap();
        let ledger_need = needs
            .iter()
            .find(|need| matches!(need, Need::Ledger { name, .. } if name == "extensions"))
            .unwrap()
            .clone();
        for (need, bytes) in sections {
            loader.provide(&need, bytes).unwrap();
        }
        loader.provide(&ledger_need, ledger_bytes.clone()).unwrap();
        let body = loader.finish().unwrap();
        assert_eq!(body.ledger("extensions"), Some(ledger_bytes.as_slice()));
    }

    #[test]
    fn loader_requires_indexed_blobs_to_match_raster_field_contract() {
        let correct = raster_blob(BlobKind::RasterTile, DType::I16, 4, 4, 1);
        assert!(verify_raster_blob(&correct, &correct).is_ok());

        for kind in [BlobKind::Index, BlobKind::Columnar] {
            let wrong_kind = raster_blob(kind, DType::Raw, 0, 0, 0);
            assert_eq!(
                verify_raster_blob(&wrong_kind, &wrong_kind).unwrap_err(),
                LoaderError::RasterKindMismatch
            );
        }

        let wrong_dtype = raster_blob(BlobKind::RasterTile, DType::U8, 4, 4, 1);
        assert_eq!(
            verify_raster_blob(&wrong_dtype, &wrong_dtype).unwrap_err(),
            LoaderError::RasterDTypeMismatch
        );

        let wrong_dimensions = raster_blob(BlobKind::RasterTile, DType::I16, 2, 8, 1);
        assert_eq!(
            verify_raster_blob(&wrong_dimensions, &wrong_dimensions).unwrap_err(),
            LoaderError::RasterDimensionsMismatch
        );

        let wrong_slices = raster_blob(BlobKind::RasterTile, DType::U8, 4, 4, 1);
        assert_eq!(
            verify_blob_slice_contract(&wrong_slices, 2).unwrap_err(),
            LoaderError::RasterSlicesMismatch
        );
    }

    #[test]
    fn radial_index_raster_tiles_use_the_declared_one_dimensional_shape() {
        let correct = raster_blob(BlobKind::RasterTile, DType::U32, 4, 1, 1);
        assert!(verify_raster_blob_on_topology(&correct, &correct, TopologyTag::Radial1d).is_ok());

        let wrong_shape = raster_blob(BlobKind::RasterTile, DType::U32, 2, 2, 1);
        assert_eq!(
            verify_raster_blob_on_topology(&wrong_shape, &wrong_shape, TopologyTag::Radial1d)
                .unwrap_err(),
            LoaderError::RasterDimensionsMismatch
        );
    }

    #[test]
    fn raster_content_hash_mismatch_is_reported_before_contract_validation() {
        let indexed = raster_blob(BlobKind::RasterTile, DType::I16, 4, 4, 1);
        let supplied = raster_blob(BlobKind::RasterTile, DType::U8, 4, 4, 1);
        assert_eq!(
            verify_raster_blob(&indexed, &supplied).unwrap_err(),
            LoaderError::HashMismatch(hash::hash(&indexed))
        );
    }

    #[test]
    fn body_loader_reports_the_bounded_decompression_limit_separately() {
        let compressed_bomb_payload = vec![0x5a; 512 * 1024];
        assert_eq!(
            verify_raster_blob(&compressed_bomb_payload, &compressed_bomb_payload).unwrap_err(),
            LoaderError::DecompressedContentTooLarge
        );
    }

    #[test]
    fn loader_checks_const_entries_against_every_v1_storage_dtype() {
        let cases = [
            ("u8", 0_i64, 255_i64),
            ("i8", i64::from(i8::MIN), i64::from(i8::MAX)),
            ("u16", 0, i64::from(u16::MAX)),
            ("i16", i64::from(i16::MIN), i64::from(i16::MAX)),
            ("u32", 0, i64::from(u32::MAX)),
            ("i32", i64::from(i32::MIN), i64::from(i32::MAX)),
        ];
        for (dtype, minimum, maximum) in cases {
            assert!(verify_const_entry(dtype, minimum).is_ok(), "{dtype} minimum");
            assert!(verify_const_entry(dtype, maximum).is_ok(), "{dtype} maximum");
            assert_eq!(
                verify_const_entry(dtype, minimum - 1).unwrap_err(),
                LoaderError::ConstValueOutOfRange,
                "{dtype} below range"
            );
            assert_eq!(
                verify_const_entry(dtype, maximum + 1).unwrap_err(),
                LoaderError::ConstValueOutOfRange,
                "{dtype} above range"
            );
        }
        assert!(verify_const_entry("f32", 0).is_ok());
        assert!(verify_const_entry("f32", i64::from(u32::MAX)).is_ok());
        assert_eq!(verify_const_entry("f32", -1).unwrap_err(), LoaderError::ConstValueOutOfRange);
        assert_eq!(
            verify_const_entry("f32", i64::from(u32::MAX) + 1).unwrap_err(),
            LoaderError::ConstValueOutOfRange
        );
    }

    use serde_json::Value;
}
