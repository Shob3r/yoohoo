use std::{fs, path::PathBuf};

use anyhow::{Context, Error, Ok, Result};
use directories::BaseDirs;

pub enum BaseDirectory {
    AppData,
    Desktop,
    Home,
    Compat,
}

/// Wrapper function for std::fs::exists(), but relative to a specific "Base
/// Directory"
///
/// If no "Base Directory" is supplied, the function defaults to the app data
/// directory (~/.local/share/elysiae)
pub fn exists(p: PathBuf, base_dir: Option<BaseDirectory>) -> Result<bool> {
    let fp = full_path(Some(p), base_dir).context("Full path could not be resolved")?;
    Ok(fs::exists(fp).context("exists() could not complete successfully")?)
}

/// Wrapper function for std::fs::read, relative to a specified "Base Directory"
///
/// If no "Base Directory" is supplied, the function defaults to the app data
/// directory (~/.local/share/elysiae)
pub fn read_file(p: PathBuf, base_dir: Option<BaseDirectory>) -> Result<Vec<u8>> {
    let fp = full_path(Some(p), base_dir)?;
    if fp.exists() {
        let data = fs::read(fp).context("Could not read this file")?;

        Ok(data)
    } else {
        Err(Error::msg(format!(
            "The Path \"{}\" could not be found on disk",
            fp.to_string_lossy()
        )))
    }
}

/// Wraper function for std::fs::write, but relative to a specified user
/// directory
///
/// If no user directory is supplied, the function defaults to the app data
/// directory (~/.local/share/elysiae)
pub fn write_file(p: PathBuf, contents: &[u8], base_dir: Option<BaseDirectory>) -> Result<()> {
    let fp = full_path(Some(p), base_dir)?;

    if fp.exists() {
        let _ = fs::write(fp, contents).context("Failed to write to the path")?;

        Ok(())
    } else {
        Err(Error::msg(format!(
            "The Path \"{}\" could not be found on disk",
            fp.to_string_lossy()
        )))
    }
}

fn dirOf(p: PathBuf) -> Result<PathBuf> {
    unimplemented!()
}

/// Wraper function for std::fs::remove_file, std::fs::remove_dir and
/// std::fs::remove_dir_all, relative to a specified "Base Directory"
///
/// The function automatically determines weather the path you specified is a
/// directory or not and calls the appropriate std function to remove the item
/// from the filesystem
///
/// If a recursive parameter is not provided, it will automatically recursively
/// delete a directory
///
/// If a "Base Directory" is not provided, it will default to the app data
/// directory (~/.local/share/elysiae)
pub fn remove(p: PathBuf, base_dir: Option<BaseDirectory>, recursive: Option<bool>) -> Result<()> {
    let fp = full_path(Some(p), base_dir)?;
    if fp.exists() {
        if fp.is_file() {
            fs::remove_file(fp).context("Could not remove this file")?;
        } else if fp.is_dir() {
            match recursive {
                Some(x) => {
                    if x {
                        fs::remove_dir_all(fp)
                            .context("Could not recursively delete this directory")?;
                    } else {
                        fs::remove_dir(fp).context("Could not remove this directory")?;
                    }
                }
                None => {
                    fs::remove_dir_all(fp)
                        .context("Could not recursively delete this directory")?;
                }
            };
        }
        Ok(())
    } else {
        Err(Error::msg(format!(
            "The Path \"{}\" could not be found on disk",
            fp.to_string_lossy()
        )))
    }
}

/// Joins a specified "Base directory" with a relative path
///
/// If no "relative path" is specified, the "base directory" is returned by
/// itself If no "base directory is specified", the function will use the app
/// data directory by default (~/.local/share/elysiae)
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
