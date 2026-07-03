//! Tauri command handlers for the Sophon downloader.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Emitter, Manager, State, command};
use tauri_plugin_log::log;

use crate::commands::sophon_downloader::game_installer::{
    self, DownloadHandle, SophonError, UpdateInfo, read_installed_tag,
};
use crate::commands::sophon_downloader::{
    client::{ActiveDownload, DownloadClient, HttpClient},
    progress::{CommandError, SophonProgress},
    state::{
        clear_download_state, delete_chunks_dir, download_state_path, load_download_state,
        save_download_state,
    },
    types::{DownloadState, DownloadType, ResumeInfo},
};

struct StateMeta {
    game_id: String,
    vo_lang: String,
    output_path: String,
    download_type: DownloadType,
    current_tag: Option<String>,
    manifest_hash: String,
}

fn make_state_saver(
    app: &AppHandle,
    state: &DownloadState,
    completed_files: Arc<Mutex<HashSet<String>>>,
) -> game_installer::StateSaver {
    let app = app.clone();
    let meta = StateMeta {
        game_id: state.game_id.clone(),
        vo_lang: state.vo_lang.clone(),
        output_path: state.output_path.clone(),
        download_type: state.download_type.clone(),
        current_tag: state.current_tag.clone(),
        manifest_hash: state.manifest_hash.clone(),
    };
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct DownloadStateRef<'a> {
        game_id: &'a str,
        vo_lang: &'a str,
        output_path: &'a str,
        download_type: &'a DownloadType,
        current_tag: &'a Option<String>,
        manifest_hash: &'a str,
        downloaded_chunks: &'a HashMap<String, u64>,
        completed_files: &'a HashSet<String>,
    }
    Arc::new(move |chunks: &HashMap<String, u64>| {
        let completed = completed_files.lock().unwrap_or_else(|e| e.into_inner());
        let snapshot = DownloadStateRef {
            game_id: &meta.game_id,
            vo_lang: &meta.vo_lang,
            output_path: &meta.output_path,
            download_type: &meta.download_type,
            current_tag: &meta.current_tag,
            manifest_hash: &meta.manifest_hash,
            downloaded_chunks: chunks,
            completed_files: &completed,
        };
        let json = match serde_json::to_string(&snapshot) {
            Ok(j) => j,
            Err(_) => return,
        };
        drop(completed);
        let Some(path) = download_state_path(&app) else {
            return;
        };
        static SAVE_COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = SAVE_COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
        let tmp_path = path.with_extension(format!("save-{seq}.tmp"));
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let file = match std::fs::File::create(&tmp_path) {
            Ok(f) => f,
            Err(_) => return,
        };
        if std::io::Write::write_all(&mut &file, json.as_bytes()).is_ok()
            && file.sync_all().is_ok()
            && let Err(e) = fs::rename(&tmp_path, &path)
        {
            let _ = fs::remove_file(&tmp_path);
            log::error!("Failed to rename state file: {e}");
        }
    })
}

