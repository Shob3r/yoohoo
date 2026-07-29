use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use directories::BaseDirs;

pub enum BaseDirectory {
    AppData,
    Desktop,
    Home,
    Compat,
}

/// Wrapper function for std::fs::exists(), but relative to a specific user
/// directory.
///
/// If no user directory is supplied, the function defaults to the app data
/// directory (~/.local/share/elysiae)
pub fn exists(p: PathBuf, base_dir: Option<BaseDirectory>) -> Result<bool> {
    let fp = full_path(Some(p), base_dir).context("Full path could not be resolved")?;
    Ok(fs::exists(fp).context("exists() could not complete successfully")?)
}

pub fn write(p: PathBuf, contents: &[u8], base_dir: Option<BaseDirectory>) -> Result<()> {
    let fp = full_path(Some(p), base_dir)?;
    let _ = fs::write(fp, contents).context("Failed to write to the path")?;

    Ok(())
}

pub fn full_path(p: Option<PathBuf>, base_dir: Option<BaseDirectory>) -> Result<PathBuf> {
    let d = BaseDirs::new().context("Could not find base directories!")?;

    let dir_path = match base_dir {
        Some(x) => match x {
            BaseDirectory::AppData => d.data_local_dir().to_path_buf().join("elysiae"),
            BaseDirectory::Desktop => d.home_dir().join("Desktop"),
            BaseDirectory::Home => d.home_dir().to_path_buf(),
            BaseDirectory::Compat => d
                .data_local_dir()
                .to_path_buf()
                .join("elysiae")
                .join("proton-data"),
        },
        None => d.home_dir().to_path_buf(),
    };

    let final_path = match p {
        Some(x) => dir_path.join(x),
        None => dir_path,
    };

    Ok(final_path)
}
