use std::fs::File;
use std::io::{BufWriter, Read, Seek, Write};
use std::path::PathBuf;

use super::header::{DiffChunkInfo, DiffSingleChunkInfo, HeaderInfo};
use super::parser::BinaryExtensions;

pub(crate) struct HDiff {
    pub(crate) source_path: String,
    pub(crate) diff_path: String,
    pub(crate) dest_path: String,
}

impl HDiff {
    pub fn new(source_path: String, diff_path: String, dest_path: String) -> Self {
        HDiff {
            source_path,
            diff_path,
            dest_path,
        }
    }

    pub fn apply(&mut self, on_progress: Option<Box<dyn Fn(u64)>>) -> bool {
        match self.apply_inner(on_progress.as_ref().map(|cb| cb.as_ref())) {
            Ok(()) => true,
            Err(err) => {
                tauri_plugin_log::log::error!("[HDiff::apply] Error: {err}");
                false
            }
        }
    }

    fn apply_inner(
        &self,
        on_progress: Option<&dyn Fn(u64)>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Reject in-place patching: canonicalize detects same file on disk
        // when both exist; string comparison catches same-path-string when
        // the destination does not yet exist.
        let source_canonical: Option<PathBuf> = std::fs::canonicalize(&self.source_path).ok();
        let dest_canonical: Option<PathBuf> = std::fs::canonicalize(&self.dest_path).ok();
        if source_canonical.is_some()
            && dest_canonical.is_some()
            && source_canonical == dest_canonical
        {
            return Err(format!(
                "source and destination paths resolve to the same file: {path}",
                path = self.source_path
            )
            .into());
        }

        let mut diff_file = File::open(&self.diff_path)?;
        let mut header_info = HeaderInfo::default();
        let header_info_line = diff_file.read_string_to_null(512)?;

        if header_info_line.len() > 64 || !header_info_line.starts_with("HDIFF") {
            return Err("not a HDiff file format".into());
        }
        let h_info_arr: Vec<&str> = header_info_line.split('&').collect();
        if h_info_arr.len() < 2 || h_info_arr.len() > 3 {
            return Err(format!(
                "unsupported HDiff header format: expected 2 or 3 parts, got {parts} (raw: {header_info_line})",
                parts = h_info_arr.len()
            )
            .into());
        }

        let p_file_ver = Self::try_get_version(h_info_arr[0])?;
        if p_file_ver == 19 {
            return Err(
                "directory patches (HDIFF19) are not supported by the single-file patcher".into(),
            );
        }
        if p_file_ver != 13 && p_file_ver != 20 {
            return Err(format!(
                "unsupported HDiff version {p_file_ver} (only 13 and 20 supported)"
            )
            .into());
        }

        // 3-part header: "HDIFF13&zstd&fadler64" or "HDIFF20&zstd&fadler64"
        // The third field is a checksum mode string; validate it if present.
        if h_info_arr.len() == 3 {
            let checksum_name = h_info_arr[2];
            match checksum_name {
                "crc32" | "fadler64" | "nochecksum" | "Crc32" | "Fadler64" | "Nochecksum" => {}
                _ => {
                    return Err(format!("unsupported HDiff checksum mode: {checksum_name}").into());
                }
            }
        }

        header_info.comp_mode = h_info_arr[1].parse()?;
        header_info.is_single_compressed_diff = p_file_ver == 20;

        if header_info.is_single_compressed_diff {
            Self::read_single_file_header(&mut diff_file, &mut header_info)?;
        } else {
            Self::read_non_single_file_header(&mut diff_file, &mut header_info)?;
        }

        // Newfile patch: old_data_size == 0 means source is optional.
        let mut old_file: File;
        if header_info.old_data_size == 0 && !std::path::Path::new(&self.source_path).exists() {
            // Use /dev/null as empty source for newfile patches.
            old_file = File::open("/dev/null")?;
        } else {
            old_file = File::open(&self.source_path)?;
            let old_len = old_file.metadata()?.len() as i64;
            if old_len != header_info.old_data_size {
                return Err(format!(
                    "input file size mismatch: expected {expected} bytes, got {old_len} bytes",
                    expected = header_info.old_data_size
                )
                .into());
            }
        }

        let expected_size = header_info.new_data_size;
        if expected_size < 0 {
            return Err(std::io::Error::other("new_data_size is negative").into());
        }

        let out_file = File::create(&self.dest_path)?;
        let mut out_writer =
            BufWriter::with_capacity(super::super::FILE_WRITE_BUFFER_SIZE, out_file);

        if header_info.is_single_compressed_diff {
            super::patch_sf::PatchSF::new(header_info).patch(
                &mut old_file,
                &mut out_writer,
                &self.diff_path,
                on_progress,
            )?;
        } else {
            super::patch_single::PatchSingle::new(header_info).patch(
                &mut old_file,
                &mut out_writer,
                &self.diff_path,
                on_progress,
            )?;
        }
        out_writer.flush()?;

        let actual_size = std::fs::metadata(&self.dest_path)?.len() as i64;
        if actual_size != expected_size {
            return Err(format!(
                "Patch output size mismatch: expected {expected_size} bytes, got {actual_size} bytes",
            )
            .into());
        }

        Ok(())
    }

