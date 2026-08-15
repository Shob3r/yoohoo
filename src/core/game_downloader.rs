use std::sync::{Arc, OnceLock};

use anyhow::{Ok, Result};
use irmin::{DownloadClient, DownloadHandle, SophonProgress, VerifyMode, client::HttpClient, sophon_download};

use crate::core::fs::{BaseDirectory, full_path};

pub enum GAMES {
    BH3,
    HK4E,
    HKRPG,
    NAP,
}

pub async fn download_game(game: GAMES, lang: &str) -> Result<()> {
    let str_game = game_to_str(game)?;
    let game_path = full_path(None, Some(BaseDirectory::AppData))?.join(str_game.clone());
    let on_progress: irmin::ProgressUpdater = Arc::new(|p: SophonProgress| {
        println!("{p:?}");
    });

    Ok(())
}

pub fn update_game(game: GAMES) -> Result<()> {
    todo!()
}

pub fn pause_download() -> Result<()> {
    Ok(())
}

pub fn resume_download() -> Result<()> {
    todo!()
}

pub fn cancel_download() -> Result<()> {
    todo!()
}

fn has_preinstall(game: GAMES) -> Result<()> {
    todo!()
}

fn game_to_str(game: GAMES) -> Result<String> {
    Ok(match game {
        GAMES::BH3 => "bh3",
        GAMES::HK4E => "hk4e",
        GAMES::HKRPG => "hkrpg",
        GAMES::NAP => "nap",
    }
    .to_string())
}
