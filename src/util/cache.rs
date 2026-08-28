use std::path::PathBuf;

use anyhow::Result;

struct AssetData {
    backgrounds: Vec<BackgroundAsset>,
    icon: String,
    overlay: String,
}

struct BackgroundAsset {
    image: String,
    video: String,
}

pub enum AssetType {
    BackgroundVideo,
    BackgroundImage,
    Icon,
    Overlay,
}

pub async fn update_cache() -> Result<()> {
    Ok(())
}

pub fn get_cached_asset_path(game_code: &str, asset_type: AssetType) -> Result<PathBuf> {
    todo!()
}
