#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::Manager;
mod admin;
mod artwork_sources;
mod common;
mod discovery;
mod dlss;
mod downloads;
mod graphics;
mod installation;
mod library;
mod library_tools;
mod media;
mod mods;
#[cfg(test)]
mod native_tests;
mod os;
mod paradox;
mod runtime;
mod remote;
mod saves;
mod sounds;
mod steam;
mod transfer;
mod universal;
mod updater;
mod workspace;
mod workshop;
struct NativeState {
    profile: PathBuf,
    legacy: PathBuf,
}
fn allowed(operation: &str) -> bool {
    matches!(
        operation,
        "bootstrap"
            | "import_legacy"
            | "scan"
            | "save_game"
            | "remove_game"
            | "pin"
            | "launch"
            | "artwork"
            | "recommendations"
            | "deals"
            | "store_details"
            | "open_store"
            | "project_link"
            | "remote_config"
            | "health"
            | "workspace"
            | "library_tools"
            | "saves"
            | "network"
            | "admin"
            | "autostart"
            | "open_folder"
            | "game_update"
            | "mods"
            | "graphics"
            | "dlss"
            | "sounds"
            | "runtime"
            | "transfer"
            | "steam"
            | "window_action"
            | "universal"
    )
}
#[tauri::command]
async fn request(
    operation: String,
    payload: Option<Value>,
    state: tauri::State<'_, NativeState>,
    app: tauri::AppHandle,
) -> Result<Value, String> {
    if operation == "app_update" {
        return updater::handle(app, &state.profile, &payload.unwrap_or(json!({}))).await;
    }
    if !allowed(&operation) {
        return Err("Unknown operation".into());
    }
    let profile = state.profile.clone();
    let legacy = state.legacy.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let p = payload.unwrap_or(json!({}));
        if matches!(operation.as_str(), "recommendations"|"deals"|"mods"|"network"|"sounds"|"graphics"|"universal") && !remote::enabled(&profile,&operation) {
            return Err("This feature is temporarily unavailable".into());
        }
        if library::handles(&operation) {
            return library::handle(&profile, &legacy, &operation, &p);
        }
        if media::handles(&operation) {
            return media::handle(&profile, &operation, &p);
        }
        match operation.as_str() {
            "library_tools" => library_tools::handle(&profile, &p),
            "workspace" => workspace::handle(&profile, &p),
            "saves" => saves::handle(&profile, &p),
            "mods" => mods::handle(&profile, &p),
            "graphics" => graphics::handle(&profile, &p),
            "dlss" => dlss::handle(&profile, &p),
            "sounds" => sounds::handle(&profile, &p),
            "universal" => universal::handle(&profile, &p),
            "runtime" => runtime::handle(&p),
            "scan" => discovery::scan(&profile),
            "launch" => runtime::launch(&profile, &p, &app),
            "transfer" => transfer::handle(&profile, &p),
            "steam" => steam::handle(&profile, &p),
            "network" => {
                if common::str(&p, "action") == "install" {
                    os::install_warp(&profile, &p)
                } else {
                    os::network(&p)
                }
            }
            "admin" => admin::handle(&profile, &p),
            "autostart" => os::autostart(p["enabled"].as_bool()).map(|v| json!({"enabled":v})),
            "open_folder" => os::open_folder(&profile, &p),
            "game_update" => os::game_update(&profile, &p),
            "open_store" => os::open_store(&p),
            "project_link" => os::open_project_link(&profile, &p),
            "remote_config" => remote::handle(&profile, &p),
            "health" => {
                Ok(json!({"engine":"rust","missingBindings":[],"compatibilityBridge":false}))
            }
            "window_action" => {
                let window = app.get_webview_window("main").ok_or("Window not found")?;
                match common::str(&p, "action") {
                    "fullscreen" => window
                        .set_fullscreen(p["enabled"] == true)
                        .map_err(common::err)?,
                    "hide" => window.hide().map_err(common::err)?,
                    "quit" => app.exit(0),
                    _ => {
                        window.show().map_err(common::err)?;
                        window.set_focus().map_err(common::err)?;
                    }
                }
                Ok(json!({"done":true}))
            }
            _ => Err("Unknown operation".into()),
        }
    })
    .await
    .map_err(|_| "The operation could not be completed".to_string())?
}
fn main() {
    let args:Vec<_>=std::env::args_os().collect();
    if args.get(1).is_some_and(|s|s=="--verify-remote-config") {
        let result=args.get(2).ok_or("Supply a signed manifest path".to_string()).and_then(|p|remote::verify_file(std::path::Path::new(p)));
        match result {Ok(value)=>{println!("{value}");return;},Err(message)=>{eprintln!("{message}");std::process::exit(1);}}
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .to_path_buf();
            let profile = if let Some(path) = std::env::var_os("PLAYSPACE_NEXT_PROFILE") {
                PathBuf::from(path)
            } else if cfg!(debug_assertions) {
                project.join(".local-data")
            } else {
                app.path().app_local_data_dir()?
            };
            let legacy = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default())
                .join("GameLauncherPRO/games.json");
            let english = common::language(&profile) == "en";
            let menu = tauri::menu::Menu::with_items(
                app,
                &[
                    &tauri::menu::MenuItem::with_id(
                        app,
                        "show",
                        if english {
                            "Show PlaySpace"
                        } else {
                            "Показать PlaySpace"
                        },
                        true,
                        None::<&str>,
                    )?,
                    &tauri::menu::MenuItem::with_id(
                        app,
                        "quit",
                        if english { "Quit" } else { "Выйти" },
                        true,
                        None::<&str>,
                    )?,
                ],
            )?;
            let mut tray = tauri::tray::TrayIconBuilder::new()
                .tooltip("PlaySpace 2.0")
                .menu(&menu)
                .on_menu_event(|app, event| {
                    if event.id.as_ref() == "quit" {
                        app.exit(0)
                    } else if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            let close_profile = profile.clone();
            if let Some(window) = app.get_webview_window("main") {
                let handle = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        if common::prefs(&close_profile)
                            .ok()
                            .is_some_and(|p| p["CloseBehavior"] == 1)
                        {
                            api.prevent_close();
                            let _ = handle.hide();
                        }
                    }
                });
            }
            let installation_profile=profile.clone();
            std::thread::spawn(move || loop {
                let _=installation::reconcile(&installation_profile);
                std::thread::sleep(std::time::Duration::from_secs(15));
            });
            let remote_profile=profile.clone();
            let _=remote::view(&remote_profile);
            std::thread::spawn(move || loop {
                remote::refresh(&remote_profile,false);
                std::thread::sleep(std::time::Duration::from_secs(60));
            });
            app.manage(NativeState { profile, legacy });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![request])
        .run(tauri::generate_context!())
        .expect("PlaySpace Next could not start");
}