/// Downloads a specific game version by tag, replacing any existing
/// installation.
#[command]
pub async fn sophon_download_version(
    game_id: String,
    vo_lang: String,
    output_path: String,
    tag: String,
    app_handle: AppHandle,
    client: State<'_, HttpClient>,
    active: State<'_, ActiveDownload>,
) -> Result<(), String> {
    let game_dir = app_handle
        .path()
        .resolve(&output_path, BaseDirectory::AppData)
        .map_err(|err| err.to_string())?;

    if game_dir.exists() {
        tokio::task::spawn_blocking({
            let gd = game_dir.clone();
            move || {
                if let Err(err) = fs::remove_dir_all(&gd) {
                    log::warn!("Failed to remove existing game dir: {err}");
                }
            }
        })
        .await
        .map_err(|err| err.to_string())?;
    }

    log::warn!("Fetching manifest for game_id={game_id} tag={tag}");
    emit(&app_handle, SophonProgress::FetchingManifest);

    let (installers, resolved_tag, manifest_hash) =
        game_installer::build_installers_for_tag(&client.0, &game_id, &vo_lang, &tag)
            .await
            .map_err(|err| {
                log::warn!("build_installers_for_tag failed: {err}");
                emit_error(&app_handle, &err);
                err.to_string()
            })?;

    let state = DownloadState {
        game_id: game_id.clone(),
        vo_lang: vo_lang.clone(),
        output_path: output_path.clone(),
        download_type: DownloadType::Fresh,
        current_tag: None,
        manifest_hash,
        downloaded_chunks: HashMap::new(),
        completed_files: HashSet::new(),
    };
    save_download_state(&app_handle, &state)?;

    let handle = DownloadHandle::new();
    *active.0.lock().await = Some(handle.clone());

    let completed_files: Arc<Mutex<HashSet<String>>> =
        Arc::new(Mutex::new(state.completed_files.clone()));
    let saver = make_state_saver(&app_handle, &state, Arc::clone(&completed_files));
    let app_clone = app_handle.clone();
    let vo_langs: Vec<String> = vec![vo_lang.clone()];
    let result = game_installer::install(
        installers,
        &game_dir,
        vec![],
        &resolved_tag,
        game_installer::ResumeContext {
            prev_manifest_hash: String::new(),
            prev_downloaded_chunks: HashMap::new(),
        },
        game_installer::InstallOptions {
            is_preinstall: false,
            is_resume: false,
            handle,
        },
        game_installer::InstallCallbacks {
            updater: Arc::new(move |p| emit(&app_clone, p)),
            state_saver: saver,
            completed_files,
        },
        &game_id,
        &vo_langs,
    )
    .await;

    clear_download_state(&app_handle);
    *active.0.lock().await = None;

    match result {
        Ok(()) => {
            let plugin_emit = app_handle.clone();
            let plugin_updater: Arc<dyn Fn(SophonProgress) + Send + Sync> =
                Arc::new(move |p| emit(&plugin_emit, p));
            if let Err(err) = game_installer::install_plugins(&client.0, &game_dir, &game_id, {
                let u = plugin_updater.clone();
                move |p| u(p)
            })
            .await
            {
                log::warn!("Plugin installation failed: {err}");
                emit_error(&app_handle, &err);
            }
            if let Err(err) =
                game_installer::install_channel_sdks(&client.0, &game_dir, &game_id, {
                    let u = plugin_updater.clone();
                    move |p| u(p)
                })
                .await
            {
                log::warn!("Channel SDK installation failed: {err}");
                emit_error(&app_handle, &err);
            }
            emit(&app_handle, SophonProgress::Finished);
            Ok(())
        }
        Err(err) => install_result(Err(err), &app_handle),
    }
}

/// Downloads a fresh game installation.
#[command]
pub async fn sophon_download(
    game_id: String,
    vo_lang: String,
    output_path: String,
    app_handle: AppHandle,
    client: State<'_, HttpClient>,
    active: State<'_, ActiveDownload>,
) -> Result<(), String> {
    let game_dir = app_handle
        .path()
        .resolve(&output_path, BaseDirectory::AppData)
        .map_err(|err| err.to_string())?;

    log::warn!("Fetching manifest for game_id={game_id}");
    emit(&app_handle, SophonProgress::FetchingManifest);

    let (installers, tag, manifest_hash) =
        game_installer::build_installers(&client.0, &game_id, &vo_lang)
            .await
            .map_err(|err| {
                log::warn!("build_installers failed: {err}");
                emit_error(&app_handle, &err);
                err.to_string()
            })?;

    let state = DownloadState {
        game_id: game_id.clone(),
        vo_lang: vo_lang.clone(),
        output_path: output_path.clone(),
        download_type: DownloadType::Fresh,
        current_tag: None,
        manifest_hash,
        downloaded_chunks: HashMap::new(),
        completed_files: HashSet::new(),
    };
    save_download_state(&app_handle, &state)?;

    let handle = DownloadHandle::new();
    *active.0.lock().await = Some(handle.clone());

    let completed_files: Arc<Mutex<HashSet<String>>> =
        Arc::new(Mutex::new(state.completed_files.clone()));
    let saver = make_state_saver(&app_handle, &state, Arc::clone(&completed_files));
    let app_clone = app_handle.clone();
    let vo_langs: Vec<String> = vec![vo_lang.clone()];
    let result = game_installer::install(
        installers,
        &game_dir,
        vec![],
        &tag,
        game_installer::ResumeContext {
            prev_manifest_hash: String::new(),
            prev_downloaded_chunks: HashMap::new(),
        },
        game_installer::InstallOptions {
            is_preinstall: false,
            is_resume: false,
            handle,
        },
        game_installer::InstallCallbacks {
            updater: Arc::new(move |p| emit(&app_clone, p)),
            state_saver: saver,
            completed_files,
        },
        &game_id,
        &vo_langs,
    )
    .await;

    clear_download_state(&app_handle);
    *active.0.lock().await = None;

    match result {
        Ok(()) => {
            let plugin_emit = app_handle.clone();
            let plugin_updater: Arc<dyn Fn(SophonProgress) + Send + Sync> =
                Arc::new(move |p| emit(&plugin_emit, p));
            if let Err(err) = game_installer::install_plugins(&client.0, &game_dir, &game_id, {
                let u = plugin_updater.clone();
                move |p| u(p)
            })
            .await
            {
                log::warn!("Plugin installation failed: {err}");
                emit_error(&app_handle, &err);
            }
            if let Err(err) =
                game_installer::install_channel_sdks(&client.0, &game_dir, &game_id, {
                    let u = plugin_updater.clone();
                    move |p| u(p)
                })
                .await
            {
                log::warn!("Channel SDK installation failed: {err}");
                emit_error(&app_handle, &err);
            }
            emit(&app_handle, SophonProgress::Finished);
            Ok(())
        }
        Err(err) => install_result(Err(err), &app_handle),
    }
}

