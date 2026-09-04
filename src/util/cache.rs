use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    core::fs::{BaseDirectory, exists, read_dir_as_paths, remove},
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
    let games = vec!["bh3", "hk4e", "hkrpg", "nap"];
    let locale = "en-us"; // TODO: Use settings-based locale later

    for game in games {
        let mut downloaded_files: Vec<PathBuf> = vec![];

        let files_present: Vec<PathBuf> = read_dir_as_paths(
            PathBuf::from(format!("cache/{game}/{locale}")),
            Some(BaseDirectory::AppData),
        )?;

        let url = format!(
            "https://aedes.elysiae.app/v3/getAssets?lang={}&game={}",
            locale, game
        );
        let response = fetch_data::<AedesResponse>(&url).await?;
        let value = serde_json::to_value(&response)?;

        if let Value::Object(map) = value {
            for (_key, value) in map.iter() {
                let v_str = value.as_str().unwrap_or_default();
                let path = PathBuf::from(format!("cache{v_str}")); // Should return cache/game code/locale/filename.ext

                // Should not continue if the value is not defined or is empty, and there is no need to continue if the file already exists
                if value.is_null()
                    || v_str == ""
                    || exists(path.clone(), Some(BaseDirectory::AppData))?
                {
                    continue;
                }
                downloaded_files.push(path.clone());
                let url = format!("https://aedes.elysiae.app{v_str}"); // v_str contains the forwards slash omitted in the url here

                // The following is done to get the file name without the file extension from the endpoint for sha256 hash verification later down the line. The file is named after its sha256sum
                let mut split: Vec<&str> = v_str.split(&['/', '.']).collect();
                let _ = split.pop().unwrap(); // file extension - useless data
                let hash = split.pop().unwrap(); // sha256sum only

                download_file(url, path, Some(BaseDirectory::AppData), None).await?;

                // TODO: File hash verification

                // Assemble a list of files that are no longer on the Aedes endpoint and delete them
                let to_delete: Vec<PathBuf> = files_present
                    .iter()
                    .filter(|x| downloaded_files.contains(x))
                    .cloned()
                    .collect();

                for file in to_delete {
                    remove(file, Some(BaseDirectory::AppData), Some(true))?;
                }
            }
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
