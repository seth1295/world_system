//! Canonical identifiers and birth-address encoding.

use core::fmt;
use core::str::FromStr;

use crate::canon::jcs;

/// A BLAKE3-256 digest.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Hash32(pub [u8; 32]);

impl Hash32 {
    /// Parses the canonical `b3:<64 lowercase hex>` form.
    pub fn parse(text: &str) -> Result<Self, IdError> {
        let hex = text.strip_prefix("b3:").ok_or(IdError::BadTextForm)?;
        let bytes = decode_hex::<32>(hex)?;
        Ok(Self(bytes))
    }

    /// Returns the canonical `b3:<64 lowercase hex>` form.
    pub fn text(self) -> String {
        format!("b3:{}", encode_hex(&self.0))
    }
}

impl fmt::Display for Hash32 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "b3:{}", encode_hex(&self.0))
    }
}

/// A universe's canonical 256-bit identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UniverseId(pub [u8; 32]);

impl UniverseId {
    /// Hashes canonical JSON to produce a universe identity.
    pub fn from_json(json: &[u8]) -> Result<Self, IdError> {
        let canonical = jcs::canonicalize_json(json).map_err(|_| IdError::InvalidJson)?;
        Ok(Self(*blake3::hash(&canonical).as_bytes()))
    }

    /// Returns the fixture sentinel identity specified by the architecture.
    pub fn fixture_sentinel() -> Self {
        Self(*blake3::hash(b"uni:fixture").as_bytes())
    }

    /// Parses `uni:b3:<64 lowercase hex>`.
    pub fn parse(text: &str) -> Result<Self, IdError> {
        let hex = text.strip_prefix("uni:b3:").ok_or(IdError::BadTextForm)?;
        Ok(Self(decode_hex::<32>(hex)?))
    }

    /// Returns `uni:b3:<64 lowercase hex>`.
    pub fn text(self) -> String {
        format!("uni:b3:{}", encode_hex(&self.0))
    }
}

impl fmt::Display for UniverseId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "uni:b3:{}", encode_hex(&self.0))
    }
}

/// A stable 128-bit identity for a system or body.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ObjectId(pub [u8; 16]);

impl ObjectId {
    /// Derives an object identity from its universe and immutable birth address.
    pub fn derive(universe: UniverseId, address: &ObjectAddress) -> Result<Self, IdError> {
        let address = address.encode()?;
        let mut hasher = blake3::Hasher::new_derive_key("veyra.object.v1");
        hasher.update(&universe.0);
        hasher.update(&address);
        let digest = hasher.finalize();
        let mut id = [0_u8; 16];
        id.copy_from_slice(&digest.as_bytes()[..16]);
        Ok(Self(id))
    }

    /// Parses `obj:<32 lowercase hex>`.
    pub fn parse(text: &str) -> Result<Self, IdError> {
        let hex = text.strip_prefix("obj:").ok_or(IdError::BadTextForm)?;
        Ok(Self(decode_hex::<16>(hex)?))
    }

    /// Returns `obj:<32 lowercase hex>`.
    pub fn text(self) -> String {
        format!("obj:{}", encode_hex(&self.0))
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "obj:{}", encode_hex(&self.0))
    }
}

impl FromStr for ObjectId {
    type Err = IdError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

/// An integer cell in the universe seeding lattice.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RegionKey {
    /// Aggregate level. Object births use level zero.
    pub level: u8,
    /// Signed X coordinate.
    pub ix: i64,
    /// Signed Y coordinate.
    pub iy: i64,
    /// Signed Z coordinate.
    pub iz: i64,
}

/// The immutable address at which an object is born.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectAddress {
    /// A system slot in a level-zero seeding cell.
    SystemSeed { region: RegionKey, slot: u32 },
    /// A body role and ordinal in its birth system.
    BodyInSystem { system: ObjectId, role: u8, ordinal: u32 },
    /// A free object slot in a level-zero seeding cell.
    FreeObject { region: RegionKey, slot: u32 },
    /// A named deterministic fixture.
    Fixture { name: String },
}