    pub(crate) fn try_get_version(str_val: &str) -> Result<i64, Box<dyn std::error::Error>> {
        let idx = str_val
            .find("HDIFF")
            .ok_or_else(|| format!("cannot find 'HDIFF' in: {str_val}"))?;
        let rest = &str_val[idx + "HDIFF".len()..];
        let num_str = rest.trim_start_matches(|c: char| !c.is_ascii_digit());
        num_str
            .parse::<i64>()
            .map_err(|_| format!("invalid version string: {num_str} (raw: {str_val})").into())
    }

    pub(crate) fn read_single_file_header(
        sr: &mut (impl Read + Seek),
        header_info: &mut HeaderInfo,
    ) -> Result<(), Box<dyn std::error::Error>> {
        header_info.single_chunk_info = DiffSingleChunkInfo::default();
        header_info.new_data_size = sr.read_long_7bit()?;
        header_info.old_data_size = sr.read_long_7bit()?;
        if header_info.new_data_size < 0 || header_info.old_data_size < 0 {
            return Err("new_data_size or old_data_size is negative".into());
        }

        header_info.chunk_info.cover_count = sr.read_long_7bit()?;
        header_info.step_mem_size = sr.read_long_7bit()?;
        if header_info.chunk_info.cover_count < 0 {
            return Err("cover_count is negative".into());
        }
        if header_info.step_mem_size < 0 {
            return Err("step_mem_size is negative".into());
        }
        header_info.single_chunk_info.uncompressed_size = sr.read_long_7bit()?;
        header_info.single_chunk_info.compressed_size = sr.read_long_7bit()?;
        if header_info.single_chunk_info.uncompressed_size < 0 {
            return Err("uncompressed_size is negative".into());
        }
        if header_info.single_chunk_info.compressed_size < 0 {
            return Err("compressed_size is negative".into());
        }

        let pos = sr.stream_position()? as i64;
        header_info.single_chunk_info.diff_data_pos = pos;
        header_info.compressed_count = if header_info.single_chunk_info.compressed_size > 0 {
            1
        } else {
            0
        };
        Ok(())
    }

    pub(crate) fn read_non_single_file_header(
        sr: &mut (impl Read + Seek),
        header_info: &mut HeaderInfo,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let type_end_pos = sr.stream_position()? as i64;
        header_info.new_data_size = sr.read_long_7bit()?;
        header_info.old_data_size = sr.read_long_7bit()?;
        if header_info.new_data_size < 0 || header_info.old_data_size < 0 {
            return Err("new_data_size or old_data_size is negative".into());
        }

        Self::get_diff_chunk_info(sr, &mut header_info.chunk_info, type_end_pos)?;
        header_info.compressed_count = ((header_info.chunk_info.compress_cover_buf_size > 0)
            as i64)
            + ((header_info.chunk_info.compress_rle_ctrl_buf_size > 0) as i64)
            + ((header_info.chunk_info.compress_rle_code_buf_size > 0) as i64)
            + ((header_info.chunk_info.compress_new_data_diff_size > 0) as i64);
        Ok(())
    }

    pub(crate) fn get_diff_chunk_info(
        sr: &mut (impl Read + Seek),
        chunk_info: &mut DiffChunkInfo,
        type_end_pos: i64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        *chunk_info = DiffChunkInfo::default();
        chunk_info.types_end_pos = type_end_pos;
        chunk_info.cover_count = sr.read_long_7bit()?;
        chunk_info.cover_buf_size = sr.read_long_7bit()?;
        chunk_info.compress_cover_buf_size = sr.read_long_7bit()?;
        chunk_info.rle_ctrl_buf_size = sr.read_long_7bit()?;
        chunk_info.compress_rle_ctrl_buf_size = sr.read_long_7bit()?;
        chunk_info.rle_code_buf_size = sr.read_long_7bit()?;
        chunk_info.compress_rle_code_buf_size = sr.read_long_7bit()?;
        chunk_info.new_data_diff_size = sr.read_long_7bit()?;
        chunk_info.compress_new_data_diff_size = sr.read_long_7bit()?;

        let fields: &[(&str, i64)] = &[
            ("cover_buf_size", chunk_info.cover_buf_size),
            (
                "compress_cover_buf_size",
                chunk_info.compress_cover_buf_size,
            ),
            ("rle_ctrl_buf_size", chunk_info.rle_ctrl_buf_size),
            (
                "compress_rle_ctrl_buf_size",
                chunk_info.compress_rle_ctrl_buf_size,
            ),
            ("rle_code_buf_size", chunk_info.rle_code_buf_size),
            (
                "compress_rle_code_buf_size",
                chunk_info.compress_rle_code_buf_size,
            ),
            ("new_data_diff_size", chunk_info.new_data_diff_size),
            (
                "compress_new_data_diff_size",
                chunk_info.compress_new_data_diff_size,
            ),
        ];
        for (name, val) in fields {
            if *val < 0 {
                return Err(format!("{name} is negative in diff chunk info").into());
            }
        }

        chunk_info.head_end_pos = sr.stream_position()? as i64;
        chunk_info.cover_end_pos = chunk_info
            .head_end_pos
            .checked_add(if chunk_info.compress_cover_buf_size > 0 {
                chunk_info.compress_cover_buf_size
            } else {
                chunk_info.cover_buf_size
            })
            .ok_or_else(|| -> Box<dyn std::error::Error> {
                "cover_end_pos overflow in diff chunk info".into()
            })?;
        Ok(())
    }
}

pub(crate) trait SeekableRead: Read + Seek {}
impl<T: Read + Seek> SeekableRead for T {}
