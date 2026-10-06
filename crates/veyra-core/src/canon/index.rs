//! Canonical V1 tile-index blob encoding.

use core::fmt;

use crate::canon::blob::{BlobKind, CanonicalBlob, DType, MAX_CANONICAL_BLOB_BYTES};
use crate::ids::Hash32;
use crate::spatial::{CellKey, DirCube, Radial1d, TileKey, Topology};

/// Cell addressing topology tag stored in an index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum TopologyTag {
    /// `veyra.topo.dir_cube/1`.
    DirCube = 1,
    /// `veyra.topo.radial_1d/1`.
    Radial1d = 2,
}

impl TryFrom<u8> for TopologyTag {
    type Error = IndexError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::DirCube),
            2 => Ok(Self::Radial1d),
            _ => Err(IndexError::InvalidIndex),
        }
    }
}

/// Stored index entry value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexValue {
    /// Content-addressed tile blob.
    Blob(Hash32),
    /// A constant raw value without a blob.
    Const(i64),
}

/// One sorted `(level, key)` tile mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexEntry {
    /// Topology level.
    pub level: u8,
    /// Topology-specific 64-bit key.
    pub key: u64,
    /// Blob reference or constant value.
    pub value: IndexValue,
}

/// Decoded tile index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexBlob {
    /// Field ID encoded in the index payload.
    pub field_id: u32,
    /// Topology key family.
    pub topology: TopologyTag,
    /// Per-body tile size exponent.
    pub tile_log2: u8,
    /// Entries sorted by `(level, key)`.
    pub entries: Vec<IndexEntry>,
}

impl IndexBlob {
    /// Encodes the VYB1 index header and its canonical payload.
    pub fn encode(&self) -> Result<Vec<u8>, IndexError> {
        self.validate()?;
        let entry_count =
            u32::try_from(self.entries.len()).map_err(|_| IndexError::InvalidIndex)?;
        let payload_len = self
            .entries
            .len()
            .checked_mul(48)
            .and_then(|length| length.checked_add(12))
            .ok_or(IndexError::InvalidIndex)?;
        if payload_len.checked_add(16).is_none_or(|size| size > MAX_CANONICAL_BLOB_BYTES) {
            return Err(IndexError::SizeLimitExceeded);
        }
        let mut payload = Vec::with_capacity(payload_len);
        payload.extend_from_slice(&self.field_id.to_le_bytes());
        payload.extend_from_slice(&entry_count.to_le_bytes());
        payload.push(self.topology as u8);
        payload.push(self.tile_log2);
        payload.push(8);
        payload.push(0);
        for entry in &self.entries {
            payload.push(entry.level);
            match entry.value {
                IndexValue::Blob(hash) => {
                    payload.push(0);
                    payload.extend_from_slice(&[0; 6]);
                    payload.extend_from_slice(&entry.key.to_le_bytes());
                    payload.extend_from_slice(&hash.0);
                }
                IndexValue::Const(value) => {
                    payload.push(1);
                    payload.extend_from_slice(&[0; 6]);
                    payload.extend_from_slice(&entry.key.to_le_bytes());
                    payload.extend_from_slice(&value.to_le_bytes());
                    payload.extend_from_slice(&[0; 24]);
                }
            }
        }
        CanonicalBlob::new(BlobKind::Index, DType::Raw, 0, 0, 0, payload)
            .map(|blob| blob.encode())
            .map_err(|_| IndexError::InvalidIndex)
    }

    /// Decodes and validates a canonical VYB1 index blob.
    pub fn decode(bytes: &[u8]) -> Result<Self, IndexError> {
        let blob = CanonicalBlob::decode(bytes).map_err(|_| IndexError::InvalidIndex)?;
        if blob.kind != BlobKind::Index || blob.dtype != DType::Raw || blob.payload.len() < 12 {
            return Err(IndexError::InvalidIndex);
        }
        let payload = &blob.payload;
        let field_id =
            u32::from_le_bytes(payload[0..4].try_into().map_err(|_| IndexError::InvalidIndex)?);
        let count = usize::try_from(u32::from_le_bytes(
            payload[4..8].try_into().map_err(|_| IndexError::InvalidIndex)?,
        ))
        .map_err(|_| IndexError::InvalidIndex)?;
        let topology = TopologyTag::try_from(payload[8])?;
        let tile_log2 = payload[9];
        let expected_len = count
            .checked_mul(48)
            .and_then(|length| length.checked_add(12))
            .ok_or(IndexError::InvalidIndex)?;
        if payload[10] != 8 || payload[11] != 0 || payload.len() != expected_len {
            return Err(IndexError::InvalidIndex);
        }
        let mut entries = Vec::with_capacity(count);
        let (entry_bytes, remainder) = payload[12..].as_chunks::<48>();
        if !remainder.is_empty() {
            return Err(IndexError::InvalidIndex);
        }
        for bytes in entry_bytes {
            if bytes[2..8] != [0; 6] {
                return Err(IndexError::InvalidIndex);
            }
            let level = bytes[0];
            let key =
                u64::from_le_bytes(bytes[8..16].try_into().map_err(|_| IndexError::InvalidIndex)?);
            let value = match bytes[1] {
                0 => IndexValue::Blob(Hash32(
                    bytes[16..48].try_into().map_err(|_| IndexError::InvalidIndex)?,
                )),
                1 => {
                    if bytes[24..48] != [0; 24] {
                        return Err(IndexError::InvalidIndex);
                    }
                    IndexValue::Const(i64::from_le_bytes(
                        bytes[16..24].try_into().map_err(|_| IndexError::InvalidIndex)?,
                    ))
                }
                _ => return Err(IndexError::InvalidIndex),
            };
            entries.push(IndexEntry { level, key, value });
        }
        let index = Self { field_id, topology, tile_log2, entries };
        index.validate()?;
        Ok(index)
    }

