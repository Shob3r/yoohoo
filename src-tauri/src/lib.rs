use std::env;
use std::time::Duration;

use tauri::{Manager, command};

use crate::commands::{file_downloader, file_manager};
pub mod commands;
use crate::commands::sophon_downloader::ActiveDownload;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[tauri::cef_entry_point]
pub fn run() {
    apply_nvidia_workaround();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .max_blocking_threads(32)
        .thread_stack_size(512 * 1024)
        .enable_all()
        .build()
        .expect("build tokio runtime");
    tauri::async_runtime::set(runtime.handle().clone());
    std::mem::forget(runtime);

    tauri::Builder::<tauri::Cef>::default()
        .command_line_args([
            ("--disable-extensions", None),
            ("--disable-plugins", None),
            ("--disable-printing", None),
            ("--disable-component-update", None),
            ("--disable-background-networking", None),
            ("--disable-domain-reliability", None),
            ("--disable-default-apps", None),
            ("--disable-device-discovery-notifications", None),
            ("--disable-field-trial-config", None),
            ("--renderer-process-limit", Some("1")),
            (
                "--disable-features",
                Some(
                    "Translate,MediaRouter,OptimizationHints,PrivacySandboxSettings,BackForwardCache,MediaSessionService",
                ),
            ),
            ("--js-flags", Some("--max-old-space-size=256")),
        ])
        .command_line_args(nvidia_gpu_backend_args())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                window.unminimize().ok();
                window.set_focus().ok();
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_window_state::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .manage(commands::sophon_downloader::HttpClient(
            reqwest::Client::builder()
                .pool_max_idle_per_host(8)
                .pool_idle_timeout(Duration::from_secs(90))
                .tcp_nodelay(true)
                .tcp_keepalive(Duration::from_secs(30))
                .connect_timeout(Duration::from_secs(10))
                .read_timeout(Duration::from_secs(300))
                .user_agent(format!(
                    "{name}/{ver}",
                    name = env!("CARGO_PKG_NAME"),
                    ver = env!("CARGO_PKG_VERSION")
                ))
                .build()
                .unwrap(),
        )) // Required for sophon chunk downloading
        .manage(ActiveDownload(tokio::sync::Mutex::new(None)))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(tauri_plugin_log::log::LevelFilter::Info)
                .level_for("cef", tauri_plugin_log::log::LevelFilter::Warn)
                .build(),
        )
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(disable_shortcuts())
        .setup(|app| {
            #[cfg(target_os = "linux")]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                if !is_flatpak()
                    && let Err(e) = app.deep_link().register_all()
                {
                    eprintln!("Elysiae: Failed to register deep links: {e}");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            file_downloader::download_file,
            file_manager::extract_file,
            file_manager::get_dir_size,
            commands::sophon_downloader::commands::sophon_download,
            commands::sophon_downloader::commands::sophon_download_version,
            commands::sophon_downloader::commands::sophon_update,
            commands::sophon_downloader::commands::sophon_preinstall,
            commands::sophon_downloader::commands::sophon_apply_preinstall,
            commands::sophon_downloader::commands::sophon_resume_download,
            commands::sophon_downloader::commands::sophon_has_resume_state,
            commands::sophon_downloader::commands::sophon_get_resume_info,
            commands::sophon_downloader::commands::sophon_verify_integrity,
            commands::sophon_downloader::commands::sophon_pause,
            commands::sophon_downloader::commands::sophon_resume,
            commands::sophon_downloader::commands::sophon_cancel,
            commands::sophon_downloader::commands::sophon_check_update,
            elysiae_version,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn apply_nvidia_workaround() {
    if is_nvidia() {
        println!("Elysiae: Applying NVIDIA GPU Workaround");
        unsafe {
            std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1");
        };
    }
}

#[cfg(target_os = "linux")]
fn nvidia_gpu_backend_args() -> Vec<(&'static str, Option<&'static str>)> {
    if is_nvidia() {
        vec![("--enable-features", Some("Vulkan"))]
    } else {
        Vec::new()
    }
}

#[cfg(not(target_os = "linux"))]
fn nvidia_gpu_backend_args() -> Vec<(&'static str, Option<&'static str>)> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn is_flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

#[cfg(target_os = "linux")]
fn is_nvidia() -> bool {
    // If a NVIDIA graphics card is present, one of these two paths should exist
    std::path::Path::new("/proc/driver/nvidia/version").exists()
        || std::path::Path::new("/dev/nvidia0").exists()
}

#[command]
fn elysiae_version() -> String {
    env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "Unknown App Version".to_string())
}

#[cfg(debug_assertions)]
fn disable_shortcuts() -> tauri::plugin::TauriPlugin<tauri::Cef> {
    use tauri_plugin_prevent_default::Flags;

    tauri_plugin_prevent_default::Builder::new()
        .with_flags(Flags::empty())
        .build()
}

#[cfg(not(debug_assertions))]
fn disable_shortcuts() -> tauri::plugin::TauriPlugin<tauri::Cef> {
    use tauri_plugin_prevent_default::Flags;

    tauri_plugin_prevent_default::Builder::new()
        .with_flags(Flags::all())
        .build()
}
