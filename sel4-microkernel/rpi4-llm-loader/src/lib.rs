#![no_std]

use core::fmt;
use core::ops::Range;

use verus_builtin_macros::verus;

pub const GGUF_MAGIC: [u8; 4] = *b"GGUF";
pub const GGUF_VERSION: u32 = 3;
pub const GGML_TYPE_F32: u32 = 0;
pub const MAX_TENSORS: usize = 8;
pub const MAX_DIMENSIONS: usize = 4;
pub const MAX_NAME_LEN: usize = 64;
pub const DEFAULT_ALIGNMENT: usize = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TensorDescriptor {
    pub name_hash: u64,
    pub dimensions: [u64; MAX_DIMENSIONS],
    pub dimension_count: u32,
    pub ggml_type: u32,
    pub data_offset: usize,
    pub byte_len: usize,
}

impl TensorDescriptor {
    pub fn range(&self) -> Range<usize> {
        self.data_offset..self.data_offset + self.byte_len
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelDescriptor {
    pub tensors: [TensorDescriptor; MAX_TENSORS],
    pub tensor_count: usize,
    pub data_start: usize,
}

impl Default for ModelDescriptor {
    fn default() -> Self {
        Self {
            tensors: [TensorDescriptor::default(); MAX_TENSORS],
            tensor_count: 0,
            data_start: 0,
        }
    }
}

impl ModelDescriptor {
    pub fn tensor(&self, index: usize) -> Option<&TensorDescriptor> {
        self.tensors.get(index).filter(|_| index < self.tensor_count)
    }

    pub fn find(&self, name: &[u8]) -> Option<&TensorDescriptor> {
        let hash = fnv1a64(name);
        self.tensors[..self.tensor_count]
            .iter()
            .find(|tensor| tensor.name_hash == hash)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    Truncated,
    InvalidMagic,
    UnsupportedVersion(u32),
    MetadataUnsupported(u64),
    TooManyTensors(u64),
    InvalidNameLength(u64),
    InvalidUtf8Name,
    InvalidDimensionCount(u32),
    ZeroDimension,
    UnsupportedTensorType(u32),
    ArithmeticOverflow,
    MisalignedTensorOffset(u64),
    TensorOutOfBounds,
    OverlappingTensors,
    TrailingHeaderPastData,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => write!(f, "truncated GGUF input"),
            Self::InvalidMagic => write!(f, "invalid GGUF magic"),
            Self::UnsupportedVersion(version) => write!(f, "unsupported GGUF version {version}"),
            Self::MetadataUnsupported(count) => {
                write!(f, "metadata entries are not supported in the bounded loader ({count})")
            }
            Self::TooManyTensors(count) => write!(f, "too many tensors ({count})"),
            Self::InvalidNameLength(length) => write!(f, "invalid tensor-name length {length}"),
            Self::InvalidUtf8Name => write!(f, "tensor name is not UTF-8"),
            Self::InvalidDimensionCount(count) => write!(f, "invalid dimension count {count}"),
            Self::ZeroDimension => write!(f, "tensor dimension is zero"),
            Self::UnsupportedTensorType(kind) => write!(f, "unsupported GGML tensor type {kind}"),
            Self::ArithmeticOverflow => write!(f, "size arithmetic overflow"),
            Self::MisalignedTensorOffset(offset) => {
                write!(f, "tensor offset {offset} is not 32-byte aligned")
            }
            Self::TensorOutOfBounds => write!(f, "tensor data is outside the GGUF buffer"),
            Self::OverlappingTensors => write!(f, "tensor data ranges overlap"),
            Self::TrailingHeaderPastData => write!(f, "tensor header overlaps data section"),
        }
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], LoadError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(LoadError::ArithmeticOverflow)?;
        let value = self.bytes.get(self.offset..end).ok_or(LoadError::Truncated)?;
        self.offset = end;
        Ok(value)
    }

