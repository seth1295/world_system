//! Deterministic integer pyramid construction for V1 raster tiles.

use veyra_core::canon::blob::{BlobKind, CanonicalBlob, DType, MAX_CANONICAL_BLOB_BYTES};
use veyra_core::canon::index::TopologyTag;

use crate::WriterError;

/// Builds below-native tiles using the descriptor's integer downsample operator.
pub struct PyramidBuilder;

impl PyramidBuilder {
    /// Reduces one child group. Integer means use rounded half-up division, and mode ties select
    /// the lowest raw value. RMS is the floor of the integer square root of mean squared input.
    pub fn reduce_group(
        dtype: DType,
        operator: &str,
        nodata: Option<i64>,
        values: &[i64],
    ) -> Result<i64, WriterError> {
        if dtype == DType::F32 || dtype == DType::Raw {
            return Err(WriterError::UnsupportedPyramidDType(dtype));
        }
        if values.is_empty() {
            return Err(WriterError::InvalidPyramid);
        }
        let valid: Vec<i64> =
            values.iter().copied().filter(|value| Some(*value) != nodata).collect();
        let result = if valid.is_empty() {
            nodata.ok_or(WriterError::InvalidPyramid)?
        } else {
            match operator {
                "mean" => {
                    round_half_up(valid.iter().map(|value| i128::from(*value)).sum(), valid.len())?
                }
                "rms" => {
                    let squares: u128 =
                        valid.iter().map(|value| i128::from(*value).unsigned_abs().pow(2)).sum();
                    i64::try_from(integer_sqrt(squares / valid.len() as u128))
                        .map_err(|_| WriterError::InvalidPyramid)?
                }
                "min" => *valid.iter().min().ok_or(WriterError::InvalidPyramid)?,
                "max" => *valid.iter().max().ok_or(WriterError::InvalidPyramid)?,
                "sum" => i64::try_from(valid.iter().map(|value| i128::from(*value)).sum::<i128>())
                    .map_err(|_| WriterError::InvalidPyramid)?,
                "mode_lowest_tiebreak" => mode_lowest(&valid),
                _ => return Err(WriterError::InvalidPyramid),
            }
        };
        if !fits(dtype, result) {
            return Err(WriterError::InvalidPyramid);
        }
        Ok(result)
    }

    /// Reduces four direction-cube child tiles or two radial child tiles into a parent tile.
    /// A single child tile is also accepted when its full grid can be halved directly.
    pub fn downsample_tile(
        topology: TopologyTag,
        dtype: DType,
        operator: &str,
        nodata: Option<i64>,
        children: &[CanonicalBlob],
    ) -> Result<CanonicalBlob, WriterError> {
        if dtype == DType::F32 {
            return Err(WriterError::UnsupportedPyramidDType(dtype));
        }
        if dtype == DType::Raw || children.is_empty() {
            return Err(WriterError::InvalidPyramid);
        }
        let slices = children[0].slices;
        for child in children {
            if child.kind != BlobKind::RasterTile || child.dtype != dtype || child.slices != slices
            {
                return Err(WriterError::InvalidPyramid);
            }
            let expected = canonical_raster_payload_len(child, dtype)?;
            if child.payload.len() != expected {
                return Err(WriterError::InvalidPyramid);
            }
        }
        let (width, height) = match topology {
            TopologyTag::DirCube => Self::assemble_cube(children)?,
            TopologyTag::Radial1d => Self::assemble_radial(children)?,
        };
        let reduce_j = usize::from(topology == TopologyTag::DirCube) + 1;
        if width == 0
            || height == 0
            || !width.is_multiple_of(2)
            || (topology == TopologyTag::DirCube && !height.is_multiple_of(2))
            || (topology == TopologyTag::Radial1d && height != 1)
        {
            return Err(WriterError::InvalidPyramid);
        }
        let output_width = width / 2;
        let output_height = height / reduce_j;
        let assembled_bytes = match topology {
            TopologyTag::DirCube => assemble_cube_bytes(children, dtype)?,
            TopologyTag::Radial1d => assemble_radial_bytes(children, dtype)?,
        };
        let mut payload = Vec::new();
        for slice in 0..usize::from(children[0].slices) {
            for j in 0..output_height {
                for i in 0..output_width {
                    let mut group =
                        Vec::with_capacity(if topology == TopologyTag::Radial1d { 2 } else { 4 });
                    for dy in 0..if topology == TopologyTag::Radial1d { 1 } else { 2 } {
                        for dx in 0..2 {
                            let index = (slice * height + j * 2 + dy) * width + i * 2 + dx;
                            group.push(read_integer(dtype, &assembled_bytes, index)?);
                        }
                    }
                    let value = Self::reduce_group(dtype, operator, nodata, &group)?;
                    write_integer(dtype, value, &mut payload)?;
                }
            }
        }
        let dim_i = u16::try_from(output_width).map_err(|_| WriterError::InvalidPyramid)?;
        let dim_j = u16::try_from(output_height).map_err(|_| WriterError::InvalidPyramid)?;
        CanonicalBlob::new(BlobKind::RasterTile, dtype, dim_i, dim_j, children[0].slices, payload)
            .map_err(|_| WriterError::InvalidPyramid)
    }