    fn validate(&self) -> Result<(), IndexError> {
        if self.tile_log2 > 30
            || self.entries.iter().any(|entry| entry.level > 30)
            || self
                .entries
                .windows(2)
                .any(|pair| (pair[0].level, pair[0].key) >= (pair[1].level, pair[1].key))
        {
            return Err(IndexError::InvalidIndex);
        }
        for entry in &self.entries {
            let tile = TileKey { level: entry.level, address: CellKey(entry.key) };
            let result = match self.topology {
                TopologyTag::DirCube => DirCube.validate_tile_key(tile, self.tile_log2),
                TopologyTag::Radial1d => {
                    Radial1d::default().validate_tile_key(tile, self.tile_log2)
                }
            };
            if result.is_err() {
                return Err(IndexError::InvalidIndex);
            }
        }
        Ok(())
    }
}

/// Index encoding or validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexError {
    /// Header, padding, topology tag, entry flags, order, or length is invalid.
    InvalidIndex,
    /// Canonical index exceeds the shared V1 decompressed content limit.
    SizeLimitExceeded,
}

impl fmt::Display for IndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidIndex => "invalid canonical index blob",
            Self::SizeLimitExceeded => "canonical index exceeds the V1 content size limit",
        })
    }
}

impl std::error::Error for IndexError {}

#[cfg(test)]
mod tests {
    use super::{IndexBlob, IndexEntry, IndexValue, TopologyTag};
    use crate::ids::Hash32;
    use crate::spatial::{DirCube, Radial1d, Topology};

    #[test]
    fn blob_and_const_entries_round_trip() {
        let index = IndexBlob {
            field_id: 0x0101_0001,
            topology: TopologyTag::DirCube,
            tile_log2: 3,
            entries: vec![
                IndexEntry {
                    level: 0,
                    key: DirCube::key(0, 0, 0, 0).unwrap().0,
                    value: IndexValue::Blob(Hash32([3; 32])),
                },
                IndexEntry {
                    level: 0,
                    key: DirCube::key(1, 0, 0, 0).unwrap().0,
                    value: IndexValue::Const(-7),
                },
            ],
        };
        let encoded = index.encode().unwrap();
        assert_eq!(IndexBlob::decode(&encoded).unwrap(), index);
    }

    #[test]
    fn unsorted_entries_are_rejected() {
        let index = IndexBlob {
            field_id: 1,
            topology: TopologyTag::Radial1d,
            tile_log2: 0,
            entries: vec![
                IndexEntry { level: 1, key: 3, value: IndexValue::Const(0) },
                IndexEntry { level: 1, key: 2, value: IndexValue::Const(0) },
            ],
        };
        assert!(index.encode().is_err());
    }

    #[test]
    fn dir_cube_indexes_accept_topology_canonical_tile_ancestors() {
        let topology = DirCube;
        for (level, tile_log2) in [(0, 3), (2, 3), (3, 3), (5, 2), (9, 4)] {
            let cell = DirCube::key(4, 0, 1_u64.min((1_u64 << level) - 1), level).unwrap();
            let tile = topology.tile_key(cell, tile_log2).unwrap();
            let index = IndexBlob {
                field_id: 1,
                topology: TopologyTag::DirCube,
                tile_log2,
                entries: vec![IndexEntry {
                    level,
                    key: tile.address.0,
                    value: IndexValue::Const(0),
                }],
            };
            assert_eq!(IndexBlob::decode(&index.encode().unwrap()).unwrap(), index);
        }
    }

    #[test]
    fn dir_cube_indexes_reject_a_cell_key_instead_of_its_tile_ancestor() {
        let invalid = IndexBlob {
            field_id: 1,
            topology: TopologyTag::DirCube,
            tile_log2: 2,
            entries: vec![IndexEntry {
                level: 5,
                key: DirCube::key(2, 0, 0, 5).unwrap().0,
                value: IndexValue::Const(0),
            }],
        };
        assert_eq!(invalid.encode(), Err(super::IndexError::InvalidIndex));
    }

    #[test]
    fn radial_indexes_accept_only_tile_ordinals_at_the_declared_level() {
        let topology = Radial1d::default();
        for (level, tile_log2, cell_index) in [(2, 3, 3), (3, 3, 7), (5, 2, 19)] {
            let cell = Radial1d::key(level, cell_index).unwrap();
            let tile = topology.tile_key(cell, tile_log2).unwrap();
            let index = IndexBlob {
                field_id: 1,
                topology: TopologyTag::Radial1d,
                tile_log2,
                entries: vec![IndexEntry {
                    level,
                    key: tile.address.0,
                    value: IndexValue::Const(0),
                }],
            };
            assert_eq!(IndexBlob::decode(&index.encode().unwrap()).unwrap(), index);
        }

        let invalid = IndexBlob {
            field_id: 1,
            topology: TopologyTag::Radial1d,
            tile_log2: 2,
            entries: vec![IndexEntry {
                level: 5,
                key: Radial1d::key(5, 8).unwrap().0,
                value: IndexValue::Const(0),
            }],
        };
        assert_eq!(invalid.encode(), Err(super::IndexError::InvalidIndex));
    }
}
