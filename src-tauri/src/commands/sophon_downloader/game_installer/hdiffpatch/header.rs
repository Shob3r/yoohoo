use super::compression_mode::CompressionMode;

#[derive(Debug, Clone, Default)]
pub(crate) struct HeaderInfo {
    pub(crate) comp_mode: CompressionMode,
    pub(crate) is_single_compressed_diff: bool,
    pub(crate) step_mem_size: i64,
    pub(crate) old_data_size: i64,
    pub(crate) new_data_size: i64,
    pub(crate) compressed_count: i64,
    pub(crate) single_chunk_info: DiffSingleChunkInfo,
    pub(crate) chunk_info: DiffChunkInfo,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DiffSingleChunkInfo {
    pub(crate) uncompressed_size: i64,
    pub(crate) compressed_size: i64,
    pub(crate) diff_data_pos: i64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DiffChunkInfo {
    pub(crate) types_end_pos: i64,
    pub(crate) cover_count: i64,
    pub(crate) cover_buf_size: i64,
    pub(crate) compress_cover_buf_size: i64,
    pub(crate) rle_ctrl_buf_size: i64,
    pub(crate) compress_rle_ctrl_buf_size: i64,
    pub(crate) rle_code_buf_size: i64,
    pub(crate) compress_rle_code_buf_size: i64,
    pub(crate) new_data_diff_size: i64,
    pub(crate) compress_new_data_diff_size: i64,
    pub(crate) head_end_pos: i64,
    pub(crate) cover_end_pos: i64,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RleRefClip {
    pub(crate) mem_copy_length: i64,
    pub(crate) mem_set_length: i64,
    pub(crate) mem_set_value: u8,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CoverHeader {
    pub(crate) old_pos: i64,
    pub(crate) new_pos: i64,
    pub(crate) cover_length: i64,
    pub(crate) next_cover_index: i64,
}

impl CoverHeader {
    pub(crate) fn new(
        old_pos: i64,
        new_pos: i64,
        cover_length: i64,
        next_cover_index: i64,
    ) -> Self {
        Self {
            old_pos,
            new_pos,
            cover_length,
            next_cover_index,
        }
    }
}

pub(crate) const K_SIGN_TAG_BIT: u8 = 1;
pub(crate) const K_BYTE_RLE_TYPE: u8 = 2;
pub(crate) const MAX_MEM_BUFFER_LEN: i64 = 7 << 20;
pub(crate) const MAX_MEM_BUFFER_LIMIT: usize = 10 << 20;
pub(crate) const MAX_ARRAY_POOL_LEN: usize = 4 << 20;
pub(crate) const MAX_ARRAY_POOL_SECOND_OFFSET: usize = MAX_ARRAY_POOL_LEN / 2;