    fn u32(&mut self) -> Result<u32, LoadError> {
        let bytes: [u8; 4] = self.take(4)?.try_into().map_err(|_| LoadError::Truncated)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn u64(&mut self) -> Result<u64, LoadError> {
        let bytes: [u8; 8] = self.take(8)?.try_into().map_err(|_| LoadError::Truncated)?;
        Ok(u64::from_le_bytes(bytes))
    }
}

pub fn parse(bytes: &[u8]) -> Result<ModelDescriptor, LoadError> {
    let mut cursor = Cursor::new(bytes);
    if cursor.take(4)? != GGUF_MAGIC {
        return Err(LoadError::InvalidMagic);
    }
    let version = cursor.u32()?;
    if version != GGUF_VERSION {
        return Err(LoadError::UnsupportedVersion(version));
    }

    let tensor_count_u64 = cursor.u64()?;
    let metadata_count = cursor.u64()?;
    if metadata_count != 0 {
        return Err(LoadError::MetadataUnsupported(metadata_count));
    }
    if tensor_count_u64 > MAX_TENSORS as u64 {
        return Err(LoadError::TooManyTensors(tensor_count_u64));
    }
    let tensor_count = tensor_count_u64 as usize;

    let mut descriptor = ModelDescriptor::default();
    descriptor.tensor_count = tensor_count;
    let mut relative_offsets = [0u64; MAX_TENSORS];

    for index in 0..tensor_count {
        let name_len = cursor.u64()?;
        if name_len == 0 || name_len > MAX_NAME_LEN as u64 {
            return Err(LoadError::InvalidNameLength(name_len));
        }
        let name = cursor.take(name_len as usize)?;
        core::str::from_utf8(name).map_err(|_| LoadError::InvalidUtf8Name)?;

        let dimension_count = cursor.u32()?;
        if dimension_count == 0 || dimension_count as usize > MAX_DIMENSIONS {
            return Err(LoadError::InvalidDimensionCount(dimension_count));
        }
        let mut dimensions = [0u64; MAX_DIMENSIONS];
        let mut elements = 1u64;
        for dimension in dimensions.iter_mut().take(dimension_count as usize) {
            *dimension = cursor.u64()?;
            if *dimension == 0 {
                return Err(LoadError::ZeroDimension);
            }
            elements = elements
                .checked_mul(*dimension)
                .ok_or(LoadError::ArithmeticOverflow)?;
        }

        let ggml_type = cursor.u32()?;
        if ggml_type != GGML_TYPE_F32 {
            return Err(LoadError::UnsupportedTensorType(ggml_type));
        }
        let relative_offset = cursor.u64()?;
        if relative_offset % DEFAULT_ALIGNMENT as u64 != 0 {
            return Err(LoadError::MisalignedTensorOffset(relative_offset));
        }
        relative_offsets[index] = relative_offset;

        let byte_len_u64 = elements
            .checked_mul(core::mem::size_of::<f32>() as u64)
            .ok_or(LoadError::ArithmeticOverflow)?;
        let byte_len = usize::try_from(byte_len_u64).map_err(|_| LoadError::ArithmeticOverflow)?;

        descriptor.tensors[index] = TensorDescriptor {
            name_hash: fnv1a64(name),
            dimensions,
            dimension_count,
            ggml_type,
            data_offset: 0,
            byte_len,
        };
    }

    descriptor.data_start = align_up(cursor.offset, DEFAULT_ALIGNMENT)?;
    if descriptor.data_start > bytes.len() {
        return Err(LoadError::TrailingHeaderPastData);
    }

    for index in 0..tensor_count {
        let relative = usize::try_from(relative_offsets[index])
            .map_err(|_| LoadError::ArithmeticOverflow)?;
        let absolute = descriptor
            .data_start
            .checked_add(relative)
            .ok_or(LoadError::ArithmeticOverflow)?;
        let end = absolute
            .checked_add(descriptor.tensors[index].byte_len)
            .ok_or(LoadError::ArithmeticOverflow)?;
        if end > bytes.len() {
            return Err(LoadError::TensorOutOfBounds);
        }
        descriptor.tensors[index].data_offset = absolute;
    }

    for left in 0..tensor_count {
        for right in left + 1..tensor_count {
            let a = descriptor.tensors[left].range();
            let b = descriptor.tensors[right].range();
            if a.start < b.end && b.start < a.end {
                return Err(LoadError::OverlappingTensors);
            }
        }
    }

    Ok(descriptor)
}

pub fn tensor_bytes<'a>(
    model: &'a [u8],
    tensor: &TensorDescriptor,
) -> Result<&'a [u8], LoadError> {
    model
        .get(tensor.range())
        .ok_or(LoadError::TensorOutOfBounds)
}

pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut index = 0;
    while index < bytes.len() {
        hash ^= bytes[index] as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
        index += 1;
    }
    hash
}

pub const fn align_up(value: usize, alignment: usize) -> Result<usize, LoadError> {
    if alignment == 0 || !alignment.is_power_of_two() {
        return Err(LoadError::ArithmeticOverflow);
    }
    let mask = alignment - 1;
    match value.checked_add(mask) {
        Some(sum) => Ok(sum & !mask),
        None => Err(LoadError::ArithmeticOverflow),
    }
}

verus! {

pub open spec fn range_fits(offset: usize, length: usize, total: usize) -> bool {
    offset <= total && length <= total - offset
}

pub fn checked_range_end(offset: usize, length: usize, total: usize) -> (result: Option<usize>)
    ensures
        result.is_some() ==> range_fits(offset, length, total),
        result.is_some() ==> result.unwrap() == offset + length,
        result.is_none() ==> !range_fits(offset, length, total),
{
    if offset <= total && length <= total - offset {
        Some(offset + length)
    } else {
        None
    }
}

} // verus!

#[cfg(test)]
mod tests {
    extern crate std;

    use std::vec;
    use std::vec::Vec;

    use super::*;

    fn push_u32(output: &mut Vec<u8>, value: u32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u64(output: &mut Vec<u8>, value: u64) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn fixture(two_tensors: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&GGUF_MAGIC);
        push_u32(&mut bytes, GGUF_VERSION);
        push_u64(&mut bytes, if two_tensors { 2 } else { 1 });
        push_u64(&mut bytes, 0);

        let names: &[&[u8]] = if two_tensors {
            &[b"transition", b"bias"]
        } else {
            &[b"transition"]
        };
        for (index, name) in names.iter().enumerate() {
            push_u64(&mut bytes, name.len() as u64);
            bytes.extend_from_slice(name);
            push_u32(&mut bytes, 2);
            push_u64(&mut bytes, 2);
            push_u64(&mut bytes, 2);
            push_u32(&mut bytes, GGML_TYPE_F32);
            push_u64(&mut bytes, (index * DEFAULT_ALIGNMENT) as u64);
        }
        while bytes.len() % DEFAULT_ALIGNMENT != 0 {
            bytes.push(0);
        }
        let data_len = if two_tensors { DEFAULT_ALIGNMENT + 16 } else { 16 };
        bytes.extend_from_slice(&vec![0; data_len]);
        bytes
    }

    #[test]
    fn parses_bounded_f32_tensor() {
        let bytes = fixture(false);
        let model = parse(&bytes).unwrap();
        let tensor = model.find(b"transition").unwrap();
        assert_eq!(tensor.dimensions[..2], [2, 2]);
        assert_eq!(tensor.byte_len, 16);
        assert_eq!(tensor_bytes(&bytes, tensor).unwrap().len(), 16);
    }

    #[test]
    fn truncated_inputs_fail_cleanly() {
        let bytes = fixture(false);
        for length in 0..bytes.len() {
            let result = parse(&bytes[..length]);
            assert!(result.is_err(), "prefix {length} unexpectedly parsed");
        }
    }

    #[test]
    fn oversized_name_is_rejected() {
        let mut bytes = fixture(false);
        bytes[24..32].copy_from_slice(&(MAX_NAME_LEN as u64 + 1).to_le_bytes());
        assert_eq!(parse(&bytes), Err(LoadError::InvalidNameLength(65)));
    }

    #[test]
    fn overlapping_tensors_are_rejected() {
        let mut bytes = fixture(true);
        let second_name = 24 + 8 + b"transition".len() + 4 + 16 + 4 + 8;
        let second_offset = second_name + 8 + b"bias".len() + 4 + 16 + 4;
        bytes[second_offset..second_offset + 8].copy_from_slice(&0u64.to_le_bytes());
        assert_eq!(parse(&bytes), Err(LoadError::OverlappingTensors));
    }

    #[test]
    fn tensor_outside_buffer_is_rejected() {
        let mut bytes = fixture(false);
        let offset_position = 24 + 8 + b"transition".len() + 4 + 16 + 4;
        bytes[offset_position..offset_position + 8]
            .copy_from_slice(&(DEFAULT_ALIGNMENT as u64 * 100).to_le_bytes());
        assert_eq!(parse(&bytes), Err(LoadError::TensorOutOfBounds));
    }
}
