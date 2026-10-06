//! Generic body artifact metadata and capability-driven field descriptors.

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::canon::blob::DType;
use crate::ids::{Hash32, ObjectAddress, ObjectId, RegionKey, UniverseId};
use crate::spatial::SpatialError;
use crate::time::DecimalString;

const CAPABILITY_IDS: &str = include_str!("../../../schema/capability_ids.toml");

/// Canonical field identifier, composed from a capability ID and local ID.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FieldId(pub u32);

impl FieldId {
    /// Combines a permanent capability number and capability-local field number.
    pub const fn new(capability_id: u16, local_id: u16) -> Self {
        Self(((capability_id as u32) << 16) | local_id as u32)
    }

    /// Returns the allocated capability number.
    pub const fn capability_id(self) -> u16 {
        (self.0 >> 16) as u16
    }

    /// Returns the capability-local field number.
    pub const fn local_id(self) -> u16 {
        self.0 as u16
    }

    /// Parses the canonical `0x` plus eight lowercase hexadecimal digit form.
    pub fn parse(text: &str) -> Result<Self, ModelError> {
        let hex = text.strip_prefix("0x").ok_or(ModelError::InvalidFieldId)?;
        if hex.len() != 8
            || !hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ModelError::InvalidFieldId);
        }
        u32::from_str_radix(hex, 16).map(Self).map_err(|_| ModelError::InvalidFieldId)
    }
}

impl fmt::Display for FieldId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "0x{:08x}", self.0)
    }
}

impl Serialize for FieldId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for FieldId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// Compatibility policy for forward data.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Compatibility {
    /// Unknown semantics must stop the reader.
    #[default]
    Critical,
    /// Unknown data may be preserved without interpretation.
    Ancillary,
}

/// A format major/minor pair.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FormatVersion {
    /// Breaking version number.
    pub major: u16,
    /// Backward-compatible version number.
    pub minor: u16,
}

/// Content reference to a canonical JSON section.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SectionRef {
    /// Relative artifact path.
    pub path: String,
    /// Canonical JCS content hash.
    pub hash: String,
}

/// Content reference with a canonical logical name, used for vocabularies and feature tables.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NamedSectionRef {
    /// Stable vocabulary or feature-table name.
    pub name: String,
    /// Relative artifact path.
    pub path: String,
    /// Canonical JCS content hash.
    pub hash: String,
}

impl NamedSectionRef {
    /// Returns the path and hash view shared with ordinary section validation.
    pub fn section_ref(&self) -> SectionRef {
        SectionRef { path: self.path.clone(), hash: self.hash.clone() }
    }
}

/// Body identity and origin metadata.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Identity {
    /// Permanent body or system identity.
    pub object_id: ObjectIdText,
    /// Birth address or fixture origin record.
    pub origin: Value,
    /// Optional display label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Forward-compatible identity properties.
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, Value>,
}

/// String-backed ObjectId value for serde compatibility.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct ObjectIdText(String);

impl ObjectIdText {
    /// Parses the canonical `obj:<32 lowercase hex>` string.
    pub fn parse(&self) -> Result<ObjectId, ModelError> {
        ObjectId::parse(&self.0).map_err(|_| ModelError::InvalidObjectId)
    }

    /// Returns the canonical identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Authoritative physical parameter block.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Physical {
    /// Gravitational parameter in cubic metres per second squared.
    pub gm_m3_s2: String,
    /// Optional declared gravity model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gravity_model: Option<String>,
    /// Forward-compatible physical properties.
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, Value>,
}

/// Declared reference figure.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Figure {
    /// Version-one figure kind.
    pub kind: String,
    /// Figure parameters stored as decimal strings or content references.
    #[serde(flatten)]
    pub parameters: std::collections::BTreeMap<String, Value>,
}

/// Capability declaration carried by a body.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CapabilityRef {
    /// Versioned capability schema ID.
    pub id: String,
    /// Capability-specific parameters.
    #[serde(default = "empty_object")]
    pub params: Value,
    /// Unknown capabilities default to critical.
    #[serde(default)]
    pub compat: Compatibility,
    /// Unknown extension properties are preserved.
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, Value>,
}

/// Spatial domain that associates a topology, frame, and vertical convention.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Domain {
    /// Body-local stable domain name.
    pub id: String,
    /// Versioned topology ID.
    pub topology: String,
    /// Declared reference frame name.
    pub frame: String,
    /// Tile edge exponent.
    pub tile_log2: u8,
    /// Finest stored level.
    pub max_level: u8,
    /// Generic vertical and extent declaration.
    pub vertical: Value,
    /// Forward-compatible domain properties.
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, Value>,
}

/// The complete body artifact root model.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BodyRoot {
    /// Body root schema identifier.
    pub schema: String,
    /// Body format version.
    pub format_version: FormatVersion,
    /// Features required to interpret canonical data.
    pub required_features: Vec<String>,
    /// Stable body identity and origin.
    pub identity: Identity,
    /// Optional named body-class composition and tags.
    pub classification: Value,
    /// Authoritative GM and physical parameters.
    pub physical: Physical,
    /// Declared body shape model.
    pub figure: Figure,
    /// Named reference frames.
    pub frames: std::collections::BTreeMap<String, Value>,
    /// Named, body-declared reference surfaces.
    pub reference_surfaces: Vec<Value>,
    /// Opaque hashed dynamics descriptor and origin keyframe references.
    pub dynamics: DynamicsSections,
    /// Declared versioned physical domains.
    pub capabilities: Vec<CapabilityRef>,
    /// Body-local spatial domains.
    pub domains: Vec<Domain>,
    /// Blob storage codec. Codec bytes are not content identity.
    pub codec: String,
    /// Content-addressed JSON sections.
    pub sections: BodySections,
    /// Field ID to index content hash mapping.
    pub indexes: std::collections::BTreeMap<String, String>,
    /// Optional append-only sealed extension ledger reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions_ledger: Option<SectionRef>,
    /// Forward-compatible root data is retained on rewrite.
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, Value>,
}

/// Required dynamics baseline sections.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DynamicsSections {
    /// Immutable dynamical model descriptor.
    pub descriptor: SectionRef,
    /// Immutable origin state at materialization time.
    pub origin_keyframe: SectionRef,
    /// Forward-compatible dynamics properties.
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, Value>,
}

/// Body sections referenced by the root.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BodySections {
    /// Required field registry.
    pub registry: SectionRef,
    /// Optional vocabularies.
    #[serde(default)]
    pub vocab: Vec<NamedSectionRef>,
    /// Optional feature tables.
    #[serde(default)]
    pub features: Vec<NamedSectionRef>,
    /// Optional provenance documents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<ProvenanceSections>,
    /// Forward-compatible sections.
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, Value>,
}

/// Baseline provenance section references.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProvenanceSections {
    /// Static dependency graph.
    pub dag: SectionRef,
    /// Data-driven explanation recipes.
    pub explain: SectionRef,
    /// Forward-compatible provenance properties.
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, Value>,
}

/// Parsed field registry section.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FieldRegistry {
    /// Field registry schema identifier.
    pub schema: String,
    /// Registered fields.
    pub fields: Vec<FieldDescriptor>,
    /// Unknown registry metadata is preserved.
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, Value>,
}

/// Generic stored field descriptor.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FieldDescriptor {
    /// Capability-allocated field ID.
    pub id: FieldId,
    /// Stable semantic field name.
    pub name: String,
    /// Capability schema that owns the field.
    pub capability: String,
    /// Domain that contains the field.
    pub domain: String,
    /// Generic semantic identifier.
    pub semantic: String,
    /// Baseline persistence class.
    pub persistence: String,
    /// Storage dtype, scale, offset, and nodata metadata.
    pub storage: Value,
    /// Finest materialized field level.
    #[serde(default)]
    pub native_level: u8,
    /// Time sampling declaration.
    #[serde(default = "empty_object")]
    pub temporal: Value,
    /// Spatial sampling declaration.
    #[serde(default = "empty_object")]
    pub sampling: Value,
    /// Downsample operator.
    #[serde(default)]
    pub downsample: Option<String>,
    /// Compatibility policy for unknown field semantics.
    #[serde(default)]
    pub compat: Compatibility,
    /// Unknown field metadata is preserved.
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, Value>,
}