impl ObjectAddress {
    /// Encodes an address using its V1 tag, unsigned LEB128, and zigzag signed integers.
    pub fn encode(&self) -> Result<Vec<u8>, IdError> {
        let mut output = Vec::new();
        match self {
            Self::SystemSeed { region, slot } => {
                output.push(1);
                encode_region(*region, &mut output)?;
                encode_uleb128(u64::from(*slot), &mut output);
            }
            Self::BodyInSystem { system, role, ordinal } => {
                output.push(2);
                output.extend_from_slice(&system.0);
                output.push(*role);
                encode_uleb128(u64::from(*ordinal), &mut output);
            }
            Self::FreeObject { region, slot } => {
                output.push(3);
                encode_region(*region, &mut output)?;
                encode_uleb128(u64::from(*slot), &mut output);
            }
            Self::Fixture { name } => {
                if name.is_empty() {
                    return Err(IdError::BadAddress);
                }
                output.push(4);
                encode_uleb128(name.len() as u64, &mut output);
                output.extend_from_slice(name.as_bytes());
            }
        }
        Ok(output)
    }

    /// Decodes a complete canonical V1 address.
    pub fn decode(bytes: &[u8]) -> Result<Self, IdError> {
        let (&tag, rest) = bytes.split_first().ok_or(IdError::BadAddress)?;
        let mut cursor = Cursor::new(rest);
        let result = match tag {
            1 => {
                let region = decode_region(&mut cursor)?;
                let slot = u32::try_from(cursor.uleb()?).map_err(|_| IdError::BadAddress)?;
                Self::SystemSeed { region, slot }
            }
            2 => {
                let mut system = [0_u8; 16];
                system.copy_from_slice(cursor.take(16)?);
                let role = cursor.byte()?;
                let ordinal = u32::try_from(cursor.uleb()?).map_err(|_| IdError::BadAddress)?;
                Self::BodyInSystem { system: ObjectId(system), role, ordinal }
            }
            3 => {
                let region = decode_region(&mut cursor)?;
                let slot = u32::try_from(cursor.uleb()?).map_err(|_| IdError::BadAddress)?;
                Self::FreeObject { region, slot }
            }
            4 => {
                let length = usize::try_from(cursor.uleb()?).map_err(|_| IdError::BadAddress)?;
                let name =
                    core::str::from_utf8(cursor.take(length)?).map_err(|_| IdError::BadAddress)?;
                if name.is_empty() {
                    return Err(IdError::BadAddress);
                }
                Self::Fixture { name: name.to_owned() }
            }
            _ => return Err(IdError::BadAddress),
        };
        if !cursor.is_empty() || result.encode()? != bytes {
            return Err(IdError::BadAddress);
        }
        Ok(result)
    }
}

fn encode_region(region: RegionKey, output: &mut Vec<u8>) -> Result<(), IdError> {
    if region.level != 0 {
        return Err(IdError::BadAddress);
    }
    encode_uleb128(zigzag(region.ix), output);
    encode_uleb128(zigzag(region.iy), output);
    encode_uleb128(zigzag(region.iz), output);
    Ok(())
}

fn decode_region(cursor: &mut Cursor<'_>) -> Result<RegionKey, IdError> {
    let region = RegionKey {
        level: 0,
        ix: unzigzag(cursor.uleb()?),
        iy: unzigzag(cursor.uleb()?),
        iz: unzigzag(cursor.uleb()?),
    };
    if region.level != 0 {
        return Err(IdError::BadAddress);
    }
    Ok(region)
}

fn zigzag(value: i64) -> u64 {
    ((value as u64) << 1) ^ ((value >> 63) as u64)
}

fn unzigzag(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}

fn encode_uleb128(mut value: u64, output: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        output.push(if value == 0 { byte } else { byte | 0x80 });
        if value == 0 {
            break;
        }
    }
}

