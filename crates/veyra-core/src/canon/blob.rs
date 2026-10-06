//! Canonical VYB1 blobs and codec separation.

use core::fmt;
use std::io::Read;

use crate::ids::Hash32;

/// Canonical blob kinds assigned by the V1 header.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum BlobKind {
    /// Raster tile.
    RasterTile = 1,
    /// Tile index.
    Index = 2,
    /// Reserved columnar table blob.
    Columnar = 3,
}

impl TryFrom<u8> for BlobKind {
    type Error = BlobError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::RasterTile),
            2 => Ok(Self::Index),
            3 => Ok(Self::Columnar),
            _ => Err(BlobError::InvalidHeader),
        }
    }
}

/// Raster primitive type codes used by VYB1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum DType {
    /// Unsigned 8-bit integer.
    U8 = 1,
    /// Signed 8-bit integer.
    I8 = 2,
    /// Unsigned 16-bit integer.
    U16 = 3,
    /// Signed 16-bit integer.
    I16 = 4,
    /// Unsigned 32-bit integer.
    U32 = 5,
    /// Signed 32-bit integer.
    I32 = 6,
    /// IEEE-754 32-bit storage bits.
    F32 = 7,
    /// No raster dtype; used by index and opaque blobs.
    Raw = 0,
}

impl TryFrom<u8> for DType {
    type Error = BlobError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Raw),
            1 => Ok(Self::U8),
            2 => Ok(Self::I8),
            3 => Ok(Self::U16),
            4 => Ok(Self::I16),
            5 => Ok(Self::U32),
            6 => Ok(Self::I32),
            7 => Ok(Self::F32),
            _ => Err(BlobError::InvalidHeader),
        }
    }
}

impl DType {
    /// Returns the byte width of a raster value.
    pub const fn width(self) -> Option<usize> {
        match self {
            Self::U8 | Self::I8 => Some(1),
            Self::U16 | Self::I16 => Some(2),
            Self::U32 | Self::I32 | Self::F32 => Some(4),
            Self::Raw => None,
        }
    }
}

/// A decoded canonical blob.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalBlob {
    /// Header kind.
    pub kind: BlobKind,
    /// Raster storage dtype or Raw for non-raster content.
    pub dtype: DType,
    /// First logical dimension.
    pub dim_i: u16,
    /// Second logical dimension.
    pub dim_j: u16,
    /// Number of slices.
    pub slices: u16,
    /// Canonical payload bytes.
    pub payload: Vec<u8>,
}

impl CanonicalBlob {
    /// Constructs a V1 blob and validates its dimensions.
    pub fn new(
        kind: BlobKind,
        dtype: DType,
        dim_i: u16,
        dim_j: u16,
        slices: u16,
        payload: Vec<u8>,
    ) -> Result<Self, BlobError> {
        let blob = Self { kind, dtype, dim_i, dim_j, slices, payload };
        blob.validate()?;
        Ok(blob)
    }

    /// Serializes the 16-byte VYB1 header followed by payload.
    pub fn encode(&self) -> Vec<u8> {
        let mut output = Vec::with_capacity(16 + self.payload.len());
        output.extend_from_slice(b"VYB1");
        output.push(self.kind as u8);
        output.push(self.dtype as u8);
        output.push(0);
        output.push(0);
        output.extend_from_slice(&self.dim_i.to_le_bytes());
        output.extend_from_slice(&self.dim_j.to_le_bytes());
        output.extend_from_slice(&self.slices.to_le_bytes());
        output.extend_from_slice(&[0, 0]);
        output.extend_from_slice(&self.payload);
        output
    }

    /// Parses and validates canonical VYB1 bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, BlobError> {
        if bytes.len() < 16
            || &bytes[..4] != b"VYB1"
            || bytes[6] != 0
            || bytes[7] != 0
            || bytes[14..16] != [0, 0]
        {
            return Err(BlobError::InvalidHeader);
        }
        let blob = Self {
            kind: BlobKind::try_from(bytes[4])?,
            dtype: DType::try_from(bytes[5])?,
            dim_i: u16::from_le_bytes([bytes[8], bytes[9]]),
            dim_j: u16::from_le_bytes([bytes[10], bytes[11]]),
            slices: u16::from_le_bytes([bytes[12], bytes[13]]),
            payload: bytes[16..].to_vec(),
        };
        blob.validate()?;
        Ok(blob)
    }

    /// Hashes the canonical uncompressed representation.
    pub fn id(&self) -> Hash32 {
        crate::canon::hash::hash(&self.encode())
    }

    fn validate(&self) -> Result<(), BlobError> {
        if self.kind == BlobKind::RasterTile {
            if self.dim_i == 0 || self.dim_j == 0 || self.slices == 0 {
                return Err(BlobError::InvalidDimensions);
            }
            let width = self.dtype.width().ok_or(BlobError::InvalidHeader)?;
            let expected = usize::from(self.dim_i)
                .checked_mul(usize::from(self.dim_j))
                .and_then(|value| value.checked_mul(usize::from(self.slices)))
                .and_then(|value| value.checked_mul(width))
                .ok_or(BlobError::InvalidDimensions)?;
            if expected != self.payload.len() {
                return Err(BlobError::InvalidDimensions);
            }
        }
        Ok(())
    }
}