/// Updates an existing game installation.
#[command]
pub async fn sophon_update(
    game_id: String,
    vo_lang: String,
    output_path: String,
    app_handle: AppHandle,
    client: State<'_, HttpClient>,
    active: State<'_, ActiveDownload>,
) -> Result<(), String> {
    let game_dir = app_handle
        .path()
        .resolve(&output_path, BaseDirectory::AppData)
        .map_err(|err| err.to_string())?;

    let current_tag =
        read_installed_tag(&game_dir).ok_or("No installed version found, cannot update")?;

    log::warn!("Fetching manifest for game_id={game_id}");
    emit(&app_handle, SophonProgress::FetchingManifest);

    let (installers, deleted_files, new_tag, manifest_hash) =
        game_installer::build_update_installers(
            &client.0,
            &game_id,
            &vo_lang,
            &current_tag,
            &game_dir,
        )
        .await
        .map_err(|err| {
            log::warn!("build_update_installers failed: {err}");
            emit_error(&app_handle, &err);
            err.to_string()
        })?;

    let state = DownloadState {
        game_id: game_id.clone(),
        vo_lang: vo_lang.clone(),
        output_path: output_path.clone(),
        download_type: DownloadType::Update,
        current_tag: Some(current_tag.clone()),
        manifest_hash,
        downloaded_chunks: HashMap::new(),
        completed_files: HashSet::new(),
    };
    save_download_state(&app_handle, &state)?;

    let handle = DownloadHandle::new();
    *active.0.lock().await = Some(handle.clone());

    let completed_files: Arc<Mutex<HashSet<String>>> =
        Arc::new(Mutex::new(state.completed_files.clone()));
    let saver = make_state_saver(&app_handle, &state, Arc::clone(&completed_files));
    let app_clone = app_handle.clone();
    let vo_langs: Vec<String> = vec![vo_lang.clone()];
    let result = game_installer::install(
        installers,
        &game_dir,
        deleted_files,
        &new_tag,
        game_installer::ResumeContext {
            prev_manifest_hash: String::new(),
            prev_downloaded_chunks: HashMap::new(),
        },
        game_installer::InstallOptions {
            is_preinstall: false,
            is_resume: false,
            handle,
        },
        game_installer::InstallCallbacks {
            updater: Arc::new(move |p| emit(&app_clone, p)),
            state_saver: saver,
            completed_files,
        },
        &game_id,
        &vo_langs,
    )
    .await;

    clear_download_state(&app_handle);
    *active.0.lock().await = None;

    match result {
        Ok(()) => {
            let plugin_emit = app_handle.clone();
            let plugin_updater: Arc<dyn Fn(SophonProgress) + Send + Sync> =
                Arc::new(move |p| emit(&plugin_emit, p));
            if let Err(err) = game_installer::install_plugins(&client.0, &game_dir, &game_id, {
                let u = plugin_updater.clone();
                move |p| u(p)
            })
            .await
            {
                log::warn!("Plugin installation failed: {err}");
                emit_error(&app_handle, &err);
            }
            if let Err(err) =
                game_installer::install_channel_sdks(&client.0, &game_dir, &game_id, {
                    let u = plugin_updater.clone();
                    move |p| u(p)
                })
                .await
            {
                log::warn!("Channel SDK installation failed: {err}");
                emit_error(&app_handle, &err);
            }
            emit(&app_handle, SophonProgress::Finished);
            Ok(())
        }
        Err(err) => install_result(Err(err), &app_handle),
    }
}

