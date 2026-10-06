//! Low-level immutable body artifact construction and directory storage adapter.

#![forbid(unsafe_code)]

use core::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;
use veyra_core::body::{BodyRoot, FieldId, FieldRegistry, parse_hash};
use veyra_core::canon::blob::{CanonicalBlob, shuffle2};
use veyra_core::canon::hash;
use veyra_core::canon::index::IndexBlob;
use veyra_core::canon::jcs;
use veyra_core::canon::ledger;
use veyra_core::ids::Hash32;
use veyra_core::io::{BlobSource, Body, BodyLoader, LoaderError, Need, SourceError};

/// Writes new body files and refuses to overwrite existing paths.
pub struct ArtifactWriter {
    root: PathBuf,
    sections: std::collections::BTreeMap<String, (Hash32, Vec<u8>)>,
    indexes: std::collections::BTreeMap<FieldId, Hash32>,
    blobs: std::collections::BTreeSet<Hash32>,
    ledgers: std::collections::BTreeMap<String, Hash32>,
    body_written: bool,
}

impl ArtifactWriter {
    /// Creates an artifact directory that does not already exist.
    pub fn new(path: impl AsRef<Path>) -> Result<Self, WriterError> {
        let root = path.as_ref().to_path_buf();
        fs::create_dir(&root).map_err(WriterError::Io)?;
        Ok(Self {
            root,
            sections: std::collections::BTreeMap::new(),
            indexes: std::collections::BTreeMap::new(),
            blobs: std::collections::BTreeSet::new(),
            ledgers: std::collections::BTreeMap::new(),
            body_written: false,
        })
    }

    /// Writes a section and returns the hash of its parsed JCS content.
    pub fn write_json_section(&mut self, path: &str, bytes: &[u8]) -> Result<Hash32, WriterError> {
        validate_relative_path(path)?;
        if self.sections.contains_key(path) {
            return Err(WriterError::DuplicateSection);
        }
        let canonical = jcs::canonicalize_json(bytes).map_err(WriterError::Jcs)?;
        let value: Value = serde_json::from_slice(bytes).map_err(|_| WriterError::InvalidJson)?;
        let section_hash = hash::hash(&canonical);
        self.write_new(path, bytes)?;
        self.sections.insert(
            path.to_owned(),
            (section_hash, serde_json::to_vec(&value).map_err(|_| WriterError::InvalidJson)?),
        );
        Ok(section_hash)
    }

    /// Writes one canonical index blob using the body codec and returns its canonical hash.
    pub fn write_index(
        &mut self,
        field_id: FieldId,
        index: &IndexBlob,
    ) -> Result<Hash32, WriterError> {
        if self.indexes.contains_key(&field_id) || index.field_id != field_id.0 {
            return Err(WriterError::DuplicateIndex);
        }
        for entry in &index.entries {
            if let veyra_core::canon::index::IndexValue::Blob(blob_id) = entry.value
                && !self.blobs.contains(&blob_id)
            {
                return Err(WriterError::MissingBlob(blob_id));
            }
        }
        let canonical = index.encode().map_err(|_| WriterError::InvalidIndex)?;
        let id = hash::hash(&canonical);
        let compressed = encode_storage(&canonical)?;
        self.write_new(&format!("index/{field_id}.idx"), &compressed)?;
        self.indexes.insert(field_id, id);
        Ok(id)
    }

    /// Writes a canonical VYB1 blob; duplicate content in this writer is written only once.
    pub fn write_blob(&mut self, canonical: &[u8]) -> Result<Hash32, WriterError> {
        CanonicalBlob::decode(canonical).map_err(|_| WriterError::InvalidBlob)?;
        let id = hash::hash(canonical);
        if self.blobs.contains(&id) {
            return Ok(id);
        }
        let hex = id.text().strip_prefix("b3:").ok_or(WriterError::InvalidBlob)?.to_owned();
        let path = format!("blobs/{}/{hex}.zst", &hex[..2]);
        let compressed = encode_storage(canonical)?;
        self.write_new(&path, &compressed)?;
        self.blobs.insert(id);
        Ok(id)
    }

    /// Writes and verifies an append-only ledger whose expected identity is its head.
    pub fn write_ledger(
        &mut self,
        name: &str,
        path: &str,
        bytes: &[u8],
        expected_head: Hash32,
    ) -> Result<(), WriterError> {
        validate_relative_path(path)?;
        let head = ledger::verify(bytes).map_err(|_| WriterError::InvalidLedger)?;
        if head.hash != expected_head || self.ledgers.contains_key(name) {
            return Err(WriterError::InvalidLedger);
        }
        self.write_new(path, bytes)?;
        self.ledgers.insert(name.to_owned(), head.hash);
        Ok(())
    }