/// Reorders bytes into two lanes before compression.
pub fn shuffle2(canonical: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(canonical.len());
    output.extend(canonical.iter().step_by(2));
    output.extend(canonical.iter().skip(1).step_by(2));
    output
}

/// Reverses [`shuffle2`].
pub fn unshuffle2(shuffled: &[u8]) -> Vec<u8> {
    let even_count = shuffled.len().div_ceil(2);
    let (even, odd) = shuffled.split_at(even_count);
    let mut output = vec![0; shuffled.len()];
    for (index, byte) in even.iter().enumerate() {
        output[index * 2] = *byte;
    }
    for (index, byte) in odd.iter().enumerate() {
        output[index * 2 + 1] = *byte;
    }
    output
}

/// Decodes one Zstandard frame and restores canonical bytes from the shuffled payload.
pub fn decode_zstd_shuffle2(encoded: &[u8]) -> Result<Vec<u8>, BlobError> {
    let cursor = std::io::Cursor::new(encoded);
    let mut decoder =
        ruzstd::decoding::StreamingDecoder::new(cursor).map_err(|_| BlobError::Codec)?;
    let mut shuffled = Vec::new();
    decoder.read_to_end(&mut shuffled).map_err(|_| BlobError::Codec)?;
    Ok(unshuffle2(&shuffled))
}

/// Canonical blob or codec error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlobError {
    /// Header bytes, reserved bits, kind, or dtype are invalid.
    InvalidHeader,
    /// Header dimensions do not agree with payload length.
    InvalidDimensions,
    /// Zstandard data is invalid or cannot be decoded.
    Codec,
}

impl fmt::Display for BlobError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidHeader => "invalid VYB1 header",
            Self::InvalidDimensions => "VYB1 dimensions do not match payload length",
            Self::Codec => "invalid zstd-shuffle2 codec data",
        })
    }
}

impl std::error::Error for BlobError {}

#[cfg(test)]
mod tests {
    use super::{BlobKind, CanonicalBlob, DType, decode_zstd_shuffle2, shuffle2, unshuffle2};

    #[test]
    fn header_round_trip_and_shuffle_round_trip() {
        let blob = CanonicalBlob::new(BlobKind::RasterTile, DType::I16, 2, 1, 1, vec![1, 0, 2, 0])
            .unwrap();
        let canonical = blob.encode();
        assert_eq!(CanonicalBlob::decode(&canonical).unwrap(), blob);
        assert_eq!(unshuffle2(&shuffle2(&canonical)), canonical);
        assert_eq!(unshuffle2(&shuffle2(&[1, 2, 3, 4, 5])), [1, 2, 3, 4, 5]);
    }

    #[test]
    fn compressed_bytes_do_not_define_blob_identity() {
        let canonical = CanonicalBlob::new(BlobKind::RasterTile, DType::U8, 3, 1, 1, vec![9, 8, 7])
            .unwrap()
            .encode();
        let shuffled = shuffle2(&canonical);
        let compressed = zstd::stream::encode_all(shuffled.as_slice(), 3).unwrap();
        assert_eq!(decode_zstd_shuffle2(&compressed).unwrap(), canonical);
        assert_ne!(compressed, canonical);
    }

    #[test]
    fn rejects_reserved_header_bits_and_wrong_payload_extent() {
        let blob = CanonicalBlob::new(BlobKind::RasterTile, DType::U8, 1, 1, 1, vec![9]).unwrap();
        let mut bad_flags = blob.encode();
        bad_flags[6] = 1;
        assert!(CanonicalBlob::decode(&bad_flags).is_err());
        let mut bad_shape = blob.encode();
        bad_shape[8] = 2;
        assert!(CanonicalBlob::decode(&bad_shape).is_err());
    }
}