impl BodyRoot {
    /// Validates V1 structure, required features, identity, and generic body constraints.
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.schema != "veyra.body/1" {
            return Err(ModelError::UnsupportedSchema);
        }
        if self.format_version.major != 1 {
            return Err(ModelError::UnsupportedMajorVersion(self.format_version.major));
        }
        self.validate_identity()?;
        validate_positive_decimal(&self.physical.gm_m3_s2)?;
        if !self.frames.contains_key("body_fixed") {
            return Err(ModelError::InvalidFrame);
        }
        self.validate_figure()?;
        self.validate_features()?;
        self.validate_domains()?;
        self.validate_surfaces()?;
        self.validate_named_sections()?;
        self.validate_capabilities()?;
        if self.codec != "zstd+shuffle2" {
            return Err(ModelError::UnknownRequiredFeature(self.codec.clone()));
        }
        Ok(())
    }

    fn validate_named_sections(&self) -> Result<(), ModelError> {
        if self
            .sections
            .vocab
            .iter()
            .chain(self.sections.features.iter())
            .any(|reference| reference.name.is_empty())
        {
            return Err(ModelError::InvalidSection);
        }
        Ok(())
    }

    fn validate_identity(&self) -> Result<(), ModelError> {
        let declared_id = self.identity.object_id.parse()?;
        let (universe, address) = parse_identity_origin(&self.identity.origin)?;
        let derived_id =
            ObjectId::derive(universe, &address).map_err(|_| ModelError::InvalidOrigin)?;
        if derived_id != declared_id {
            return Err(ModelError::ObjectIdOriginMismatch);
        }
        Ok(())
    }

    /// Validates all descriptors in a registry against this root's declarations.
    pub fn validate_registry(&self, registry: &FieldRegistry) -> Result<(), ModelError> {
        if registry.schema != "veyra.field_registry/1" {
            return Err(ModelError::InvalidSection);
        }
        let capabilities: std::collections::BTreeSet<&str> =
            self.capabilities.iter().map(|item| item.id.as_str()).collect();
        let domains: std::collections::BTreeSet<&str> =
            self.domains.iter().map(|item| item.id.as_str()).collect();
        let mut ids = std::collections::BTreeSet::new();
        for field in &registry.fields {
            if !ids.insert(field.id)
                || field.name.is_empty()
                || !capabilities.contains(field.capability.as_str())
            {
                if field.compat == Compatibility::Critical {
                    return Err(ModelError::InvalidField);
                }
                continue;
            }
            if let Some(capability_id) = capability_numeric_id(&field.capability)
                && field.id.capability_id() != capability_id
            {
                return Err(ModelError::InvalidField);
            }
            if !domains.is_empty()
                && !domains.contains(field.domain.as_str())
                && field.compat == Compatibility::Critical
            {
                return Err(ModelError::InvalidField);
            }
            if field.persistence == "dynamic" {
                return Err(ModelError::DynamicBaselineField);
            }
            if field.compat == Compatibility::Ancillary {
                continue;
            }
            let domain = self
                .domains
                .iter()
                .find(|domain| domain.id == field.domain)
                .ok_or(ModelError::InvalidField)?;
            if field.native_level > 30 || field.native_level > domain.max_level {
                return Err(ModelError::InvalidField);
            }
            if !matches!(
                field.persistence.as_str(),
                "invariant" | "periodic_mean" | "initial_state"
            ) {
                return Err(ModelError::UnknownCriticalSemantic(field.persistence.clone()));
            }
            validate_decimal_storage(&field.storage)?;
            if !is_known_semantic(&field.semantic) {
                return Err(ModelError::UnknownCriticalSemantic(field.semantic.clone()));
            }
            let dtype = field.storage.get("dtype").and_then(Value::as_str).unwrap_or_default();
            let Some(dtype_value) = field_dtype(field) else {
                return Err(ModelError::UnknownCriticalSemantic(dtype.to_owned()));
            };
            if let Some(nodata) = field_nodata(field)?
                && !raw_storage_value_fits(dtype_value, nodata)
            {
                return Err(ModelError::InvalidField);
            }
            if matches!(field.semantic.as_str(), "category" | "feature_ref" | "flags")
                && dtype_value == DType::F32
            {
                return Err(ModelError::InvalidField);
            }
            validate_critical_field_metadata(field, domain)?;
        }
        self.validate_figure_registry(registry)?;
        Ok(())
    }

    fn validate_figure_registry(&self, registry: &FieldRegistry) -> Result<(), ModelError> {
        if self.figure.kind != "star_convex_radial" {
            return Ok(());
        }
        let radius_name = self
            .figure
            .parameters
            .get("radius_field")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .ok_or(ModelError::InvalidFigureField)?;
        let field = registry
            .fields
            .iter()
            .find(|field| field.name == radius_name)
            .ok_or(ModelError::InvalidFigureField)?;
        let domain = self
            .domains
            .iter()
            .find(|domain| domain.id == field.domain)
            .ok_or(ModelError::InvalidFigureField)?;
        if field.compat != Compatibility::Critical
            || field.persistence != "invariant"
            || !field.semantic.starts_with("scalar.")
            || field.extra.get("unit").and_then(Value::as_str) != Some("m")
            || field_dtype(field).is_none()
            || domain.topology != "veyra.topo.dir_cube/1"
            || !self.indexes.contains_key(&field.id.to_string())
        {
            return Err(ModelError::InvalidFigureField);
        }
        Ok(())
    }

    fn validate_figure(&self) -> Result<(), ModelError> {
        match self.figure.kind.as_str() {
            "sphere" => validate_positive_decimal(
                self.figure
                    .parameters
                    .get("radius_m")
                    .and_then(Value::as_str)
                    .ok_or(ModelError::InvalidFigure)?,
            ),
            "star_convex_radial" => self
                .figure
                .parameters
                .get("radius_field")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .map(|_| ())
                .ok_or(ModelError::InvalidFigure),
            "radial_profile_sphere" => validate_positive_decimal(
                self.figure
                    .parameters
                    .get("extent_m")
                    .and_then(Value::as_str)
                    .ok_or(ModelError::InvalidFigure)?,
            ),
            _ => Err(ModelError::InvalidFigure),
        }
    }

    fn validate_features(&self) -> Result<(), ModelError> {
        let capability_contracts =
            crate::capability::contracts().map_err(|_| ModelError::InvalidCapabilityContract)?;
        let mut seen = std::collections::BTreeSet::new();
        for feature in &self.required_features {
            if !seen.insert(feature.as_str()) {
                return Err(ModelError::InvalidFeatureList);
            }
            let is_core_feature = matches!(
                feature.as_str(),
                "veyra.body/1"
                    | "veyra.canon.jcs/1"
                    | "veyra.topo.dir_cube/1"
                    | "veyra.topo.radial_1d/1"
                    | "veyra.codec.zstd-shuffle2/1"
            );
            if !is_core_feature && !capability_contracts.contains_key(feature) {
                return Err(ModelError::UnknownRequiredFeature(feature.clone()));
            }
        }
        for mandatory in ["veyra.body/1", "veyra.canon.jcs/1", "veyra.codec.zstd-shuffle2/1"] {
            if !seen.contains(mandatory) {
                return Err(ModelError::MissingRequiredFeature(mandatory.to_owned()));
            }
        }
        Ok(())
    }

    fn validate_capabilities(&self) -> Result<(), ModelError> {
        let contracts =
            crate::capability::contracts().map_err(|_| ModelError::InvalidCapabilityContract)?;
        let requirements = contracts
            .iter()
            .map(|(id, contract)| {
                let required = contract
                    .get("requires")
                    .and_then(Value::as_array)
                    .ok_or(ModelError::InvalidCapabilityContract)?
                    .iter()
                    .map(|required| {
                        required
                            .as_str()
                            .map(str::to_owned)
                            .ok_or(ModelError::InvalidCapabilityContract)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((id.clone(), required))
            })
            .collect::<Result<std::collections::BTreeMap<_, _>, ModelError>>()?;
        crate::capability::validate_dependency_graph(&requirements)
            .map_err(|_| ModelError::InvalidCapabilityContract)?;

        let mut seen = std::collections::BTreeSet::new();
        for capability in &self.capabilities {
            if !seen.insert(capability.id.as_str()) {
                return Err(ModelError::InvalidCapability);
            }
        }
        for capability in &self.capabilities {
            let Some(contract) = contracts.get(&capability.id) else {
                if capability.compat == Compatibility::Critical {
                    return Err(ModelError::UnknownCriticalCapability(capability.id.clone()));
                }
                continue;
            };
            let params_schema =
                contract.get("params_schema").ok_or(ModelError::InvalidCapabilityContract)?;
            if crate::capability::validate_instance(&capability.params, params_schema).is_err() {
                return Err(ModelError::InvalidCapabilityParameters(capability.id.clone()));
            }
            for required in
                requirements.get(&capability.id).ok_or(ModelError::InvalidCapabilityContract)?
            {
                if !seen.contains(required.as_str()) {
                    return Err(ModelError::MissingCapabilityDependency(required.clone()));
                }
            }

            let annotations = crate::capability::reference_annotations(params_schema)
                .map_err(|_| ModelError::InvalidCapabilityContract)?;
            for (parameter, target) in &annotations {
                let Some(value) = capability.params.get(parameter) else {
                    continue;
                };
                let reference = value
                    .as_str()
                    .ok_or(ModelError::InvalidCapabilityParameters(capability.id.clone()))?;
                let resolves = match target.as_str() {
                    "domain" => self.domains.iter().any(|domain| domain.id == reference),
                    "reference_surface" => self.reference_surfaces.iter().any(|surface| {
                        surface.get("id").and_then(Value::as_str) == Some(reference)
                    }),
                    "figure" => {
                        reference == "figure" && self.figure.kind != "radial_profile_sphere"
                    }
                    _ => return Err(ModelError::InvalidCapabilityContract),
                };
                if !resolves {
                    return Err(ModelError::InvalidCapabilityReference(parameter.clone()));
                }
            }

            let required_kinds = contract
                .get("required_reference_surface_kinds")
                .and_then(Value::as_array)
                .ok_or(ModelError::InvalidCapabilityContract)?;
            for kind in required_kinds {
                let kind = kind.as_str().ok_or(ModelError::InvalidCapabilityContract)?;
                let explicitly_declared = self
                    .reference_surfaces
                    .iter()
                    .any(|surface| surface.get("kind").and_then(Value::as_str) == Some(kind));
                let figure_declared = kind == "figure_surface"
                    && self.figure.kind != "radial_profile_sphere"
                    && annotations.iter().any(|(parameter, target)| {
                        target == "figure"
                            && capability.params.get(parameter).and_then(Value::as_str)
                                == Some("figure")
                    });
                if !explicitly_declared && !figure_declared {
                    return Err(ModelError::InvalidCapabilityReference(kind.to_owned()));
                }
            }
        }
        Ok(())
    }

    fn validate_domains(&self) -> Result<(), ModelError> {
        let mut ids = std::collections::BTreeSet::new();
        for domain in &self.domains {
            let vertical_kind = domain.vertical.get("kind").and_then(Value::as_str);
            let vertical_valid = match domain.topology.as_str() {
                "veyra.topo.dir_cube/1" => vertical_kind == Some("none"),
                "veyra.topo.radial_1d/1" => {
                    vertical_kind == Some("radius")
                        && domain
                            .vertical
                            .get("extent_m")
                            .and_then(Value::as_str)
                            .is_some_and(|extent| validate_positive_decimal(extent).is_ok())
                }
                _ => false,
            };
            if domain.id.is_empty()
                || domain.frame.is_empty()
                || !ids.insert(&domain.id)
                || domain.tile_log2 > 30
                || domain.max_level > 30
                || !vertical_valid
                || !matches!(
                    domain.topology.as_str(),
                    "veyra.topo.dir_cube/1" | "veyra.topo.radial_1d/1"
                )
                || !self.frames.contains_key(&domain.frame)
                || !self.required_features.contains(&domain.topology)
            {
                return Err(ModelError::InvalidDomain);
            }
        }
        Ok(())
    }

    fn validate_surfaces(&self) -> Result<(), ModelError> {
        let mut surfaces = std::collections::BTreeMap::new();
        for surface in &self.reference_surfaces {
            let object = surface.as_object().ok_or(ModelError::InvalidSurface)?;
            let id = object
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .ok_or(ModelError::InvalidSurface)?;
            let kind =
                object.get("kind").and_then(Value::as_str).ok_or(ModelError::InvalidSurface)?;
            if !matches!(kind, "sphere" | "offset_of" | "figure_surface")
                || surfaces.insert(id, surface).is_some()
            {
                return Err(ModelError::InvalidSurface);
            }
            if kind == "sphere" {
                validate_positive_decimal(
                    object
                        .get("radius_m")
                        .and_then(Value::as_str)
                        .ok_or(ModelError::InvalidSurface)?,
                )?;
            }
            if kind == "offset_of" {
                DecimalString::parse(
                    object
                        .get("offset_m")
                        .and_then(Value::as_str)
                        .ok_or(ModelError::InvalidSurface)?,
                )
                .map_err(|_| ModelError::InvalidSurface)?;
            }
        }
        for id in surfaces.keys() {
            let mut seen = std::collections::BTreeSet::new();
            let mut current = *id;
            loop {
                if !seen.insert(current) {
                    return Err(ModelError::SurfaceCycle);
                }
                let surface = surfaces.get(current).ok_or(ModelError::InvalidSurface)?;
                let object = surface.as_object().ok_or(ModelError::InvalidSurface)?;
                if object.get("kind").and_then(Value::as_str) != Some("offset_of") {
                    break;
                }
                current =
                    object.get("base").and_then(Value::as_str).ok_or(ModelError::InvalidSurface)?;
            }
        }
        Ok(())
    }
}

/// Looks up the permanent numeric capability ID from the allocation file embedded at build time.
pub fn capability_numeric_id(id: &str) -> Option<u16> {
    let name = id.strip_prefix("veyra.cap.")?.strip_suffix("/1")?;
    for line in CAPABILITY_IDS.lines() {
        let line = line.trim();
        if line.starts_with('#') || !line.contains('=') {
            continue;
        }
        let (key, value) = line.split_once('=')?;
        if key.trim() == name {
            return u16::from_str_radix(value.trim().trim_start_matches("0x"), 16).ok();
        }
    }
    None
}

fn parse_identity_origin(origin: &Value) -> Result<(UniverseId, ObjectAddress), ModelError> {
    let object = origin.as_object().ok_or(ModelError::InvalidOrigin)?;
    let kind = object.get("kind").and_then(Value::as_str).ok_or(ModelError::InvalidOrigin)?;
    match kind {
        "fixture" => {
            let name =
                object.get("name").and_then(Value::as_str).ok_or(ModelError::InvalidOrigin)?;
            if name.is_empty() {
                return Err(ModelError::InvalidOrigin);
            }
            Ok((UniverseId::fixture_sentinel(), ObjectAddress::Fixture { name: name.to_owned() }))
        }
        "universe" => {
            let universe_text = object
                .get("universe_id")
                .and_then(Value::as_str)
                .ok_or(ModelError::InvalidOrigin)?;
            let universe =
                UniverseId::parse(universe_text).map_err(|_| ModelError::InvalidOrigin)?;
            let address = object.get("address").ok_or(ModelError::InvalidOrigin)?;
            Ok((universe, parse_object_address(address)?))
        }
        other => Err(ModelError::UnsupportedOrigin(other.to_owned())),
    }
}

fn parse_object_address(value: &Value) -> Result<ObjectAddress, ModelError> {
    let object = value.as_object().ok_or(ModelError::InvalidOrigin)?;
    let kind = object.get("kind").and_then(Value::as_str).ok_or(ModelError::InvalidOrigin)?;
    match kind {
        "system_seed" | "free_object" => {
            let region =
                object.get("region").and_then(Value::as_object).ok_or(ModelError::InvalidOrigin)?;
            if region.get("level").is_some_and(|level| level.as_u64() != Some(0)) {
                return Err(ModelError::InvalidOrigin);
            }
            let region = RegionKey {
                level: 0,
                ix: region.get("ix").and_then(Value::as_i64).ok_or(ModelError::InvalidOrigin)?,
                iy: region.get("iy").and_then(Value::as_i64).ok_or(ModelError::InvalidOrigin)?,
                iz: region.get("iz").and_then(Value::as_i64).ok_or(ModelError::InvalidOrigin)?,
            };
            let slot = object
                .get("slot")
                .and_then(Value::as_u64)
                .and_then(|slot| u32::try_from(slot).ok())
                .ok_or(ModelError::InvalidOrigin)?;
            if kind == "system_seed" {
                Ok(ObjectAddress::SystemSeed { region, slot })
            } else {
                Ok(ObjectAddress::FreeObject { region, slot })
            }
        }
        "body_in_system" => {
            let system =
                object.get("system").and_then(Value::as_str).ok_or(ModelError::InvalidOrigin)?;
            let system = ObjectId::parse(system).map_err(|_| ModelError::InvalidOrigin)?;
            let role = object
                .get("role")
                .and_then(Value::as_u64)
                .and_then(|role| u8::try_from(role).ok())
                .ok_or(ModelError::InvalidOrigin)?;
            let ordinal = object
                .get("ordinal")
                .and_then(Value::as_u64)
                .and_then(|ordinal| u32::try_from(ordinal).ok())
                .ok_or(ModelError::InvalidOrigin)?;
            Ok(ObjectAddress::BodyInSystem { system, role, ordinal })
        }
        "fixture" => {
            let name =
                object.get("name").and_then(Value::as_str).ok_or(ModelError::InvalidOrigin)?;
            if name.is_empty() {
                return Err(ModelError::InvalidOrigin);
            }
            Ok(ObjectAddress::Fixture { name: name.to_owned() })
        }
        other => Err(ModelError::UnsupportedOrigin(other.to_owned())),
    }
}

/// Validates all numeric storage parameters as decimal strings without float conversion.
pub fn validate_decimal_storage(value: &Value) -> Result<(), ModelError> {
    match value {
        Value::Object(properties) => {
            for (key, item) in properties {
                if matches!(key.as_str(), "scale" | "offset") {
                    DecimalString::parse(item.as_str().ok_or(ModelError::InvalidDecimal)?)
                        .map_err(|_| ModelError::InvalidDecimal)?;
                } else {
                    validate_decimal_storage(item)?;
                }
            }
            Ok(())
        }
        Value::Array(items) => items.iter().try_for_each(validate_decimal_storage),
        _ => Ok(()),
    }
}

fn validate_positive_decimal(text: &str) -> Result<(), ModelError> {
    let decimal = DecimalString::parse(text).map_err(|_| ModelError::InvalidDecimal)?;
    if decimal.is_negative() || decimal.is_zero() {
        return Err(ModelError::InvalidDecimal);
    }
    Ok(())
}

fn is_known_semantic(semantic: &str) -> bool {
    semantic.strip_prefix("scalar.").is_some_and(|name| !name.is_empty())
        || matches!(semantic, "category" | "feature_ref" | "flags")
}

pub(crate) fn field_dtype(field: &FieldDescriptor) -> Option<DType> {
    match field.storage.get("dtype")?.as_str()? {
        "u8" => Some(DType::U8),
        "i8" => Some(DType::I8),
        "u16" => Some(DType::U16),
        "i16" => Some(DType::I16),
        "u32" => Some(DType::U32),
        "i32" => Some(DType::I32),
        "f32" => Some(DType::F32),
        _ => None,
    }
}

pub(crate) fn field_nodata(field: &FieldDescriptor) -> Result<Option<i64>, ModelError> {
    match field.storage.get("nodata") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .map(Some)
            .ok_or(ModelError::InvalidField),
        Some(_) => Err(ModelError::InvalidField),
    }
}