/// Pre-downloads an upcoming game version using patch-based preinstall.
#[command]
pub async fn sophon_preinstall(
    game_id: String,
    vo_lang: String,
    output_path: String,
    app_handle: AppHandle,
    client: State<'_, HttpClient>,
    active: State<'_, ActiveDownload>,
) -> Result<(), String> {
    let game_dir = app_handle
        .path()
        .resolve(&output_path, BaseDirectory::AppData)
        .map_err(|err| err.to_string())?;

    log::warn!("Fetching manifest for game_id={game_id}");
    emit(&app_handle, SophonProgress::FetchingManifest);

    let plan = game_installer::build_preinstall_plan(&client.0, &game_id, &vo_lang, &game_dir)
        .await
        .map_err(|err| {
            log::warn!("build_preinstall_plan failed: {err}");
            err.to_string()
        })?;

    let tag = plan.tag.clone();

    let current_tag = game_installer::read_installed_tag(&game_dir);

    let state = DownloadState {
        game_id: game_id.clone(),
        vo_lang: vo_lang.clone(),
        output_path: output_path.clone(),
        download_type: DownloadType::Preinstall,
        current_tag,
        manifest_hash: tag.clone(),
        downloaded_chunks: HashMap::new(),
        completed_files: HashSet::new(),
    };
    save_download_state(&app_handle, &state)?;

    let handle = DownloadHandle::new();
    *active.0.lock().await = Some(handle.clone());

    let completed_files: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let saver = make_state_saver(&app_handle, &state, Arc::clone(&completed_files));
    let app_clone = app_handle.clone();

    let download_client = DownloadClient::new().0;
    let result = game_installer::preinstall_download(
        &download_client,
        &plan,
        &game_dir,
        &game_id,
        &vo_lang,
        handle,
        Arc::new(move |p| emit(&app_clone, p)),
        saver,
        HashMap::new(),
    )
    .await;

    clear_download_state(&app_handle);
    *active.0.lock().await = None;

    match result {
        Ok(_) => {
            emit(&app_handle, SophonProgress::Finished);
            Ok(())
        }
        Err(err) => install_result(Err(err), &app_handle),
    }
}

#[command]
pub async fn sophon_apply_preinstall(
    preinstall_tag: String,
    output_path: String,
    app_handle: AppHandle,
    client: State<'_, HttpClient>,
) -> Result<(), String> {
    // Reject path traversal in preinstall_tag before using it in file paths.
    game_installer::validate_asset_name(&preinstall_tag).map_err(|err| err.to_string())?;

    let game_dir = app_handle
        .path()
        .resolve(&output_path, BaseDirectory::AppData)
        .map_err(|err| err.to_string())?;

    let updater: Arc<dyn Fn(SophonProgress) + Send + Sync> = Arc::new({
        let app = app_handle.clone();
        move |p| emit(&app, p)
    });

    let apply_handle = DownloadHandle::new();
    game_installer::apply_preinstall(
        &client.0,
        &game_dir,
        &preinstall_tag,
        updater,
        &apply_handle,
    )
    .await
    .or_else(|err| match err {
        SophonError::Cancelled => Ok(()),
        other => {
            emit_error(&app_handle, &other);
            Err(other.to_string())
        }
    })
}