    fn assemble_cube(children: &[CanonicalBlob]) -> Result<(usize, usize), WriterError> {
        match children.len() {
            1 => {
                let child = &children[0];
                let width = usize::from(child.dim_i);
                let height = usize::from(child.dim_j);
                if width != height {
                    return Err(WriterError::InvalidPyramid);
                }
                Ok((width, height))
            }
            4 => {
                let width = usize::from(children[0].dim_i);
                let height = usize::from(children[0].dim_j);
                if width != height
                    || children.iter().any(|child| {
                        usize::from(child.dim_i) != width || usize::from(child.dim_j) != height
                    })
                {
                    return Err(WriterError::InvalidPyramid);
                }
                Ok((width * 2, height * 2))
            }
            _ => Err(WriterError::InvalidPyramid),
        }
    }

    fn assemble_radial(children: &[CanonicalBlob]) -> Result<(usize, usize), WriterError> {
        match children.len() {
            1 => {
                if children[0].dim_j != 1 {
                    return Err(WriterError::InvalidPyramid);
                }
                Ok((usize::from(children[0].dim_i), 1))
            }
            2 => {
                let width = usize::from(children[0].dim_i);
                if children[0].dim_j != 1
                    || children[1].dim_j != 1
                    || usize::from(children[1].dim_i) != width
                {
                    return Err(WriterError::InvalidPyramid);
                }
                Ok((width * 2, 1))
            }
            _ => Err(WriterError::InvalidPyramid),
        }
    }
}

fn assemble_cube_bytes(children: &[CanonicalBlob], dtype: DType) -> Result<Vec<u8>, WriterError> {
    let bytes_per_value = dtype.width().ok_or(WriterError::InvalidPyramid)?;
    let slices = usize::from(children[0].slices);
    let tile_width = usize::from(children[0].dim_i);
    let tile_height = usize::from(children[0].dim_j);
    if children.len() == 1 {
        return copy_payload(&children[0].payload);
    }
    let width = tile_width * 2;
    let height = tile_height * 2;
    let output_len = checked_payload_len(width, height, slices, bytes_per_value)?;
    let mut output = Vec::new();
    output.try_reserve_exact(output_len).map_err(|_| WriterError::InvalidPyramid)?;
    output.resize(output_len, 0);
    for (quadrant, child) in children.iter().enumerate() {
        let origin_i = (quadrant >> 1) * tile_width;
        let origin_j = (quadrant & 1) * tile_height;
        for slice in 0..slices {
            for j in 0..tile_height {
                for i in 0..tile_width {
                    let source = ((slice * tile_height + j) * tile_width + i) * bytes_per_value;
                    let target =
                        ((slice * height + origin_j + j) * width + origin_i + i) * bytes_per_value;
                    output[target..target + bytes_per_value]
                        .copy_from_slice(&child.payload[source..source + bytes_per_value]);
                }
            }
        }
    }
    Ok(output)
}

