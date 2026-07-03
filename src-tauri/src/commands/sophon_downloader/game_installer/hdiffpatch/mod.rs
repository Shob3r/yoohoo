#[allow(dead_code)]
mod compression;
mod compression_mode;
mod diff;
mod header;
mod parser;
mod patch_core;
mod patch_sf;
mod patch_single;

pub(crate) use compression_mode::CompressionMode;
pub(crate) use diff::HDiff;
pub(crate) use diff::SeekableRead;
#[allow(unused_imports)]
pub(crate) use header::{
    CoverHeader, DiffChunkInfo, DiffSingleChunkInfo, HeaderInfo, K_BYTE_RLE_TYPE, K_SIGN_TAG_BIT,
    MAX_ARRAY_POOL_LEN, MAX_ARRAY_POOL_SECOND_OFFSET, MAX_MEM_BUFFER_LEN, MAX_MEM_BUFFER_LIMIT,
    RleRefClip,
};

#[cfg(test)]
mod tests;