/// Resume an interrupted download using the saved state.
#[command]
pub async fn sophon_resume_download(
    app_handle: AppHandle,
    client: State<'_, HttpClient>,
    active: State<'_, ActiveDownload>,
) -> Result<(), String> {
    let state = load_download_state(&app_handle).ok_or("No download state found to resume")?;

    let game_dir = app_handle
        .path()
        .resolve(&state.output_path, BaseDirectory::AppData)
        .map_err(|err| err.to_string())?;

    let game_id = state.game_id.clone();
    let prev_chunks = state.downloaded_chunks.clone();
    let current_tag = state.current_tag.clone();
    let old_manifest_hash = state.manifest_hash.clone();

    emit(&app_handle, SophonProgress::FetchingManifest);

    if state.download_type == DownloadType::Preinstall {
        if let Some(ref saved_tag) = current_tag {
            let actual_tag = game_installer::read_installed_tag(&game_dir);
            if actual_tag.as_deref() != Some(saved_tag) {
                return Err("Cannot resume preinstall: installed game version changed since preinstall started. Delete preinstall data and start over.".to_string());
            }
        }

        let plan = game_installer::build_preinstall_plan(
            &client.0,
            &state.game_id,
            &state.vo_lang,
            &game_dir,
        )
        .await
        .map_err(|err| {
            emit_error(&app_handle, &err);
            err.to_string()
        })?;

        let resumed_state = DownloadState {
            game_id: state.game_id.clone(),
            vo_lang: state.vo_lang.clone(),
            output_path: state.output_path.clone(),
            download_type: DownloadType::Preinstall,
            current_tag,
            manifest_hash: plan.tag.clone(),
            downloaded_chunks: prev_chunks.clone(),
            completed_files: state.completed_files.clone(),
        };
        let completed_files: Arc<Mutex<HashSet<String>>> =
            Arc::new(Mutex::new(resumed_state.completed_files.clone()));
        let saver = make_state_saver(&app_handle, &resumed_state, Arc::clone(&completed_files));

        let handle = DownloadHandle::new();
        *active.0.lock().await = Some(handle.clone());

        let app_clone = app_handle.clone();
        let download_client = DownloadClient::new().0;
        let result = game_installer::preinstall_download(
            &download_client,
            &plan,
            &game_dir,
            &game_id,
            &state.vo_lang,
            handle,
            Arc::new(move |p| emit(&app_clone, p)),
            saver,
            prev_chunks,
        )
        .await;

        clear_download_state(&app_handle);
        *active.0.lock().await = None;

        return match result {
            Ok(_) => {
                emit(&app_handle, SophonProgress::Finished);
                Ok(())
            }
            Err(err) => install_result(Err(err), &app_handle),
        };
    }

    let (installers, deleted_files, tag, manifest_hash) = match state.download_type {
        DownloadType::Fresh => {
            let (installers, tag, new_manifest_hash) =
                game_installer::build_installers(&client.0, &state.game_id, &state.vo_lang)
                    .await
                    .map_err(|err| {
                        emit_error(&app_handle, &err);
                        err.to_string()
                    })?;
            (installers, vec![], tag, new_manifest_hash)
        }
        DownloadType::Update => {
            let ct = current_tag
                .clone()
                .ok_or("No current tag in resume state for update")?;
            let (installers, deleted_files, tag, new_manifest_hash) =
                game_installer::build_update_installers(
                    &client.0,
                    &state.game_id,
                    &state.vo_lang,
                    &ct,
                    &game_dir,
                )
                .await
                .map_err(|err| {
                    emit_error(&app_handle, &err);
                    err.to_string()
                })?;
            (installers, deleted_files, tag, new_manifest_hash)
        }
        DownloadType::Preinstall => unreachable!(),
    };

    let manifest_changed = old_manifest_hash != manifest_hash;
    let resumed_chunks = if manifest_changed {
        if delete_chunks_dir(&app_handle, &state.output_path) {
            log::info!("Deleted stale chunks directory due to manifest change");
        }
        HashMap::new()
    } else {
        prev_chunks
    };

    let resumed_state = DownloadState {
        game_id: state.game_id.clone(),
        vo_lang: state.vo_lang.clone(),
        output_path: state.output_path.clone(),
        download_type: state.download_type,
        current_tag,
        manifest_hash,
        downloaded_chunks: resumed_chunks,
        completed_files: state.completed_files.clone(),
    };
    let completed_files: Arc<Mutex<HashSet<String>>> =
        Arc::new(Mutex::new(resumed_state.completed_files.clone()));
    let saver = make_state_saver(&app_handle, &resumed_state, Arc::clone(&completed_files));

    let handle = DownloadHandle::new();
    *active.0.lock().await = Some(handle.clone());

    let app_clone = app_handle.clone();
    let vo_langs: Vec<String> = vec![state.vo_lang.clone()];
    let result = game_installer::install(
        installers,
        &game_dir,
        deleted_files,
        &tag,
        game_installer::ResumeContext {
            prev_manifest_hash: old_manifest_hash,
            prev_downloaded_chunks: resumed_state.downloaded_chunks,
        },
        game_installer::InstallOptions {
            is_preinstall: false,
            is_resume: true,
            handle,
        },
        game_installer::InstallCallbacks {
            updater: Arc::new(move |p| emit(&app_clone, p)),
            state_saver: saver,
            completed_files,
        },
        &game_id,
        &vo_langs,
    )
    .await;

    clear_download_state(&app_handle);
    *active.0.lock().await = None;

    match result {
        Ok(()) => {
            let plugin_emit = app_handle.clone();
            let plugin_updater: Arc<dyn Fn(SophonProgress) + Send + Sync> =
                Arc::new(move |p| emit(&plugin_emit, p));
            if let Err(err) = game_installer::install_plugins(&client.0, &game_dir, &game_id, {
                let u = plugin_updater.clone();
                move |p| u(p)
            })
            .await
            {
                log::warn!("Plugin installation failed: {err}");
                emit_error(&app_handle, &err);
            }
            if let Err(err) =
                game_installer::install_channel_sdks(&client.0, &game_dir, &game_id, {
                    let u = plugin_updater.clone();
                    move |p| u(p)
                })
                .await
            {
                log::warn!("Channel SDK installation failed: {err}");
                emit_error(&app_handle, &err);
            }
            emit(&app_handle, SophonProgress::Finished);
            Ok(())
        }
        Err(err) => install_result(Err(err), &app_handle),
    }
}