    /// Validates references and fields, then writes pretty JSON plus the verified body ID.
    pub fn write_body_json(&mut self, body_json: &[u8]) -> Result<Hash32, WriterError> {
        if self.body_written {
            return Err(WriterError::BodyAlreadyWritten);
        }
        let canonical = jcs::canonicalize_json(body_json).map_err(WriterError::Jcs)?;
        let value: Value =
            serde_json::from_slice(body_json).map_err(|_| WriterError::InvalidJson)?;
        let root: BodyRoot =
            serde_json::from_value(value.clone()).map_err(|_| WriterError::InvalidJson)?;
        root.validate().map_err(WriterError::Model)?;
        verify_section_ref(&root.sections.registry, &self.sections)?;
        for reference in root.sections.vocab.iter().chain(root.sections.features.iter()) {
            verify_section_ref(reference, &self.sections)?;
        }
        if let Some(provenance) = &root.sections.provenance {
            verify_section_ref(&provenance.dag, &self.sections)?;
            verify_section_ref(&provenance.explain, &self.sections)?;
        }
        verify_section_ref(&root.dynamics.descriptor, &self.sections)?;
        verify_section_ref(&root.dynamics.origin_keyframe, &self.sections)?;
        for (field_text, expected) in &root.indexes {
            let field_id = FieldId::parse(field_text).map_err(WriterError::Model)?;
            let expected_hash = parse_hash(expected).map_err(WriterError::Model)?;
            if self.indexes.get(&field_id) != Some(&expected_hash) {
                return Err(WriterError::InvalidIndex);
            }
        }
        if self.indexes.len() != root.indexes.len() {
            return Err(WriterError::InvalidIndex);
        }
        if let Some(extension_ledger) = &root.extensions_ledger {
            let expected = parse_hash(&extension_ledger.hash).map_err(WriterError::Model)?;
            if self.ledgers.get("extensions") != Some(&expected) {
                return Err(WriterError::InvalidLedger);
            }
        }
        let registry_value =
            self.sections.get(&root.sections.registry.path).ok_or(WriterError::InvalidSection)?;
        let registry: FieldRegistry =
            serde_json::from_slice(&registry_value.1).map_err(|_| WriterError::InvalidSection)?;
        root.validate_registry(&registry).map_err(WriterError::Model)?;

        let baseline_id = hash::hash(&canonical);
        let pretty = serde_json::to_vec_pretty(&value).map_err(|_| WriterError::InvalidJson)?;
        self.write_new("body.json", &pretty)?;
        self.write_new("body.id", format!("bas:{}\n", baseline_id).as_bytes())?;
        self.body_written = true;
        Ok(baseline_id)
    }

    fn write_new(&self, relative: &str, bytes: &[u8]) -> Result<(), WriterError> {
        validate_relative_path(relative)?;
        let path = safe_join(&self.root, relative)?;
        let parent = path.parent().ok_or(WriterError::InvalidPath)?;
        fs::create_dir_all(parent).map_err(WriterError::Io)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(WriterError::Io)?;
        use std::io::Write;
        file.write_all(bytes).map_err(WriterError::Io)?;
        file.sync_all().map_err(WriterError::Io)?;
        Ok(())
    }
}

/// Opens a body directory through the core's sans-IO need/provide contract.
pub fn open_directory(path: impl AsRef<Path>) -> Result<Body, WriterError> {
    let path = path.as_ref();
    let body_json = read_contained(path, "body.json")?;
    let (mut loader, mut needs) = BodyLoader::begin(&body_json).map_err(WriterError::Loader)?;
    let source = DirectoryBlobSource::new(path);
    while let Some(need) = needs.first().cloned() {
        let bytes = source.load(&need).map_err(|error| WriterError::Source(error.0))?;
        needs = loader.provide(&need, bytes).map_err(WriterError::Loader)?;
    }
    let body = loader.finish().map_err(WriterError::Loader)?;
    let body_id = String::from_utf8(read_contained(path, "body.id")?)
        .map_err(|_| WriterError::InvalidJson)?;
    if body_id.trim_end() != format!("bas:{}", body.baseline_id()) {
        return Err(WriterError::BodyIdMismatch);
    }
    Ok(body)
}

/// Verifies all referenced baseline content in a body directory.
pub fn verify_directory(path: impl AsRef<Path>) -> Result<Hash32, WriterError> {
    open_directory(path).map(|body| body.baseline_id())
}

/// Directory-backed implementation of the core's abstract content source.
pub struct DirectoryBlobSource {
    root: PathBuf,
}

impl DirectoryBlobSource {
    /// Creates a source rooted at a body artifact directory.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self { root: path.as_ref().to_path_buf() }
    }
}