pub(crate) fn raw_storage_value_fits(dtype: DType, raw: i64) -> bool {
    match dtype {
        DType::U8 => u8::try_from(raw).is_ok(),
        DType::I8 => i8::try_from(raw).is_ok(),
        DType::U16 => u16::try_from(raw).is_ok(),
        DType::I16 => i16::try_from(raw).is_ok(),
        DType::U32 => u32::try_from(raw).is_ok(),
        DType::I32 => i32::try_from(raw).is_ok(),
        // Const values use the low four bytes as the raw IEEE-754 bit pattern.
        DType::F32 => u32::try_from(raw).is_ok(),
        DType::Raw => false,
    }
}

pub(crate) fn temporal_slice_count(field: &FieldDescriptor) -> Option<u16> {
    let temporal = field.temporal.as_object()?;
    match temporal.get("kind")?.as_str()? {
        "static" => Some(1),
        "periodic_slices" => temporal
            .get("count")
            .and_then(Value::as_u64)
            .filter(|count| (1..=u64::from(u16::MAX)).contains(count))
            .and_then(|count| u16::try_from(count).ok()),
        _ => None,
    }
}

fn validate_critical_field_metadata(
    field: &FieldDescriptor,
    domain: &Domain,
) -> Result<(), ModelError> {
    let sampling = field.sampling.as_object().ok_or(ModelError::InvalidField)?;
    for key in sampling.keys() {
        if !matches!(key.as_str(), "interp" | "below_native" | "above_native")
            && !key.starts_with("x-")
        {
            return Err(ModelError::UnsupportedCriticalFieldMetadata(format!("sampling.{key}")));
        }
    }
    let interpolation =
        sampling.get("interp").and_then(Value::as_str).ok_or(ModelError::InvalidField)?;
    let interpolation_supported = match domain.topology.as_str() {
        "veyra.topo.dir_cube/1" => matches!(interpolation, "nearest" | "bilinear"),
        "veyra.topo.radial_1d/1" => interpolation == "linear",
        _ => false,
    };
    if !interpolation_supported {
        return Err(ModelError::UnsupportedCriticalFieldMetadata(format!(
            "sampling.interp={interpolation}"
        )));
    }
    if let Some(value) = sampling.get("below_native")
        && value.as_str() != Some("pyramid")
    {
        return Err(ModelError::UnsupportedCriticalFieldMetadata(
            "sampling.below_native".to_owned(),
        ));
    }
    if let Some(value) = sampling.get("above_native")
        && !matches!(value.as_str(), Some("refine" | "inherit" | "smooth_only" | "none"))
    {
        return Err(ModelError::UnsupportedCriticalFieldMetadata(
            "sampling.above_native".to_owned(),
        ));
    }

    let downsample = field.downsample.as_deref().ok_or(ModelError::InvalidField)?;
    if !matches!(downsample, "mean" | "rms" | "min" | "max" | "sum" | "mode_lowest_tiebreak") {
        return Err(ModelError::UnsupportedCriticalFieldMetadata(format!(
            "downsample={downsample}"
        )));
    }

    let temporal = field.temporal.as_object().ok_or(ModelError::InvalidField)?;
    for key in temporal.keys() {
        if !matches!(
            key.as_str(),
            "kind" | "count" | "period_ref" | "origin_ref" | "reduce_default"
        ) && !key.starts_with("x-")
        {
            return Err(ModelError::UnsupportedCriticalFieldMetadata(format!("temporal.{key}")));
        }
    }
    match temporal.get("kind").and_then(Value::as_str) {
        Some("static") if temporal_slice_count(field) == Some(1) => {
            if temporal.keys().any(|key| !matches!(key.as_str(), "kind") && !key.starts_with("x-"))
            {
                return Err(ModelError::InvalidField);
            }
        }
        Some("periodic_slices") if temporal_slice_count(field).is_some() => {
            for key in ["period_ref", "origin_ref"] {
                if temporal.get(key).and_then(Value::as_str).is_none_or(str::is_empty) {
                    return Err(ModelError::InvalidField);
                }
            }
            if let Some(reduction) = temporal.get("reduce_default") {
                let reduction = reduction.as_str().ok_or(ModelError::InvalidField)?;
                if !matches!(
                    reduction,
                    "mean" | "rms" | "min" | "max" | "sum" | "mode_lowest_tiebreak"
                ) {
                    return Err(ModelError::UnsupportedCriticalFieldMetadata(format!(
                        "temporal.reduce_default={reduction}"
                    )));
                }
            }
        }
        Some("series") => {
            return Err(ModelError::UnsupportedCriticalFieldMetadata(
                "temporal.kind=series".to_owned(),
            ));
        }
        Some(kind) => {
            return Err(ModelError::UnsupportedCriticalFieldMetadata(format!(
                "temporal.kind={kind}"
            )));
        }
        None => return Err(ModelError::InvalidField),
    }
    Ok(())
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

/// Body schema, feature, identity, or field validation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelError {
    /// Body root schema is unsupported.
    UnsupportedSchema,
    /// The body format major version is unsupported.
    UnsupportedMajorVersion(u16),
    /// Required feature is not implemented by this core build.
    UnknownRequiredFeature(String),
    /// A required core feature is absent from the body root.
    MissingRequiredFeature(String),
    /// Object identifier is malformed.
    InvalidObjectId,
    /// Object birth-origin metadata is malformed.
    InvalidOrigin,
    /// The declared origin kind is not supported by this V1 core.
    UnsupportedOrigin(String),
    /// ObjectId does not derive from the declared immutable origin.
    ObjectIdOriginMismatch,
    /// A required physical decimal is invalid or nonpositive.
    InvalidDecimal,
    /// Figure declaration is invalid or unsupported.
    InvalidFigure,
    /// Star-convex radius field is missing or incompatible with its figure.
    InvalidFigureField,
    /// Required feature list contains duplicates.
    InvalidFeatureList,
    /// Capability declarations are malformed or duplicated.
    InvalidCapability,
    /// The embedded V1 capability-contract projection is malformed or cyclic.
    InvalidCapabilityContract,
    /// A known capability's params do not satisfy its declared schema.
    InvalidCapabilityParameters(String),
    /// A capability omits a dependency listed by its schema.
    MissingCapabilityDependency(String),
    /// A capability parameter or required surface does not resolve in this body.
    InvalidCapabilityReference(String),
    /// An unknown critical capability is required.
    UnknownCriticalCapability(String),
    /// Domain declarations are malformed or unsupported.
    InvalidDomain,
    /// Required body-fixed frame is missing or malformed.
    InvalidFrame,
    /// Reference surface declarations are malformed or reference missing bases.
    InvalidSurface,
    /// Reference surfaces contain a cycle.
    SurfaceCycle,
    /// Field ID text is malformed.
    InvalidFieldId,
    /// Field descriptor or field registry is invalid.
    InvalidField,
    /// A baseline attempts to contain a dynamic field.
    DynamicBaselineField,
    /// An unknown field semantic is required to be interpreted.
    UnknownCriticalSemantic(String),
    /// A critical field declares an operator or contract unavailable to this V1 reader.
    UnsupportedCriticalFieldMetadata(String),
    /// Section content or schema is invalid.
    InvalidSection,
    /// Spatial frame validation failed.
    Spatial(SpatialError),
    /// Section hash text is malformed.
    InvalidHash,
}

