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

use std::sync::Mutex;

pub(crate) struct BufferPool {
    buffers: Mutex<Vec<Vec<u8>>>,
    max_idle: usize,
}

impl BufferPool {
    pub const fn new(max_idle: usize) -> Self {
        Self {
            buffers: Mutex::new(Vec::new()),
            max_idle,
        }
    }

    pub fn take(&self, min_capacity: usize) -> Vec<u8> {
        let mut pool = self.buffers.lock().unwrap();
        let mut best_idx = None;
        let mut best_cap = usize::MAX;
        for i in 0..pool.len() {
            let cap = pool[i].capacity();
            if cap >= min_capacity && cap < best_cap {
                best_idx = Some(i);
                best_cap = cap;
            }
        }
        if let Some(i) = best_idx {
            return pool.swap_remove(i);
        }
        drop(pool);
        Vec::with_capacity(min_capacity)
    }

    pub fn return_buf(&self, buf: Vec<u8>) {
        let mut pool = self.buffers.lock().unwrap();
        if pool.len() < self.max_idle {
            pool.push(buf);
        }
    }
}

#[cfg(test)]
mod tests;
