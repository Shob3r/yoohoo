use std::{path::PathBuf, sync::OnceLock};

use crate::{
    core::{
        fs::{BaseDirectory, exists, full_path, write_file},
        game::Game,
        proton_manager::exec_proton,
    },
    util::{normalize_game_name, notifications::broadcast_notification, settings::get_option},
};
use anyhow::{Error, Ok, Result};
use irmin::{ControlState, DownloadHandle, Sophon};

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
pub async fn download_game(game: Game, lang: &str) -> Result<()> {
    if !download_active()? {
        let inst_path = game.install_path();
        let s = Sophon::builder(game.code(), inst_path)
            .vo_lang(lang)
            .verify_mode(irmin::VerifyMode::None)
            .build();

        s.download(&download_handle(), |p| {
            // Todo
        })
        .await?;

        s.verify_integrity(|p| {}).await?;
    }
    if get_option("generate-desktop-shortcut").try_into().unwrap() {
        generate_desktop_file(game)?;
    }

    broadcast_notification("Download Complete");
    Ok(())
}

/// Downloads updates/preinstalls and applies preinstalls during an update if
/// one is available one is available
pub async fn download_update(game: Game, lang: &str) -> Result<()> {
    if !download_active()? {
        let update = update_status(game).await?;
        let handle = download_handle();
        let inst_path = game.install_path();
        let s = Sophon::builder(game.code(), inst_path)
            .vo_lang(lang)
            .verify_mode(irmin::VerifyMode::None)
            .build();

        match update {
            UpdateAvailability::Preinstall => {
                s.preinstall(&handle, |p| {
                    //todo
                })
                .await?;
                broadcast_notification("Preinstall Download Complete");
            }
            UpdateAvailability::Outdated => {
                // TODO: Check if a preinstall is download before downloading an update
                s.update(&handle, |p| {
                    // todo
                })
                .await?;
                broadcast_notification("Update Complete");
            }
            UpdateAvailability::NotInstalled | UpdateAvailability::Updated => {}
        }
    }

    Ok(())
}

/// Gets the status of the game supplied as a parameter
async fn update_status(game: Game) -> Result<UpdateAvailability> {
    if !exists(game.install_path(), None)? {
        return Ok(UpdateAvailability::NotInstalled);
    }

    let inst_path = game.install_path();
    let s = Sophon::builder(game.code(), inst_path).build();
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
pub fn toggle_downloading() -> Result<()> {
    if download_active()? {
        let handle = download_handle();
        if handle.is_paused() {
            handle.resume();
        } else {
            handle.pause();
        }
    }

    Ok(())
}

/// Cancels a sophon operation
pub fn cancel_download() {
    download_handle().cancel();
}

/// Path relative to a hypothetical game installed, named after its game code
fn game_rel_path(game: Game) -> PathBuf {
    PathBuf::from("games").join(game.code())
}

/// Checks if a game is installed
pub fn game_installed(game: Game) -> Result<bool> {
    Ok(exists(game_rel_path(game), None)?)
}

// Checks if the download handle is active. In the case of Elysiae, "Active"
// means that the handle is reporting that the download is in progress or has
// been paused
fn download_active() -> Result<bool> {
    let handle = download_handle();
    let state: irmin::ControlState = handle.get_state();

    Ok(state == ControlState::Running || state == ControlState::Paused)
}

/// Writes a desktop entry for a specified game to the desktop folder
fn generate_desktop_file(game: Game) -> Result<()> {
    let game_name = game.display_name();
    let deep_link_uri = format!("elysiae://open-game/{}", game.code());
    let icon_path = ""; // TODO: Implement icon path fetching function

    let contents = format!(
        "Name={game_name}\nComment=Play {game_name} with Elysiae\nExec=xdg-open {deep_link_uri}\nType=Application\nCategories=Game\nIcon={icon_path}"
    );

    let path = PathBuf::from(format!("{}.desktop", game_name));
    let _ = write_file(path, contents.as_bytes(), Some(BaseDirectory::Desktop))?;

    Ok(())
}

pub fn launch_game(game: Game) -> Result<()> {
    exec_proton(game.install_path().join(game.executable()))
}