impl BlobSource for DirectoryBlobSource {
    fn load(&self, need: &Need) -> Result<Vec<u8>, SourceError> {
        let relative = match need {
            Need::Section { path, .. } | Need::Ledger { path, .. } | Need::Index { path, .. } => {
                path.clone()
            }
            Need::Blob { hash } => {
                let hex = hash.text().trim_start_matches("b3:").to_owned();
                format!("blobs/{}/{hex}.zst", &hex[..2])
            }
        };
        read_contained(&self.root, &relative).map_err(|error| SourceError(error.to_string()))
    }
}

fn encode_storage(canonical: &[u8]) -> Result<Vec<u8>, WriterError> {
    let shuffled = shuffle2(canonical);
    zstd::stream::encode_all(shuffled.as_slice(), 3).map_err(WriterError::Io)
}

/// Encodes canonical bytes with the declared `zstd+shuffle2` storage codec.
pub fn encode_storage_file(canonical: &[u8]) -> Result<Vec<u8>, WriterError> {
    encode_storage(canonical)
}

fn verify_section_ref(
    reference: &veyra_core::body::SectionRef,
    sections: &std::collections::BTreeMap<String, (Hash32, Vec<u8>)>,
) -> Result<(), WriterError> {
    let expected = parse_hash(&reference.hash).map_err(WriterError::Model)?;
    if sections.get(&reference.path).map(|section| section.0) != Some(expected) {
        return Err(WriterError::InvalidSection);
    }
    Ok(())
}

fn validate_relative_path(path: &str) -> Result<(), WriterError> {
    let parsed = Path::new(path);
    if path.is_empty()
        || parsed.is_absolute()
        || parsed.components().any(|component| !matches!(component, Component::Normal(_)))
        || path.contains('\\')
    {
        return Err(WriterError::InvalidPath);
    }
    Ok(())
}

fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, WriterError> {
    validate_relative_path(relative)?;
    Ok(root.join(relative))
}

fn read_contained(root: &Path, relative: &str) -> Result<Vec<u8>, WriterError> {
    let canonical_root = root.canonicalize().map_err(WriterError::Io)?;
    let candidate = safe_join(&canonical_root, relative)?;
    let resolved = candidate.canonicalize().map_err(WriterError::Io)?;
    if !resolved.starts_with(&canonical_root) {
        return Err(WriterError::InvalidPath);
    }
    fs::read(resolved).map_err(WriterError::Io)
}

/// Writer, storage, codec, or validation failure.
#[derive(Debug)]
pub enum WriterError {
    /// Underlying filesystem operation failed.
    Io(io::Error),
    /// Canonical JSON validation failed.
    Jcs(veyra_core::canon::jcs::JcsError),
    /// Body root model is invalid.
    Model(veyra_core::body::ModelError),
    /// Core body loading failed.
    Loader(LoaderError),
    /// Abstract content source failed to satisfy a need.
    Source(String),
    /// JSON input is invalid.
    InvalidJson,
    /// Section content or reference is invalid.
    InvalidSection,
    /// Artifact path is not safe and relative.
    InvalidPath,
    /// A writer attempted to add the same section twice.
    DuplicateSection,
    /// A writer attempted to add the same field index twice.
    DuplicateIndex,
    /// An index references a blob that has not been written.
    MissingBlob(Hash32),
    /// Canonical index bytes are invalid.
    InvalidIndex,
    /// Canonical VYB1 bytes are invalid.
    InvalidBlob,
    /// Ledger content or head is invalid.
    InvalidLedger,
    /// Body root has already been written by this writer.
    BodyAlreadyWritten,
    /// Convenience body ID does not match canonical body root.
    BodyIdMismatch,
}

impl fmt::Display for WriterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Jcs(error) => error.fmt(formatter),
            Self::Model(error) => error.fmt(formatter),
            Self::Loader(error) => error.fmt(formatter),
            Self::Source(error) => formatter.write_str(error),
            Self::InvalidJson => formatter.write_str("invalid JSON"),
            Self::InvalidSection => formatter.write_str("invalid section reference or content"),
            Self::InvalidPath => formatter.write_str("artifact path must be safe and relative"),
            Self::DuplicateSection => {
                formatter.write_str("section path was written more than once")
            }
            Self::DuplicateIndex => formatter.write_str("field index was written more than once"),
            Self::MissingBlob(hash) => write!(formatter, "index references missing blob {hash}"),
            Self::InvalidIndex => formatter.write_str("invalid index blob"),
            Self::InvalidBlob => formatter.write_str("invalid canonical blob"),
            Self::InvalidLedger => formatter.write_str("invalid ledger or head hash"),
            Self::BodyAlreadyWritten => formatter.write_str("body root has already been written"),
            Self::BodyIdMismatch => formatter.write_str("body.id does not match body.json"),
        }
    }
}

impl std::error::Error for WriterError {}