impl From<crate::ids::IdError> for ModelError {
    fn from(_: crate::ids::IdError) -> Self {
        Self::InvalidObjectId
    }
}

impl FromStr for ObjectIdText {
    type Err = ModelError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        ObjectId::parse(text).map_err(|_| ModelError::InvalidObjectId)?;
        Ok(Self(text.to_owned()))
    }
}

impl fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema => formatter.write_str("unsupported body schema"),
            Self::UnsupportedMajorVersion(major) => {
                write!(formatter, "unsupported body format major {major}")
            }
            Self::UnknownRequiredFeature(id) => write!(formatter, "unknown required feature {id}"),
            Self::MissingRequiredFeature(id) => write!(formatter, "missing required feature {id}"),
            Self::InvalidObjectId => formatter.write_str("invalid object ID"),
            Self::InvalidOrigin => formatter.write_str("object origin is malformed"),
            Self::UnsupportedOrigin(kind) => write!(formatter, "unsupported object origin {kind}"),
            Self::ObjectIdOriginMismatch => {
                formatter.write_str("object ID does not match its declared origin")
            }
            Self::InvalidDecimal => formatter.write_str("invalid or nonpositive decimal string"),
            Self::InvalidFigure => formatter.write_str("invalid or unsupported figure"),
            Self::InvalidFigureField => {
                formatter.write_str("star-convex figure radius field is invalid")
            }
            Self::InvalidFeatureList => formatter.write_str("required feature list is malformed"),
            Self::InvalidCapability => formatter.write_str("capability declaration is malformed"),
            Self::InvalidCapabilityContract => {
                formatter.write_str("embedded capability contracts are invalid")
            }
            Self::InvalidCapabilityParameters(id) => {
                write!(formatter, "invalid params for capability {id}")
            }
            Self::MissingCapabilityDependency(id) => {
                write!(formatter, "capability dependency {id} is missing")
            }
            Self::InvalidCapabilityReference(name) => {
                write!(formatter, "capability reference {name} does not resolve")
            }
            Self::UnknownCriticalCapability(id) => {
                write!(formatter, "unknown critical capability {id}")
            }
            Self::InvalidDomain => {
                formatter.write_str("domain declaration is malformed or unsupported")
            }
            Self::InvalidFrame => formatter.write_str("body-fixed frame declaration is missing"),
            Self::InvalidSurface => formatter.write_str("reference surface declaration is invalid"),
            Self::SurfaceCycle => formatter.write_str("reference surface graph contains a cycle"),
            Self::InvalidFieldId => formatter.write_str("field ID is not canonical"),
            Self::InvalidField => {
                formatter.write_str("field registry contains an invalid descriptor")
            }
            Self::DynamicBaselineField => {
                formatter.write_str("dynamic fields cannot be written into a baseline")
            }
            Self::UnknownCriticalSemantic(id) => {
                write!(formatter, "unknown critical field semantic {id}")
            }
            Self::UnsupportedCriticalFieldMetadata(operator) => {
                write!(formatter, "unsupported critical field metadata {operator}")
            }
            Self::InvalidSection => formatter.write_str("section schema is invalid"),
            Self::Spatial(error) => error.fmt(formatter),
            Self::InvalidHash => formatter.write_str("section hash is malformed"),
        }
    }
}

