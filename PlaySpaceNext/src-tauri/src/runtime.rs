use crate::{
    common::*,
    downloads,
    library::{set, text},
};
use serde_json::{json, Value};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::process::CommandExt;
use std::{
    collections::HashMap,
    path::Path,
    process::Command,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};
static SESSIONS: LazyLock<Mutex<HashMap<String, Value>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
fn args(input: &str) -> Result<Vec<String>> {
    let mut values = vec![];
    let mut value = String::new();
    let mut quoted = false;
    let mut slashes = 0;
    for c in input.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        if c == '"' {
            value.extend(std::iter::repeat_n('\\', slashes / 2));
            if slashes % 2 == 1 {
                value.push('"')
            } else {
                quoted = !quoted;
            }
            slashes = 0;
            continue;
        }
        value.extend(std::iter::repeat_n('\\', slashes));
        slashes = 0;
        if c.is_whitespace() && !quoted {
            if !value.is_empty() {
                values.push(std::mem::take(&mut value));
            }
        } else {
            value.push(c)
        }
    }
    value.extend(std::iter::repeat_n('\\', slashes));
    if quoted {
        return Err("Unclosed quote in launch arguments".into());
    }
    if !value.is_empty() {
        values.push(value);
    }
    Ok(values)
}
pub fn sessions() -> Value {
    json!(SESSIONS
        .lock()
        .unwrap()
        .values()
        .cloned()
        .collect::<Vec<_>>())
}
pub fn launch(profile: &Path, p: &Value, app: &tauri::AppHandle) -> Result<Value> {
    let startup_guard=crate::library::lock(profile)?;
    let g = game(profile, p)?;
    let id = text(&g, "LibraryId");
    use sha2::{Digest, Sha256};
    let key = format!("{:X}", Sha256::digest(id.as_bytes()));
    let lock = profile.join(format!("sessions/{key}.lock"));
    no_links(&lock)?;
    std::fs::create_dir_all(lock.parent().unwrap()).map_err(err)?;
    let session_guard = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .share_mode(0)
        .open(lock)
        .map_err(|_| "This game already has an active session".to_string())?;
    drop(startup_guard);
    if SESSIONS.lock().unwrap().contains_key(&id) {
        return Err("This game already has an active session".into());
    }
    crate::saves::closed(&g)?;
    crate::saves::before_launch(profile, &g)?;
    let mut arguments = crate::graphics::prepare(&g)?;
    arguments.extend(args(&text(&g, "LaunchArguments"))?);
    let exe = text(&g, "ExePath");
    let steam = text(&g, "SteamAppId");
    let root = crate::mods::game_root(&g).ok();
    if let Some(root) = &root {
        if let Ok(v) = read(&root.join(".playspace-mods/installed.json"), Value::Null) {
            if v["Packages"]
                .get("Thunderstore-unreal_shimloader")
                .is_some()
            {
                for (option, folder) in [
                    ("--mod-dir", "mods"),
                    ("--pak-dir", "paks"),
                    ("--cfg-dir", "cfg"),
                    ("--overlay-dir", "overlay"),
                ] {
                    let dir = safe(root, &format!(".playspace-mods/unreal/{folder}"))?;
                    std::fs::create_dir_all(&dir).map_err(err)?;
                    arguments.push(option.into());
                    arguments.push(dir.to_string_lossy().to_string());
                }
            }
        }
    }
    let direct = steam.is_empty();
    let mut child = if !direct {
        if let Some(steam_exe) = crate::discovery::steam()
            .map(|p| p.join("steam.exe"))
            .filter(|p| p.is_file())
        {
            let mut command = Command::new(steam_exe);
            command
                .arg("-applaunch")
                .arg(&steam)
                .args(&arguments)
                .creation_flags(0x08000000);
            Some(command.spawn().map_err(err)?)
        } else {
            crate::os::open_url(&format!("steam://rungameid/{steam}"))?;
            None
        }
    } else {
        let path = Path::new(&exe);
        if !path.is_file()
            || !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        {
            return Err("The game's executable was not found".into());
        }
        let mut command = Command::new(path);
        command
            .args(&arguments)
            .current_dir(path.parent().unwrap())
            .creation_flags(0x08000000);
        Some(command.spawn().map_err(err)?)
    };
    let pid = child.as_ref().map(|c| c.id());
    let window = app.clone();
    if prefs(profile)?["HideWhilePlaying"] == true {
        use tauri::Manager;
        if let Some(w) = window.get_webview_window("main") {
            let _ = w.hide();
        }
    }
    SESSIONS.lock().unwrap().insert(
        id.clone(),
        json!({"id":id,"name":text(&g,"Name"),"state":"starting","seconds":0}),
    );
    let profile = profile.to_path_buf();
    let game = g.clone();
    std::thread::spawn(move || {
        let _session_guard = session_guard;
        let began = Instant::now();
        let mut last = Instant::now();
        let mut observed = false;
        let mut saved = 0.;
        let mut due = None;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let mut system = sysinfo::System::new_all();
            system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
            let tracking = text(&game, "TrackingExecutable");
            let running = system.processes().values().any(|proc| {
                if direct && pid.is_some_and(|id| proc.pid().as_u32() == id) {
                    return true;
                }
                if !tracking.is_empty() {
                    return proc
                        .name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&tracking);
                }
                if let Some(path) = proc.exe() {
                    if let Some(root) = &root {
                        let current = path.to_string_lossy().to_lowercase();
                        let root = root.to_string_lossy().to_lowercase();
                        return current.starts_with(&(root + "\\"))
                            && !current.contains("crash")
                            && !current.ends_with("launcher.exe");
                    }
                }
                false
            });
            if running {
                if !observed {
                    observed = true;
                    last = Instant::now();
                    let _ = update_game(&profile, &id, |g| {
                        set(g, "LastLaunchUtc", json!(now()));
                        Ok(())
                    });
                    history(&profile, &game, "Launch", "Game session started");
                    if let Some(s) = SESSIONS.lock().unwrap().get_mut(&id) {
                        s["state"] = json!("running");
                    }
                }
                let elapsed = last.elapsed().as_secs_f64();
                if elapsed >= 5. {
                    if update_game(&profile, &id, |g| {
                        let old = g["PlayedSeconds"].as_f64().unwrap_or(0.);
                        set(g, "PlayedSeconds", json!(old + elapsed));
                        Ok(())
                    })
                    .is_ok()
                    {
                        saved += elapsed;
                        last = Instant::now();
                    }
                }
                if let Some(s) = SESSIONS.lock().unwrap().get_mut(&id) {
                    s["seconds"] = json!(saved + last.elapsed().as_secs_f64());
                }
                if let Ok(settings) = read(&profile.join("game-sounds.json"), Value::Null) {
                    let rule = settings["Games"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|r| r["GameId"] == id && r["Enabled"] == true);
                    if settings["Enabled"] == true {
                        if let Some(rule) = rule {
                            let min = rule["MinimumSeconds"]
                                .as_u64()
                                .unwrap_or(45)
                                .clamp(10, 3600);
                            let max = rule["MaximumSeconds"]
                                .as_u64()
                                .unwrap_or(180)
                                .clamp(min, 3600);
                            let files = rule["Files"].as_array().cloned().unwrap_or_default();
                            let random = uuid::Uuid::new_v4().as_u128() as u64;
                            if let Some(time) = due {
                                if Instant::now() >= time && !files.is_empty() {
                                    let name = files[(random as usize) % files.len()]
                                        .as_str()
                                        .unwrap_or("");
                                    let _ = crate::sounds::play(&profile, name);
                                    due = None;
                                }
                            } else {
                                due = Some(
                                    Instant::now()
                                        + Duration::from_secs(min + random % (max - min + 1)),
                                );
                            }
                        }
                    } else {
                        due = None;
                        crate::sounds::stop();
                    }
                }
            } else if observed {
                let elapsed = last.elapsed().as_secs_f64();
                let _ = update_game(&profile, &id, |g| {
                    let old = g["PlayedSeconds"].as_f64().unwrap_or(0.);
                    set(g, "PlayedSeconds", json!(old + elapsed));
                    Ok(())
                });
                history(
                    &profile,
                    &game,
                    "Session",
                    &format!("Played {:.0} seconds", saved + elapsed),
                );
                if let Err(error)=crate::saves::after_playing(&profile,&game) {
                    history(&profile,&game,"Backup failed",&error);
                }
                break;
            } else if began.elapsed() > Duration::from_secs(300)
                || direct
                    && began.elapsed() > Duration::from_secs(30)
                    && child
                        .as_mut()
                        .is_some_and(|p| p.try_wait().ok().flatten().is_some())
            {
                history(
                    &profile,
                    &game,
                    "Launch failed",
                    "Game process was not detected",
                );
                break;
            }
        }
        SESSIONS.lock().unwrap().remove(&id);
        crate::sounds::stop();
        if prefs(&profile)
            .ok()
            .is_some_and(|p| p["RestoreAfterPlaying"] != false)
        {
            use tauri::Manager;
            if let Some(w) = window.get_webview_window("main") {
                let _ = w.show();
            }
        }
    });
    Ok(json!({"accepted":true,"engine":"rust"}))
}
pub fn handle(p: &Value) -> Result<Value> {
    match str(p, "action") {
        "cancel" => {
            downloads::cancel(str(p, "id"));
            Ok(json!({"cancelled":true}))
        }
        _ => Ok(json!({"downloads":downloads::items(),"sessions":sessions()})),
    }
}