/// Checks if there is a downloadable state to resume.
#[command]
pub async fn sophon_has_resume_state(app_handle: AppHandle) -> bool {
    load_download_state(&app_handle).is_some()
}

/// Returns details about the resumable download state, if any.
#[command]
pub async fn sophon_get_resume_info(app_handle: AppHandle) -> Option<ResumeInfo> {
    load_download_state(&app_handle).map(|s| ResumeInfo {
        game_id: s.game_id,
        download_type: s.download_type,
    })
}

/// Pauses the active download.
#[command]
pub async fn sophon_pause(active: State<'_, ActiveDownload>) -> Result<(), String> {
    let guard = active.0.lock().await;
    let h = guard.as_ref().ok_or("No active download")?;
    h.pause();
    Ok(())
}

/// Resumes a paused download.
#[command]
pub async fn sophon_resume(active: State<'_, ActiveDownload>) -> Result<(), String> {
    let guard = active.0.lock().await;
    let h = guard.as_ref().ok_or("No active download")?;
    h.resume();
    Ok(())
}

/// Cancels the active download.
#[command]
pub async fn sophon_cancel(active: State<'_, ActiveDownload>) -> Result<(), String> {
    let guard = active.0.lock().await;
    let h = guard.as_ref().ok_or("No active download")?;
    h.cancel();
    Ok(())
}

/// Checks if an update is available for the game.
#[command]
pub async fn sophon_check_update(
    game_id: String,
    vo_lang: String,
    output_path: String,
    app_handle: AppHandle,
    client: State<'_, HttpClient>,
) -> Result<UpdateInfo, String> {
    let game_dir = app_handle
        .path()
        .resolve(&output_path, BaseDirectory::AppData)
        .map_err(|err| err.to_string())?;

    map_sophon_error(
        game_installer::check_update(&client.0, &game_id, &vo_lang, &game_dir).await,
        &app_handle,
    )
}

/// Verifies the integrity of installed game files and re-downloads any
/// corrupted ones.
#[command]
pub async fn sophon_verify_integrity(
    game_id: String,
    vo_lang: String,
    output_path: String,
    app_handle: AppHandle,
    client: State<'_, HttpClient>,
) -> Result<(), String> {
    let game_dir = app_handle
        .path()
        .resolve(&output_path, BaseDirectory::AppData)
        .map_err(|err| err.to_string())?;

    let app_clone = app_handle.clone();
    map_sophon_error(
        game_installer::verify_integrity(&client.0, &game_id, &vo_lang, &game_dir, move |p| {
            emit(&app_clone, p)
        })
        .await,
        &app_handle,
    )
}

fn emit(app: &AppHandle, progress: SophonProgress) {
    if let Err(err) = app.emit("sophon://progress", progress) {
        log::error!("Failed to emit progress event: {err}");
    }
}

/// Emits a structured error event across the Tauri IPC boundary.
fn emit_error(app: &AppHandle, error: &SophonError) {
    let _ = app.emit("sophon://error", CommandError::from(error));
    emit(
        app,
        SophonProgress::Error {
            message: error.to_string(),
        },
    );
}

/// Handle the final install result. Success and cancellation both return
/// `Ok(())`; other errors are emitted and returned as `Err(string)`.
fn install_result(result: Result<(), SophonError>, app: &AppHandle) -> Result<(), String> {
    match result {
        Ok(()) => Ok(()),
        Err(SophonError::Cancelled) => Ok(()),
        Err(err) => {
            emit_error(app, &err);
            Err(err.to_string())
        }
    }
}

/// Map `SophonResult<T>` to `Result<T, String>` and emit a structured error
/// event.
fn map_sophon_error<T>(result: Result<T, SophonError>, app: &AppHandle) -> Result<T, String> {
    result.map_err(|err| {
        emit_error(app, &err);
        err.to_string()
    })
}
