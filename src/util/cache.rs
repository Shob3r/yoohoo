use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    core::{
        fs::{BaseDirectory, exists, full_path, read_dir, remove},
        game::Game,
    },
    util::{
        settings::{SettingValue, get_option},
        web::{download_file, fetch_data},
    },
};

pub enum AssetType {
    Image,
    Video,
    Icon,
    Shortcut,
    Overlay,
}

#[derive(Debug, Serialize, Deserialize)]
struct AedesResponse {
    backgrounds: Vec<AedesBackgroundAssets>,
    icon: String,
    icon_cn: String,
    shortcut: String,
    shortcut_cn: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct AedesBackgroundAssets {
    image: String,
    video: Option<String>,
    overlay: Option<String>,
}

pub async fn update_cache() -> Result<()> {
    let games = vec![
        Game::try_from("bh3")?,
        Game::try_from("hk4e")?,
        Game::try_from("hkrpg")?,
        Game::try_from("nap")?,
    ];
    let locale = "en-us"; // TODO: Use settings-based locale later

    for game in games {
        let mut downloaded: Vec<PathBuf> = vec![];

        let files_present: Vec<PathBuf> = read_dir(
            PathBuf::from(format!("cache/{}/{}", game.code(), locale)),
            None,
        )?;

        let url = format!(
            "https://aedes.elysiae.app/v3/getAssets?game={}&locale={}",
            game.code(),
            locale
        );
        let response = fetch_data::<AedesResponse>(&url).await?;
        let value = serde_json::to_value(&response)?;

        if let Value::Object(map) = value {
            for (_key, v) in map.iter() {
                let vs = v.as_str().unwrap_or_default();
                let p = PathBuf::from(format!("cache{vs}")); // Should return cache/game code/locale/filename.ext

                // Should not continue if the value is not defined or is empty, and there is no need to continue if the file already exists
                if v.is_null() || vs == "" || files_present.contains(&p) {
                    continue;
                }
                downloaded.push(full_path(Some(p.clone()), None)?);
                let url = format!("https://aedes.elysiae.app{vs}"); // v_str contains the forwards slash omitted in the url here

                // The following is done to get the file name without the file extension from the endpoint for sha256 hash verification later down the line. The file is named after its sha256sum
                let mut split: Vec<&str> = vs.split(&['/', '.']).collect();
                let _ = split.pop(); // file extension - useless data; no need to unwrap either
                let hash = split.pop().unwrap(); // sha256sum only

                download_file(url, p, None, None).await?;

                // TODO: File hash verification
            }
        }

        // Assemble a list of files that are no longer on the Aedes endpoint and delete them
        let to_delete: Vec<PathBuf> = files_present
            .iter()
            .filter(|x| !downloaded.contains(x))
            .cloned()
            .collect();

        for file in to_delete {
            remove(file, None, Some(true))?;
        }
    }

    Ok(())
}

pub fn get_cached_asset_paths(
    game_code: &str,
    asset_type: Option<AssetType>,
) -> Result<Vec<PathBuf>> {
    todo!()
}