fn assemble_radial_bytes(children: &[CanonicalBlob], dtype: DType) -> Result<Vec<u8>, WriterError> {
    if children.len() == 1 {
        return copy_payload(&children[0].payload);
    }
    let width = usize::from(children[0].dim_i);
    let bytes_per_value = dtype.width().ok_or(WriterError::InvalidPyramid)?;
    let slices = usize::from(children[0].slices);
    let slice_bytes = width.checked_mul(bytes_per_value).ok_or(WriterError::InvalidPyramid)?;
    let output_len = slice_bytes
        .checked_mul(slices)
        .and_then(|length| length.checked_mul(children.len()))
        .ok_or(WriterError::InvalidPyramid)?;
    let mut output = Vec::new();
    output.try_reserve_exact(output_len).map_err(|_| WriterError::InvalidPyramid)?;
    for slice in 0..slices {
        for child in children {
            let start = slice.checked_mul(slice_bytes).ok_or(WriterError::InvalidPyramid)?;
            let end = start.checked_add(slice_bytes).ok_or(WriterError::InvalidPyramid)?;
            let source = child.payload.get(start..end).ok_or(WriterError::InvalidPyramid)?;
            output.extend_from_slice(source);
        }
    }
    Ok(output)
}

fn canonical_raster_payload_len(blob: &CanonicalBlob, dtype: DType) -> Result<usize, WriterError> {
    let dim_i = usize::from(blob.dim_i);
    let dim_j = usize::from(blob.dim_j);
    let slices = usize::from(blob.slices);
    let bytes_per_value = dtype.width().ok_or(WriterError::InvalidPyramid)?;
    if dim_i == 0 || dim_j == 0 || slices == 0 {
        return Err(WriterError::InvalidPyramid);
    }
    let length = checked_payload_len(dim_i, dim_j, slices, bytes_per_value)?;
    if length.checked_add(16).is_none_or(|size| size > MAX_CANONICAL_BLOB_BYTES) {
        return Err(WriterError::InvalidPyramid);
    }
    Ok(length)
}

fn checked_payload_len(
    width: usize,
    height: usize,
    slices: usize,
    bytes_per_value: usize,
) -> Result<usize, WriterError> {
    width
        .checked_mul(height)
        .and_then(|length| length.checked_mul(slices))
        .and_then(|length| length.checked_mul(bytes_per_value))
        .ok_or(WriterError::InvalidPyramid)
}

fn copy_payload(payload: &[u8]) -> Result<Vec<u8>, WriterError> {
    let mut output = Vec::new();
    output.try_reserve_exact(payload.len()).map_err(|_| WriterError::InvalidPyramid)?;
    output.extend_from_slice(payload);
    Ok(output)
}

fn read_integer(dtype: DType, payload: &[u8], index: usize) -> Result<i64, WriterError> {
    let width = dtype.width().ok_or(WriterError::InvalidPyramid)?;
    let start = index.checked_mul(width).ok_or(WriterError::InvalidPyramid)?;
    let value = payload.get(start..start + width).ok_or(WriterError::InvalidPyramid)?;
    match dtype {
        DType::U8 => Ok(i64::from(value[0])),
        DType::I8 => Ok(i64::from(value[0] as i8)),
        DType::U16 => Ok(i64::from(u16::from_le_bytes(
            value.try_into().map_err(|_| WriterError::InvalidPyramid)?,
        ))),
        DType::I16 => Ok(i64::from(i16::from_le_bytes(
            value.try_into().map_err(|_| WriterError::InvalidPyramid)?,
        ))),
        DType::U32 => Ok(i64::from(u32::from_le_bytes(
            value.try_into().map_err(|_| WriterError::InvalidPyramid)?,
        ))),
        DType::I32 => Ok(i64::from(i32::from_le_bytes(
            value.try_into().map_err(|_| WriterError::InvalidPyramid)?,
        ))),
        DType::F32 | DType::Raw => Err(WriterError::InvalidPyramid),
    }
}

fn write_integer(dtype: DType, value: i64, output: &mut Vec<u8>) -> Result<(), WriterError> {
    if !fits(dtype, value) {
        return Err(WriterError::InvalidPyramid);
    }
    match dtype {
        DType::U8 => output.push(u8::try_from(value).map_err(|_| WriterError::InvalidPyramid)?),
        DType::I8 => {
            output.push(i8::try_from(value).map_err(|_| WriterError::InvalidPyramid)? as u8)
        }
        DType::U16 => output.extend_from_slice(
            &u16::try_from(value).map_err(|_| WriterError::InvalidPyramid)?.to_le_bytes(),
        ),
        DType::I16 => output.extend_from_slice(
            &i16::try_from(value).map_err(|_| WriterError::InvalidPyramid)?.to_le_bytes(),
        ),
        DType::U32 => output.extend_from_slice(
            &u32::try_from(value).map_err(|_| WriterError::InvalidPyramid)?.to_le_bytes(),
        ),
        DType::I32 => output.extend_from_slice(
            &i32::try_from(value).map_err(|_| WriterError::InvalidPyramid)?.to_le_bytes(),
        ),
        DType::F32 | DType::Raw => return Err(WriterError::InvalidPyramid),
    }
    Ok(())
}