impl std::error::Error for ModelError {}

/// Parses a `b3:` section hash.
pub fn parse_hash(text: &str) -> Result<Hash32, ModelError> {
    Hash32::parse(text).map_err(|_| ModelError::InvalidHash)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{BodyRoot, FieldId, NamedSectionRef, SectionRef, capability_numeric_id};
    use crate::ids::{ObjectAddress, ObjectId, RegionKey, UniverseId};

    fn fixture_object_id(name: &str) -> String {
        ObjectId::derive(
            UniverseId::fixture_sentinel(),
            &ObjectAddress::Fixture { name: name.to_owned() },
        )
        .unwrap()
        .to_string()
    }

    fn minimal_root(name: &str) -> serde_json::Value {
        json!({
            "schema":"veyra.body/1","format_version":{"major":1,"minor":0},
            "required_features":["veyra.body/1","veyra.canon.jcs/1","veyra.codec.zstd-shuffle2/1"],
            "identity":{"object_id":fixture_object_id(name),"origin":{"kind":"fixture","name":name}},
            "classification":{},"physical":{"gm_m3_s2":"1"},"figure":{"kind":"sphere","radius_m":"1"},
            "frames":{"body_fixed":{"axes":"right-handed"}},"reference_surfaces":[],
            "dynamics":{"descriptor":{"path":"dynamics/descriptor.json","hash":"b3:0000000000000000000000000000000000000000000000000000000000000000"},"origin_keyframe":{"path":"dynamics/origin.json","hash":"b3:0000000000000000000000000000000000000000000000000000000000000000"}},
            "capabilities":[],"domains":[],"codec":"zstd+shuffle2",
            "sections":{"registry":{"path":"registry/fields.json","hash":"b3:0000000000000000000000000000000000000000000000000000000000000000"}},"indexes":{}
        })
    }

    fn capability_root(name: &str, capabilities: serde_json::Value) -> BodyRoot {
        let mut value = minimal_root(name);
        value["required_features"].as_array_mut().unwrap().push(json!("veyra.topo.dir_cube/1"));
        value["capabilities"] = capabilities;
        value["domains"] = json!([{"id":"surface","topology":"veyra.topo.dir_cube/1","frame":"body_fixed","vertical":{"kind":"none"},"tile_log2":2,"max_level":5}]);
        value["reference_surfaces"] = json!([{"id":"datum.mean","kind":"sphere","radius_m":"1"}]);
        serde_json::from_value(value).unwrap()
    }

    fn field_root(name: &str, with_domain: bool) -> BodyRoot {
        let mut value = minimal_root(name);
        value["required_features"].as_array_mut().unwrap().push(json!("veyra.topo.dir_cube/1"));
        let capability_domain = if with_domain { "surface" } else { "other" };
        value["capabilities"] = json!([
            {"id":"veyra.cap.solid_surface/1","params":{"figure_ref":"figure"}},
            {"id":"veyra.cap.topography/1","params":{"reference_surface":"datum.mean","domain":capability_domain}}
        ]);
        value["reference_surfaces"] = json!([{"id":"datum.mean","kind":"sphere","radius_m":"1"}]);
        value["domains"] = if with_domain {
            json!([{"id":"surface","topology":"veyra.topo.dir_cube/1","frame":"body_fixed","vertical":{"kind":"none"},"tile_log2":2,"max_level":5}])
        } else {
            json!([{"id":"other","topology":"veyra.topo.dir_cube/1","frame":"body_fixed","vertical":{"kind":"none"},"tile_log2":2,"max_level":5}])
        };
        serde_json::from_value(value).unwrap()
    }

    fn field_descriptor_json() -> serde_json::Value {
        json!({
            "id":"0x01010001","name":"topography.height_m","capability":"veyra.cap.topography/1",
            "domain":"surface","semantic":"scalar.height","persistence":"invariant",
            "storage":{"dtype":"i16","scale":"0.5","offset":"0"},"native_level":3,
            "temporal":{"kind":"static"},
            "sampling":{"interp":"bilinear","below_native":"pyramid","above_native":"refine"},
            "downsample":"mean","compat":"critical"
        })
    }

    fn star_convex_root(name: &str, include_index: bool) -> serde_json::Value {
        let mut value = minimal_root(name);
        value["required_features"].as_array_mut().unwrap().push(json!("veyra.topo.dir_cube/1"));
        value["figure"] = json!({"kind":"star_convex_radial","radius_field":"figure.radius_m"});
        value["capabilities"] = json!([{
            "id":"veyra.cap.solid_surface/1","params":{"figure_ref":"figure"}
        }]);
        value["domains"] = json!([{"id":"surface","topology":"veyra.topo.dir_cube/1","frame":"body_fixed","vertical":{"kind":"none"},"tile_log2":2,"max_level":5}]);
        if include_index {
            value["indexes"] = json!({"0x01000001":"b3:0000000000000000000000000000000000000000000000000000000000000000"});
        }
        value
    }

    fn radius_field_descriptor() -> serde_json::Value {
        json!({
            "id":"0x01000001","name":"figure.radius_m","capability":"veyra.cap.solid_surface/1",
            "domain":"surface","semantic":"scalar.distance","persistence":"invariant",
            "storage":{"dtype":"u32","scale":"1","offset":"0"},"unit":"m","native_level":3,
            "temporal":{"kind":"static"},"sampling":{"interp":"bilinear"},
            "downsample":"mean","compat":"critical"
        })
    }

    fn registry(fields: Vec<serde_json::Value>) -> super::FieldRegistry {
        serde_json::from_value(json!({"schema":"veyra.field_registry/1","fields":fields})).unwrap()
    }

    #[test]
    fn named_vocab_and_feature_references_preserve_names_while_plain_refs_stay_strict() {
        let vocab: NamedSectionRef = serde_json::from_value(json!({
            "name":"tectonics.crust_type/1","path":"vocab/tectonics.crust_type.json",
            "hash":"b3:0000000000000000000000000000000000000000000000000000000000000000"
        }))
        .unwrap();
        let feature: NamedSectionRef = serde_json::from_value(json!({
            "name":"tectonics.plate","path":"features/tectonics.plate.json",
            "hash":"b3:0000000000000000000000000000000000000000000000000000000000000000"
        }))
        .unwrap();
        assert_eq!(vocab.name, "tectonics.crust_type/1");
        assert_eq!(feature.name, "tectonics.plate");

        let mut root_value = minimal_root("named-section-values");
        root_value["sections"]["vocab"] = json!([vocab.clone()]);
        root_value["sections"]["features"] = json!([feature.clone()]);
        let root: BodyRoot = serde_json::from_value(root_value).unwrap();
        assert_eq!(root.sections.vocab[0].name, "tectonics.crust_type/1");
        assert_eq!(root.sections.features[0].name, "tectonics.plate");

        assert!(
            serde_json::from_value::<NamedSectionRef>(json!({
                "path":"vocab/missing-name.json",
                "hash":"b3:0000000000000000000000000000000000000000000000000000000000000000"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<NamedSectionRef>(json!({
                "name":"vocab/missing-hash","path":"vocab/missing-hash.json"
            }))
            .is_err()
        );
        let empty_name: NamedSectionRef = serde_json::from_value(json!({
            "name":"","path":"vocab/empty-name.json",
            "hash":"b3:0000000000000000000000000000000000000000000000000000000000000000"
        }))
        .unwrap();
        let mut root_value = minimal_root("named-section-empty-name");
        root_value["sections"]["vocab"] = json!([empty_name]);
        let root: BodyRoot = serde_json::from_value(root_value).unwrap();
        assert_eq!(root.validate(), Err(super::ModelError::InvalidSection));

        assert!(
            serde_json::from_value::<SectionRef>(json!({
                "path":"registry/fields.json",
                "hash":"b3:0000000000000000000000000000000000000000000000000000000000000000",
                "unexpected":true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<SectionRef>(json!({
                "path":"registry/fields.json",
                "hash":"b3:0000000000000000000000000000000000000000000000000000000000000000",
                "name":"not-allowed-on-plain-reference"
            }))
            .is_err()
        );
    }

    #[test]
    fn field_ids_use_capability_and_local_parts() {
        let id = FieldId::new(0x0101, 1);
        assert_eq!(id.to_string(), "0x01010001");
        assert_eq!(FieldId::parse("0x01010001").unwrap(), id);
        assert!(FieldId::parse("0X01010001").is_err());
    }

    #[test]
    fn capability_numbers_come_from_the_allocation_file() {
        assert_eq!(capability_numeric_id("veyra.cap.topography/1"), Some(0x0101));
        assert_eq!(capability_numeric_id("veyra.cap.conformance_probe/1"), Some(0x7ffe));
        assert_eq!(capability_numeric_id("veyra.cap.unallocated/1"), None);
    }

    #[test]
    fn root_accepts_zero_capabilities_without_required_domains() {
        let root: BodyRoot = serde_json::from_value(json!({
            "schema":"veyra.body/1",
            "format_version":{"major":1,"minor":0},
            "required_features":["veyra.body/1","veyra.canon.jcs/1","veyra.codec.zstd-shuffle2/1"],
            "identity":{"object_id":fixture_object_id("cb9-minimal-void"),"origin":{"kind":"fixture","name":"cb9-minimal-void"}},
            "classification":{},
            "physical":{"gm_m3_s2":"1"},
            "figure":{"kind":"sphere","radius_m":"1"},
            "frames":{"body_fixed":{"axes":"right-handed"}},
            "reference_surfaces":[],
            "dynamics":{"descriptor":{"path":"dynamics/descriptor.json","hash":"b3:0000000000000000000000000000000000000000000000000000000000000000"},"origin_keyframe":{"path":"dynamics/origin.json","hash":"b3:0000000000000000000000000000000000000000000000000000000000000000"}},
            "capabilities":[],"domains":[],"codec":"zstd+shuffle2",
            "sections":{"registry":{"path":"registry/fields.json","hash":"b3:0000000000000000000000000000000000000000000000000000000000000000"}},"indexes":{}
        })).unwrap();
        assert!(root.validate().is_ok());
        assert!(root.capabilities.is_empty());
    }

    #[test]
    fn refuses_unimplemented_required_features() {
        let mut root: BodyRoot = serde_json::from_value(json!({
            "schema":"veyra.body/1","format_version":{"major":1,"minor":0},
            "required_features":["veyra.body/1","veyra.canon.jcs/1","veyra.codec.zstd-shuffle2/1","veyra.refine.cdetail/1"],
            "identity":{"object_id":fixture_object_id("feature-test"),"origin":{"kind":"fixture","name":"feature-test"}},
            "classification":{},"physical":{"gm_m3_s2":"1"},"figure":{"kind":"sphere","radius_m":"1"},
            "frames":{"body_fixed":{"axes":"right-handed"}},"reference_surfaces":[],
            "dynamics":{"descriptor":{"path":"dynamics/descriptor.json","hash":"b3:0000000000000000000000000000000000000000000000000000000000000000"},"origin_keyframe":{"path":"dynamics/origin.json","hash":"b3:0000000000000000000000000000000000000000000000000000000000000000"}},
            "capabilities":[],"domains":[],"codec":"zstd+shuffle2",
            "sections":{"registry":{"path":"registry/fields.json","hash":"b3:0000000000000000000000000000000000000000000000000000000000000000"}},"indexes":{}
        })).unwrap();
        assert!(
            matches!(root.validate(), Err(super::ModelError::UnknownRequiredFeature(feature)) if feature == "veyra.refine.cdetail/1")
        );
        root.required_features.pop();
        assert!(root.validate().is_ok());
    }

    #[test]
    fn radial_domains_require_their_declared_physical_extent_without_required_capabilities() {
        let root: BodyRoot = serde_json::from_value(json!({
            "schema":"veyra.body/1","format_version":{"major":1,"minor":0},
            "required_features":["veyra.body/1","veyra.canon.jcs/1","veyra.codec.zstd-shuffle2/1","veyra.topo.radial_1d/1"],
            "identity":{"object_id":fixture_object_id("radial-contract"),"origin":{"kind":"fixture","name":"radial-contract"}},
            "classification":{},"physical":{"gm_m3_s2":"1"},"figure":{"kind":"radial_profile_sphere","extent_m":"2"},
            "frames":{"body_fixed":{"axes":"right-handed"}},
            "reference_surfaces":[{"id":"photosphere","kind":"sphere","radius_m":"2"}],
            "dynamics":{"descriptor":{"path":"dynamics/descriptor.json","hash":"b3:0000000000000000000000000000000000000000000000000000000000000000"},"origin_keyframe":{"path":"dynamics/origin.json","hash":"b3:0000000000000000000000000000000000000000000000000000000000000000"}},
            "capabilities":[],"domains":[{"id":"interior","topology":"veyra.topo.radial_1d/1","frame":"body_fixed","vertical":{"kind":"radius","extent_m":"2"},"tile_log2":3,"max_level":5}],
            "codec":"zstd+shuffle2","sections":{"registry":{"path":"registry/fields.json","hash":"b3:0000000000000000000000000000000000000000000000000000000000000000"}},"indexes":{}
        })).unwrap();
        assert!(root.validate().is_ok());
        assert!(root.capabilities.is_empty());
    }

    #[test]
    fn body_identity_must_derive_from_its_fixture_origin() {
        let valid: BodyRoot = serde_json::from_value(minimal_root("identity-valid")).unwrap();
        assert!(valid.validate().is_ok());

        let mut mismatch = minimal_root("identity-mismatch");
        mismatch["identity"]["object_id"] = json!("obj:00000000000000000000000000000001");
        let mismatch: BodyRoot = serde_json::from_value(mismatch).unwrap();
        assert_eq!(mismatch.validate(), Err(super::ModelError::ObjectIdOriginMismatch));
    }

    #[test]
    fn body_identity_rejects_malformed_and_unsupported_origins() {
        let mut malformed = minimal_root("bad-origin");
        malformed["identity"]["origin"] = json!({"kind":"fixture","name":""});
        let malformed: BodyRoot = serde_json::from_value(malformed).unwrap();
        assert_eq!(malformed.validate(), Err(super::ModelError::InvalidOrigin));

        let mut unsupported = minimal_root("unsupported-origin");
        unsupported["identity"]["origin"] = json!({"kind":"recipe","id":"x"});
        let unsupported: BodyRoot = serde_json::from_value(unsupported).unwrap();
        assert_eq!(
            unsupported.validate(),
            Err(super::ModelError::UnsupportedOrigin("recipe".to_owned()))
        );
    }

    #[test]
    fn known_capability_schemas_enforce_params_dependencies_and_references_generically() {
        let solid = json!({"id":"veyra.cap.solid_surface/1","params":{"figure_ref":"figure"}});
        let topography = |params| json!({"id":"veyra.cap.topography/1","params":params});
        let valid = capability_root(
            "capability-contract-valid",
            json!([
                solid.clone(),
                topography(json!({"reference_surface":"datum.mean","domain":"surface"}))
            ]),
        );
        assert!(valid.validate().is_ok());
        let mut required_capability = capability_root(
            "capability-feature-known",
            json!([
                solid.clone(),
                topography(json!({"reference_surface":"datum.mean","domain":"surface"}))
            ]),
        );
        required_capability.required_features.push("veyra.cap.topography/1".to_owned());
        assert!(required_capability.validate().is_ok());
        let mut unknown_capability_feature = required_capability.clone();
        unknown_capability_feature.required_features.push("veyra.cap.future/1".to_owned());
        assert!(matches!(
            unknown_capability_feature.validate(),
            Err(super::ModelError::UnknownRequiredFeature(feature)) if feature == "veyra.cap.future/1"
        ));

        let missing_dependency = capability_root(
            "capability-contract-missing-dependency",
            json!([topography(json!({"reference_surface":"datum.mean","domain":"surface"}))]),
        );
        assert_eq!(
            missing_dependency.validate(),
            Err(super::ModelError::MissingCapabilityDependency(
                "veyra.cap.solid_surface/1".to_owned()
            ))
        );

        let missing_params = capability_root(
            "capability-contract-missing-params",
            json!([solid.clone(), topography(json!({"reference_surface":"datum.mean"}))]),
        );
        assert_eq!(
            missing_params.validate(),
            Err(super::ModelError::InvalidCapabilityParameters(
                "veyra.cap.topography/1".to_owned()
            ))
        );

        for (name, params, expected) in [
            (
                "capability-contract-bad-domain",
                json!({"reference_surface":"datum.mean","domain":"missing"}),
                "domain",
            ),
            (
                "capability-contract-bad-surface",
                json!({"reference_surface":"missing","domain":"surface"}),
                "reference_surface",
            ),
        ] {
            let invalid = capability_root(name, json!([solid.clone(), topography(params)]));
            assert_eq!(
                invalid.validate(),
                Err(super::ModelError::InvalidCapabilityReference(expected.to_owned()))
            );
        }

        let surface_material =
            json!({"id":"veyra.cap.surface_material/1","params":{"domain":"surface"}});
        assert!(
            capability_root(
                "capability-contract-other-capability",
                json!([solid.clone(), surface_material.clone()])
            )
            .validate()
            .is_ok()
        );
        assert_eq!(
            capability_root("capability-contract-other-dependency", json!([surface_material]))
                .validate(),
            Err(super::ModelError::MissingCapabilityDependency(
                "veyra.cap.solid_surface/1".to_owned()
            ))
        );

        let mut stellar_value = minimal_root("capability-required-surface-kind");
        stellar_value["required_features"]
            .as_array_mut()
            .unwrap()
            .push(json!("veyra.topo.radial_1d/1"));
        stellar_value["figure"] = json!({"kind":"radial_profile_sphere","extent_m":"2"});
        stellar_value["capabilities"] = json!([{
            "id":"veyra.cap.stellar_structure/1","params":{"domain":"interior"}
        }]);
        stellar_value["domains"] = json!([{
            "id":"interior","topology":"veyra.topo.radial_1d/1","frame":"body_fixed",
            "vertical":{"kind":"radius","extent_m":"2"},"tile_log2":2,"max_level":5
        }]);
        let stellar_without_surface: BodyRoot =
            serde_json::from_value(stellar_value.clone()).unwrap();
        assert_eq!(
            stellar_without_surface.validate(),
            Err(super::ModelError::InvalidCapabilityReference("sphere".to_owned()))
        );
        stellar_value["reference_surfaces"] =
            json!([{"id":"photosphere","kind":"sphere","radius_m":"2"}]);
        let stellar_with_surface: BodyRoot = serde_json::from_value(stellar_value).unwrap();
        assert!(stellar_with_surface.validate().is_ok());
    }

    #[test]
    fn body_identity_derives_universe_birth_addresses() {
        let universe = UniverseId([0x42; 32]);
        let address = ObjectAddress::SystemSeed {
            region: RegionKey { level: 0, ix: -1, iy: 4, iz: 0 },
            slot: 9,
        };
        let id = ObjectId::derive(universe, &address).unwrap();
        let mut value = minimal_root("universe-origin");
        value["identity"] = json!({
            "object_id":id.to_string(),
            "origin":{
                "kind":"universe","universe_id":universe.to_string(),
                "address":{"kind":"system_seed","region":{"level":0,"ix":-1,"iy":4,"iz":0},"slot":9}
            }
        });
        let root: BodyRoot = serde_json::from_value(value).unwrap();
        assert!(root.validate().is_ok());

        let system = ObjectId([0x11; 16]);
        let address_cases = [
            (
                ObjectAddress::BodyInSystem { system, role: 2, ordinal: 3 },
                json!({"kind":"body_in_system","system":system.to_string(),"role":2,"ordinal":3}),
            ),
            (
                ObjectAddress::FreeObject {
                    region: RegionKey { level: 0, ix: -2, iy: 5, iz: 7 },
                    slot: 11,
                },
                json!({"kind":"free_object","region":{"level":0,"ix":-2,"iy":5,"iz":7},"slot":11}),
            ),
            (
                ObjectAddress::Fixture { name: "nested-fixture".to_owned() },
                json!({"kind":"fixture","name":"nested-fixture"}),
            ),
        ];
        for (index, (address, address_json)) in address_cases.into_iter().enumerate() {
            let id = ObjectId::derive(universe, &address).unwrap();
            let mut value = minimal_root(&format!("universe-address-{index}"));
            value["identity"] = json!({
                "object_id":id.to_string(),
                "origin":{"kind":"universe","universe_id":universe.to_string(),"address":address_json}
            });
            let root: BodyRoot = serde_json::from_value(value).unwrap();
            assert!(root.validate().is_ok(), "address kind {index}");
        }

        let mut malformed = minimal_root("universe-address-level");
        let invalid_id = ObjectId::derive(
            universe,
            &ObjectAddress::SystemSeed {
                region: RegionKey { level: 0, ix: 1, iy: 2, iz: 3 },
                slot: 4,
            },
        )
        .unwrap();
        malformed["identity"] = json!({
            "object_id":invalid_id.to_string(),
            "origin":{
                "kind":"universe","universe_id":universe.to_string(),
                "address":{"kind":"system_seed","region":{"level":1,"ix":1,"iy":2,"iz":3},"slot":4}
            }
        });
        let root: BodyRoot = serde_json::from_value(malformed).unwrap();
        assert_eq!(root.validate(), Err(super::ModelError::InvalidOrigin));
    }

    #[test]
    fn positive_gm_decimal_runtime_rule_rejects_zero_and_negative_forms() {
        for gm in ["1", "0.1", "1e3", "1.25e-3", "1e2147483647", "1e-2147483648"] {
            let mut value = minimal_root("gm-positive");
            value["physical"]["gm_m3_s2"] = json!(gm);
            let root: BodyRoot = serde_json::from_value(value).unwrap();
            assert!(root.validate().is_ok(), "rejected positive GM {gm}");
        }
        for gm in [
            "0",
            "-1",
            "-0",
            "-0.0",
            "-0e3",
            "0.0",
            "0e3",
            "0.0e-3",
            "1e2147483648",
            "1e-2147483649",
            "+1",
        ] {
            let mut value = minimal_root("gm-invalid");
            value["physical"]["gm_m3_s2"] = json!(gm);
            let root: BodyRoot = serde_json::from_value(value).unwrap();
            assert_eq!(root.validate(), Err(super::ModelError::InvalidDecimal), "{gm}");
        }
    }

    #[test]
    fn critical_fields_require_a_declared_domain_but_void_bodies_remain_valid() {
        let void: BodyRoot = serde_json::from_value(minimal_root("field-domain-void")).unwrap();
        assert!(void.validate().is_ok());
        assert!(void.validate_registry(&registry(vec![])).is_ok());

        let root_without_domain = field_root("field-domain-missing", false);
        let error = root_without_domain
            .validate_registry(&registry(vec![field_descriptor_json()]))
            .unwrap_err();
        assert_eq!(error, super::ModelError::InvalidField);

        let root_with_domain = field_root("field-domain-valid", true);
        assert!(
            root_with_domain.validate_registry(&registry(vec![field_descriptor_json()])).is_ok()
        );

        let mut ancillary = field_descriptor_json();
        ancillary["compat"] = json!("ancillary");
        ancillary["semantic"] = json!("x-future.semantic/1");
        ancillary["storage"]["dtype"] = json!("future_dtype");
        ancillary["sampling"]["interp"] = json!("cubic");
        ancillary["temporal"]["kind"] = json!("future_time_mode");
        let root_without_domain = field_root("field-domain-ancillary", false);
        assert!(root_without_domain.validate_registry(&registry(vec![ancillary])).is_ok());
    }

    #[test]
    fn critical_native_level_is_bounded_by_v1_and_its_domain() {
        let root = field_root("field-level-bound", true);
        for level in [3, 5] {
            let mut field = field_descriptor_json();
            field["native_level"] = json!(level);
            assert!(root.validate_registry(&registry(vec![field])).is_ok(), "level {level}");
        }
        for level in [6, 31] {
            let mut field = field_descriptor_json();
            field["native_level"] = json!(level);
            assert_eq!(
                root.validate_registry(&registry(vec![field])),
                Err(super::ModelError::InvalidField),
                "level {level}"
            );
        }
    }

    #[test]
    fn star_convex_figure_resolves_a_stored_cube_radius_field() {
        let root: BodyRoot =
            serde_json::from_value(star_convex_root("star-radius-valid", true)).unwrap();
        assert!(root.validate().is_ok());
        assert!(root.validate_registry(&registry(vec![radius_field_descriptor()])).is_ok());

        let root_without_index: BodyRoot =
            serde_json::from_value(star_convex_root("star-radius-not-indexed", false)).unwrap();
        assert_eq!(
            root_without_index.validate_registry(&registry(vec![radius_field_descriptor()])),
            Err(super::ModelError::InvalidFigureField)
        );

        let root_with_index: BodyRoot =
            serde_json::from_value(star_convex_root("star-radius-missing-field", true)).unwrap();
        assert_eq!(
            root_with_index.validate_registry(&registry(vec![])),
            Err(super::ModelError::InvalidFigureField)
        );

        for (key, value) in
            [("domain", json!("interior")), ("semantic", json!("category")), ("unit", json!("km"))]
        {
            let mut field = radius_field_descriptor();
            field[key] = value;
            assert!(root.validate_registry(&registry(vec![field])).is_err(), "{key}");
        }

        let sphere: BodyRoot = serde_json::from_value(minimal_root("sphere-unaffected")).unwrap();
        assert!(sphere.validate().is_ok());
        assert!(sphere.validate_registry(&registry(vec![])).is_ok());
        let mut radial_value = minimal_root("radial-profile-unaffected");
        radial_value["figure"] = json!({"kind":"radial_profile_sphere","extent_m":"2"});
        let radial: BodyRoot = serde_json::from_value(radial_value).unwrap();
        assert!(radial.validate().is_ok());
        assert!(radial.validate_registry(&registry(vec![])).is_ok());
    }

    #[test]
    fn critical_field_metadata_accepts_v1_contract_and_refuses_unknown_operators() {
        let root = field_root("field-contract-valid", true);
        assert!(root.validate_registry(&registry(vec![field_descriptor_json()])).is_ok());

        let mut radial_root_value = minimal_root("field-contract-radial");
        radial_root_value["required_features"]
            .as_array_mut()
            .unwrap()
            .push(json!("veyra.topo.radial_1d/1"));
        radial_root_value["capabilities"] = json!([{
            "id":"veyra.cap.stellar_structure/1","params":{"domain":"interior"}
        }]);
        radial_root_value["reference_surfaces"] =
            json!([{"id":"photosphere","kind":"sphere","radius_m":"2"}]);
        radial_root_value["domains"] = json!([{"id":"interior","topology":"veyra.topo.radial_1d/1","frame":"body_fixed","vertical":{"kind":"radius","extent_m":"2"},"tile_log2":2,"max_level":5}]);
        let radial_root: BodyRoot = serde_json::from_value(radial_root_value).unwrap();
        let mut radial_field = field_descriptor_json();
        radial_field["id"] = json!("0x01300001");
        radial_field["name"] = json!("stellar.density");
        radial_field["capability"] = json!("veyra.cap.stellar_structure/1");
        radial_field["domain"] = json!("interior");
        radial_field["semantic"] = json!("scalar.density");
        radial_field["storage"]["dtype"] = json!("u32");
        radial_field["sampling"]["interp"] = json!("linear");
        assert!(radial_root.validate_registry(&registry(vec![radial_field])).is_ok());

        for (path, value) in [
            ("sampling.interp", json!("cubic")),
            ("sampling.below_native", json!("nearest")),
            ("sampling.above_native", json!("extrapolate")),
            ("downsample", json!("median")),
            ("temporal.kind", json!("series")),
        ] {
            let mut field = field_descriptor_json();
            let mut parts = path.split('.');
            let first = parts.next().unwrap();
            if let Some(second) = parts.next() {
                field[first][second] = value;
            } else {
                field[first] = value;
            }
            assert!(
                root.validate_registry(&registry(vec![field])).is_err(),
                "accepted unsupported metadata {path}"
            );
        }

        let mut periodic = field_descriptor_json();
        periodic["persistence"] = json!("periodic_mean");
        periodic["temporal"] = json!({
            "kind":"periodic_slices","count":12,"period_ref":"dynamics.orbital_period",
            "origin_ref":"dynamics.periapsis","reduce_default":"mean"
        });
        assert!(root.validate_registry(&registry(vec![periodic])).is_ok());

        let mut incompatible_dtype = field_descriptor_json();
        incompatible_dtype["semantic"] = json!("feature_ref");
        incompatible_dtype["storage"]["dtype"] = json!("f32");
        assert_eq!(
            root.validate_registry(&registry(vec![incompatible_dtype])),
            Err(super::ModelError::InvalidField)
        );

        let mut valid_nodata = field_descriptor_json();
        valid_nodata["storage"]["dtype"] = json!("u8");
        valid_nodata["storage"]["nodata"] = json!(255);
        assert!(root.validate_registry(&registry(vec![valid_nodata])).is_ok());
        let mut invalid_nodata = field_descriptor_json();
        invalid_nodata["storage"]["dtype"] = json!("u8");
        invalid_nodata["storage"]["nodata"] = json!(256);
        assert_eq!(
            root.validate_registry(&registry(vec![invalid_nodata])),
            Err(super::ModelError::InvalidField)
        );
    }
}
