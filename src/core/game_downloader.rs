use std::{path::PathBuf, sync::OnceLock};

use crate::core::fs::{BaseDirectory, exists, full_path};
use anyhow::{Ok, Result};
use irmin::{DownloadHandle, Sophon};

static DOWNLOAD_HANDLE: OnceLock<DownloadHandle> = OnceLock::new();

#[derive(PartialEq, Eq)]
pub enum UpdateAvailability {
    Updated,
    Preinstall,
    Outdated,
    NotInstalled,
}

fn download_handle() -> DownloadHandle {
    DOWNLOAD_HANDLE.get_or_init(DownloadHandle::new).clone()
}

/// Downloads a fresh copy of a game code and a copy of the voice-over files for
/// the game in a valid requested language
pub async fn download_game(game: String, lang: &str) -> Result<()> {
    let inst_path = get_install_path(&game)?;
    let s = Sophon::builder(game, inst_path)
        .vo_lang(lang)
        .verify_mode(irmin::VerifyMode::None)
        .build();

    s.download(&download_handle(), |p| {
        // Todo
    })
    .await?;

    s.verify_integrity(|p| {}).await?;

    Ok(())
}

/// Downloads updates/preinstalls and applies preinstalls during an update if
/// one is available one is available
pub async fn update_downloader(game: String, lang: &str) -> Result<()> {
    let update = update_status(game.clone()).await?;
    let handle = download_handle();
    let inst_path = get_install_path(&game)?;
    let s = Sophon::builder(game, inst_path)
        .vo_lang(lang)
        .verify_mode(irmin::VerifyMode::None)
        .build();

    match update {
        UpdateAvailability::Preinstall => {
            s.preinstall(&handle, |p| {
                //todo
            })
            .await?;
        }
        UpdateAvailability::Outdated => {
            // TODO: Preinstall check
            s.update(&handle, |p| {
                // todo
            })
            .await?;
        }
        UpdateAvailability::NotInstalled | UpdateAvailability::Updated => {}
    }

    Ok(())
}

/// Gets the status of the game supplied as a parameter
async fn update_status(game: String) -> Result<UpdateAvailability> {
    if !game_installed(&game)? {
        return Ok(UpdateAvailability::NotInstalled);
    }

    let inst_path = get_install_path(&game)?;
    let s = Sophon::builder(game, inst_path).build();
    let res = s.check_update().await?;

    Ok(if res.preinstall_available && !res.preinstall_downloaded {
        UpdateAvailability::Preinstall
    } else if res.update_available {
        UpdateAvailability::Outdated
    } else {
        UpdateAvailability::Updated
    })
}

// Pauses/Resumes an acive sophon download
pub fn toggle_downloading() {
    let handle = download_handle();
    
    if handle.is_paused() {
        handle.resume();
    } else {
        handle.pause();
    }
}

/// Cancels a sophon operation
pub fn cancel_download() {
    download_handle().cancel();
}

/// Path relative to a hypothetical game installed, named after its game code
fn game_rel_path(game: &str) -> PathBuf {
    PathBuf::from(format!("games/{game}"))
}

/// Gets the full path to a game install, provided by game_rel_path
fn get_install_path(game: &str) -> Result<PathBuf> {
    Ok(full_path(
        Some(game_rel_path(game)),
        Some(BaseDirectory::AppData),
    )?)
}

/// Checks if a game is installed
pub fn game_installed(game: &str) -> Result<bool> {
    Ok(exists(game_rel_path(game), Some(BaseDirectory::AppData))?)
}