fn fits(dtype: DType, value: i64) -> bool {
    match dtype {
        DType::U8 => u8::try_from(value).is_ok(),
        DType::I8 => i8::try_from(value).is_ok(),
        DType::U16 => u16::try_from(value).is_ok(),
        DType::I16 => i16::try_from(value).is_ok(),
        DType::U32 => u32::try_from(value).is_ok(),
        DType::I32 => i32::try_from(value).is_ok(),
        DType::F32 | DType::Raw => false,
    }
}

fn round_half_up(sum: i128, divisor: usize) -> Result<i64, WriterError> {
    let divisor = i128::try_from(divisor).map_err(|_| WriterError::InvalidPyramid)?;
    let numerator = sum.checked_add(divisor / 2).ok_or(WriterError::InvalidPyramid)?;
    let quotient = numerator.div_euclid(divisor);
    i64::try_from(quotient).map_err(|_| WriterError::InvalidPyramid)
}

fn mode_lowest(values: &[i64]) -> i64 {
    let mut counts = std::collections::BTreeMap::<i64, usize>::new();
    for value in values {
        *counts.entry(*value).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by(|(left_value, left_count), (right_value, right_count)| {
            left_count.cmp(right_count).then_with(|| right_value.cmp(left_value))
        })
        .map(|(value, _)| value)
        .unwrap_or_default()
}

fn integer_sqrt(value: u128) -> u128 {
    if value < 2 {
        return value;
    }
    let mut x = value;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + value / x) / 2;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::PyramidBuilder;
    use crate::WriterError;
    use veyra_core::canon::blob::{BlobKind, CanonicalBlob, DType};
    use veyra_core::canon::index::TopologyTag;

    fn u8_blob(dim_i: u16, dim_j: u16, values: &[u8]) -> CanonicalBlob {
        CanonicalBlob::new(BlobKind::RasterTile, DType::U8, dim_i, dim_j, 1, values.to_vec())
            .unwrap()
    }

    fn u16_values(blob: &CanonicalBlob) -> Vec<u16> {
        let (chunks, remainder) = blob.payload.as_chunks::<2>();
        assert!(remainder.is_empty());
        chunks.iter().map(|bytes| u16::from_le_bytes(*bytes)).collect()
    }

    #[test]
    fn integer_downsample_operators_have_exact_rounding_and_lowest_ties() {
        assert_eq!(
            PyramidBuilder::reduce_group(DType::I16, "mean", None, &[1, 2, 2, 3]).unwrap(),
            2
        );
        assert_eq!(PyramidBuilder::reduce_group(DType::I16, "mean", None, &[-1, 0]).unwrap(), 0);
        assert_eq!(
            PyramidBuilder::reduce_group(DType::U8, "mode_lowest_tiebreak", None, &[2, 1, 2, 1])
                .unwrap(),
            1
        );
        assert_eq!(
            PyramidBuilder::reduce_group(DType::U8, "mean", Some(255), &[255, 4, 255, 8]).unwrap(),
            6
        );
        assert_eq!(
            PyramidBuilder::reduce_group(DType::U8, "mean", Some(255), &[255, 255]).unwrap(),
            255
        );
        assert!(matches!(
            PyramidBuilder::reduce_group(DType::U8, "sum", None, &[255, 255]),
            Err(WriterError::InvalidPyramid)
        ));
        assert!(matches!(
            PyramidBuilder::downsample_tile(
                TopologyTag::DirCube,
                DType::F32,
                "mean",
                None,
                &[CanonicalBlob::new(BlobKind::RasterTile, DType::F32, 2, 2, 1, vec![0; 16])
                    .unwrap()]
            ),
            Err(WriterError::UnsupportedPyramidDType(DType::F32))
        ));
    }

    #[test]
    fn cube_parent_tile_assembles_four_children_in_canonical_quadrant_order() {
        let children = [
            u8_blob(2, 2, &[1, 2, 3, 4]),
            u8_blob(2, 2, &[5, 6, 7, 8]),
            u8_blob(2, 2, &[9, 10, 11, 12]),
            u8_blob(2, 2, &[13, 14, 15, 16]),
        ];
        let parent = PyramidBuilder::downsample_tile(
            TopologyTag::DirCube,
            DType::U8,
            "mean",
            None,
            &children,
        )
        .unwrap();
        assert_eq!((parent.dim_i, parent.dim_j), (2, 2));
        assert_eq!(parent.payload, vec![3, 11, 7, 15]);
    }

    #[test]
    fn radial_parent_tile_reduces_child_pairs_and_preserves_shell_order() {
        let children = [
            CanonicalBlob::new(BlobKind::RasterTile, DType::U16, 2, 1, 1, vec![1, 0, 2, 0])
                .unwrap(),
            CanonicalBlob::new(BlobKind::RasterTile, DType::U16, 2, 1, 1, vec![4, 0, 5, 0])
                .unwrap(),
        ];
        let parent = PyramidBuilder::downsample_tile(
            TopologyTag::Radial1d,
            DType::U16,
            "mean",
            None,
            &children,
        )
        .unwrap();
        assert_eq!((parent.dim_i, parent.dim_j), (2, 1));
        assert_eq!(u16_values(&parent), vec![2, 5]);
    }

    #[test]
    fn malformed_public_cube_children_return_invalid_pyramid() {
        let valid_children = [
            u8_blob(2, 2, &[1, 2, 3, 4]),
            u8_blob(2, 2, &[5, 6, 7, 8]),
            u8_blob(2, 2, &[9, 10, 11, 12]),
            u8_blob(2, 2, &[13, 14, 15, 16]),
        ];
        for child_index in 0..valid_children.len() {
            let mut children = valid_children.clone();
            children[child_index].payload.pop();
            assert!(matches!(
                PyramidBuilder::downsample_tile(
                    TopologyTag::DirCube,
                    DType::U8,
                    "mean",
                    None,
                    &children
                ),
                Err(WriterError::InvalidPyramid)
            ));
        }

        let mut oversized = valid_children.clone();
        oversized[0].payload.push(99);
        assert!(matches!(
            PyramidBuilder::downsample_tile(
                TopologyTag::DirCube,
                DType::U8,
                "mean",
                None,
                &oversized
            ),
            Err(WriterError::InvalidPyramid)
        ));

        let mut inconsistent_dimensions = valid_children;
        inconsistent_dimensions[0].dim_i = 3;
        inconsistent_dimensions[0].payload = vec![1, 2, 3, 4, 5, 6];
        assert!(matches!(
            PyramidBuilder::downsample_tile(
                TopologyTag::DirCube,
                DType::U8,
                "mean",
                None,
                &inconsistent_dimensions
            ),
            Err(WriterError::InvalidPyramid)
        ));
    }

    #[test]
    fn malformed_public_radial_children_return_invalid_pyramid() {
        let valid_children = [u8_blob(2, 1, &[1, 2]), u8_blob(2, 1, &[3, 4])];
        for child_index in 0..valid_children.len() {
            let mut children = valid_children.clone();
            children[child_index].payload.pop();
            assert!(matches!(
                PyramidBuilder::downsample_tile(
                    TopologyTag::Radial1d,
                    DType::U8,
                    "mean",
                    None,
                    &children
                ),
                Err(WriterError::InvalidPyramid)
            ));
        }

        let mut oversized = valid_children.clone();
        oversized[1].payload.push(99);
        assert!(matches!(
            PyramidBuilder::downsample_tile(
                TopologyTag::Radial1d,
                DType::U8,
                "mean",
                None,
                &oversized
            ),
            Err(WriterError::InvalidPyramid)
        ));

        let mut inconsistent_dimensions = valid_children;
        inconsistent_dimensions[1].dim_i = 3;
        inconsistent_dimensions[1].payload.push(5);
        assert!(matches!(
            PyramidBuilder::downsample_tile(
                TopologyTag::Radial1d,
                DType::U8,
                "mean",
                None,
                &inconsistent_dimensions
            ),
            Err(WriterError::InvalidPyramid)
        ));
    }
}