#[derive(Clone, Copy)]
struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn byte(&mut self) -> Result<u8, IdError> {
        let value = *self.bytes.get(self.position).ok_or(IdError::BadAddress)?;
        self.position += 1;
        Ok(value)
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], IdError> {
        let end = self.position.checked_add(length).ok_or(IdError::BadAddress)?;
        let value = self.bytes.get(self.position..end).ok_or(IdError::BadAddress)?;
        self.position = end;
        Ok(value)
    }

    fn uleb(&mut self) -> Result<u64, IdError> {
        let start = self.position;
        let mut value = 0_u64;
        for shift in (0..70).step_by(7) {
            let byte = self.byte()?;
            if shift == 63 && byte & 0x7e != 0 {
                return Err(IdError::BadAddress);
            }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                let mut canonical = Vec::new();
                encode_uleb128(value, &mut canonical);
                if canonical.as_slice() != &self.bytes[start..self.position] {
                    return Err(IdError::BadAddress);
                }
                return Ok(value);
            }
        }
        Err(IdError::BadAddress)
    }

    const fn is_empty(self) -> bool {
        self.position == self.bytes.len()
    }
}

/// Identifier parsing and encoding failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdError {
    /// Text does not use the canonical identifier prefix or form.
    BadTextForm,
    /// Hexadecimal text has invalid length or digits.
    BadHex,
    /// Address bytes are malformed or noncanonical.
    BadAddress,
    /// JSON could not be canonicalized for an identity.
    InvalidJson,
}

impl fmt::Display for IdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::BadTextForm => "identifier text is not canonical",
            Self::BadHex => "identifier hex is not canonical",
            Self::BadAddress => "object address is malformed or noncanonical",
            Self::InvalidJson => "JSON cannot be used for an identity",
        })
    }
}

impl std::error::Error for IdError {}

fn decode_hex<const N: usize>(text: &str) -> Result<[u8; N], IdError> {
    if text.len() != N * 2 {
        return Err(IdError::BadHex);
    }
    let mut output = [0_u8; N];
    let bytes = text.as_bytes();
    let (pairs, remainder) = bytes.as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(IdError::BadHex);
    }
    for (index, pair) in pairs.iter().enumerate() {
        let high = lower_hex(pair[0]).ok_or(IdError::BadHex)?;
        let low = lower_hex(pair[1]).ok_or(IdError::BadHex)?;
        output[index] = high << 4 | low;
    }
    Ok(output)
}

fn lower_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{ObjectAddress, ObjectId, RegionKey, UniverseId};

    #[test]
    fn universe_identity_uses_canonical_json_and_round_trips_text() {
        let left = UniverseId::from_json(br#"{"b":2,"a":1}"#).unwrap();
        let right = UniverseId::from_json(br#"{"a":1,"b":2}"#).unwrap();
        assert_eq!(left, right);
        assert_eq!(UniverseId::parse(&left.to_string()).unwrap(), left);
    }

    #[test]
    fn address_round_trip_and_rejects_overlong_varint() {
        let address = ObjectAddress::SystemSeed {
            region: RegionKey { level: 0, ix: -1, iy: 64, iz: 0 },
            slot: 300,
        };
        let bytes = address.encode().unwrap();
        assert_eq!(ObjectAddress::decode(&bytes).unwrap(), address);
        assert!(ObjectAddress::decode(&[4, 0x81, 0, b'x']).is_err());
    }

    #[test]
    fn system_birth_address_uses_implicit_level_zero_and_minimal_varints() {
        let address = ObjectAddress::SystemSeed {
            region: RegionKey { level: 0, ix: -1, iy: 64, iz: 0 },
            slot: 300,
        };
        let bytes = address.encode().unwrap();
        assert_eq!(bytes, [1, 1, 0x80, 1, 0, 0xac, 2]);
        assert_eq!(ObjectAddress::decode(&bytes).unwrap(), address);
        let invalid_region = ObjectAddress::FreeObject {
            region: RegionKey { level: 1, ix: 0, iy: 0, iz: 0 },
            slot: 0,
        };
        assert!(invalid_region.encode().is_err());
        assert!(ObjectAddress::Fixture { name: String::new() }.encode().is_err());
    }

    #[test]
    fn object_id_is_stable_and_has_canonical_text() {
        let universe = UniverseId([7; 32]);
        let address = ObjectAddress::Fixture { name: "cb0-addressing".to_owned() };
        let id = ObjectId::derive(universe, &address).unwrap();
        assert_eq!(ObjectId::parse(&id.to_string()).unwrap(), id);
        assert_eq!(id.to_string().len(), 36);
    }
}
