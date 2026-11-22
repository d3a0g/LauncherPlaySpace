use crate::{
    common::*,
    library::{set, text},
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn backend(g: &Value) -> &'static str {
    let exe = text(g, "ExePath").to_lowercase();
    if text(g, "SteamAppId") == "1091500" || exe.ends_with("cyberpunk2077.exe") {
        "Cyberpunk"
    } else if text(g, "SteamAppId") == "730" {
        "Source2"
    } else if Path::new(&exe)
        .parent()
        .is_some_and(|p| p.join("UnityPlayer.dll").is_file())
    {
        "Unity"
    } else {
        let a = crate::mods::adapters();
        if let Some(a) = a["thunderstore"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| str(a, "SteamId") == text(g, "SteamAppId"))
        {
            if a["IsUnreal"] == true {
                "Unreal"
            } else {
                "Unity"
            }
        } else if exe.contains("-win64-shipping")
            || PathBuf::from(text(g, "InstallDirectory"))
                .join("Engine")
                .exists()
        {
            "Unreal"
        } else {
            "None"
        }
    }
}
fn profile(g: &Value) -> Value {
    let mut p = if g["Performance"].is_object() {
        g["Performance"].clone()
    } else {
        json!({})
    };
    for (k, v) in [
        ("TargetFps", json!(60)),
        ("RenderScale", json!(100)),
        ("Quality", json!("Средние")),
        ("ResolutionWidth", json!(1920)),
        ("ResolutionHeight", json!(1080)),
        ("WindowMode", json!("Безрамочный")),
        ("VSync", json!(false)),
        ("ApplyOnLaunch", json!(false)),
        ("NativeOptions", json!({})),
        ("DetailLevels", json!({})),
    ] {
        if p.get(k).is_none() {
            p[k] = v;
        }
    }
    for (k, fallback) in [("ResolutionWidth", 1920), ("ResolutionHeight", 1080)] {
        if p[k] == 0 {
            p[k] = json!(fallback);
        }
    }
    p
}
fn validate(p: &Value) -> Result<()> {
    for (k, min, max) in [
        ("TargetFps", 30, 360),
        ("RenderScale", 50, 100),
        ("ResolutionWidth", 640, 16384),
        ("ResolutionHeight", 480, 16384),
    ] {
        if !p[k].as_u64().is_some_and(|n| (min..=max).contains(&n)) {
            return Err(format!("Invalid graphics setting: {k}"));
        }
    }
    if !matches!(
        str(p, "WindowMode"),
        "Оконный" | "Безрамочный" | "Полноэкранный"
    ) || !matches!(str(p, "Quality"), "Низкие" | "Средние" | "Высокие")
    {
        return Err("Invalid graphics profile".into());
    }
    Ok(())
}
fn config(g: &Value, p: &Value) -> Option<PathBuf> {
    let custom = str(p, "ConfigFile");
    if !custom.is_empty() {
        return Some(PathBuf::from(custom));
    }
    if backend(g) == "Cyberpunk" {
        let path = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default())
            .join("CD Projekt Red/Cyberpunk 2077/UserSettings.json");
        return path.exists().then_some(path);
    }
    let a = crate::mods::adapters();
    let adapter = a["thunderstore"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| str(a, "SteamId") == text(g, "SteamAppId"));
    let project = adapter
        .map(|a| str(a, "UnrealProject"))
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| {
            Path::new(&text(g, "ExePath"))
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .replace("-Win64-Shipping", "")
        });
    for platform in ["Windows", "WindowsNoEditor"] {
        let path = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default())
            .join(&project)
            .join(format!("Saved/Config/{platform}/GameUserSettings.ini"));
        if path.exists() {
            return Some(path);
        }
    }
    None
}
fn ini(input: &str, p: &Value) -> Result<String> {
    let mut lines: Vec<String> = input
        .replace("\r\n", "\n")
        .split('\n')
        .map(String::from)
        .collect();
    let section = lines
        .iter()
        .find(|s| s.starts_with('[') && s.contains("GameUserSettings"))
        .cloned()
        .ok_or("GameUserSettings section not found")?;
    let mut put = |section: &str, key: &str, value: String| {
        let start = if let Some(i) = lines.iter().position(|l| l.trim() == section) {
            i
        } else {
            lines.push(section.into());
            lines.len() - 1
        };
        let end = (start + 1..lines.len())
            .find(|i| lines[*i].trim().starts_with('['))
            .unwrap_or(lines.len());
        if let Some(i) = (start + 1..end).find(|i| {
            lines[*i]
                .split('=')
                .next()
                .is_some_and(|k| k.trim().eq_ignore_ascii_case(key))
        }) {
            lines[i] = format!("{key}={value}");
        } else {
            lines.insert(end, format!("{key}={value}"));
        }
    };
    for key in ["ResolutionSizeX", "LastUserConfirmedResolutionSizeX"] {
        put(&section, key, p["ResolutionWidth"].to_string());
    }
    for key in ["ResolutionSizeY", "LastUserConfirmedResolutionSizeY"] {
        put(&section, key, p["ResolutionHeight"].to_string());
    }
    let mode = match str(p, "WindowMode") {
        "Полноэкранный" => "0",
        "Оконный" => "2",
        _ => "1",
    };
    for key in ["FullscreenMode", "LastConfirmedFullscreenMode"] {
        put(&section, key, mode.into());
    }
    put(
        &section,
        "FrameRateLimit",
        if p["UnlimitedFps"] == true {
            "0".into()
        } else {
            p["TargetFps"].to_string()
        },
    );
    put(
        &section,
        "bUseVSync",
        if p["VSync"] == true { "True" } else { "False" }.into(),
    );
    put(&section, "bUseDynamicResolution", "False".into());
    let level = match str(p, "Quality") {
        "Низкие" => 0,
        "Высокие" => 2,
        _ => 1,
    };
    for key in [
        "ViewDistanceQuality",
        "AntiAliasingQuality",
        "ShadowQuality",
        "GlobalIlluminationQuality",
        "ReflectionQuality",
        "PostProcessQuality",
        "TextureQuality",
        "EffectsQuality",
        "FoliageQuality",
        "ShadingQuality",
    ] {
        let custom = p["DetailLevels"][key]
            .as_u64()
            .filter(|n| *n <= 3)
            .unwrap_or(level);
        put(
            "[ScalabilityGroups]",
            &format!("sg.{key}"),
            custom.to_string(),
        );
    }
    put(
        "[ScalabilityGroups]",
        "sg.ResolutionQuality",
        p["RenderScale"].to_string(),
    );
    Ok(lines.join("\r\n"))
}
fn native_options(value: &Value) -> Vec<Value> {
    let mut options = vec![];
    for group in value["data"].as_array().into_iter().flatten() {
        let name = str(group, "group_name");
        if !(name.starts_with("/graphics") || name == "/video/display") {
            continue;
        }
        for option in group["options"].as_array().into_iter().flatten() {
            let key = str(option, "name");
            if matches!(key, "QuickPresets" | "OutputMonitor" | "") {
                continue;
            }
            let choices = if option["type"] == "bool" {
                json!(["false", "true"])
            } else {
                option["values"].clone()
            };
            options.push(json!({"key":format!("{name}|{key}"),"name":key,"value":option["value"],"choices":choices,"minimum":option["min_value"],"maximum":option["max_value"],"type":option["type"]}));
        }
    }
    options.truncate(100);
    options
}
fn apply(g: &Value, p: &Value) -> Result<()> {
    validate(p)?;
    crate::saves::closed(g)?;
    let path = config(g, p).ok_or("Select this game's existing configuration file")?;
    no_links(&path)?;
    if fs::metadata(&path).map_err(err)?.len() > 4 * 1024 * 1024 {
        return Err("Configuration is too large".into());
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    if !matches!(name.as_ref(), "GameUserSettings.ini" | "UserSettings.json") {
        return Err("Unsupported configuration file".into());
    }
    let bytes = fs::read(&path).map_err(err)?;
    let updated = if name == "UserSettings.json" {
        let mut v: Value = serde_json::from_slice(&bytes).map_err(err)?;
        let defs = native_options(&v);
        let mut keys = std::collections::HashSet::new();
        if defs.iter().any(|d| !keys.insert(str(d, "key"))) {
            return Err("Duplicate native graphics option".into());
        }
        for (key, value) in p["NativeOptions"]
            .as_object()
            .unwrap_or(&serde_json::Map::new())
        {
            let def = defs
                .iter()
                .find(|o| str(o, "key") == key)
                .ok_or("Unknown native graphics option")?;
            let text = value.as_str().ok_or("Invalid option value")?;
            let chosen = if let Some(choices) = def["choices"].as_array() {
                if !choices
                    .iter()
                    .any(|c| c.as_str() == Some(text) || c.to_string() == text)
                {
                    return Err("Unsupported native option value".into());
                }
                match str(def, "type") {
                    "bool" => json!(text == "true"),
                    "int_list" => json!(text.parse::<i64>().map_err(err)?),
                    _ => json!(text),
                }
            } else {
                let number = text.parse::<f64>().map_err(err)?;
                if !number.is_finite()
                    || number < def["minimum"].as_f64().ok_or("Missing numeric range")?
                    || number > def["maximum"].as_f64().ok_or("Missing numeric range")?
                {
                    return Err("Native option outside supported range".into());
                }
                if str(def, "type") == "int" {
                    if number.fract() != 0. {
                        return Err("Native integer option requires a whole number".into());
                    }
                    json!(number as i64)
                } else {
                    json!(number)
                }
            };
            let gpu = crate::dlss::gpu().to_lowercase();
            if key.ends_with("|DLSSFrameGen") && text == "true" {
                let generation = regex::Regex::new(r"rtx\s*(\d{2})")
                    .unwrap()
                    .captures(&gpu)
                    .and_then(|c| c[1].parse::<u32>().ok());
                if !generation.is_some_and(|g| g >= 40) {
                    return Err(
                        "NVIDIA frame generation requires a supported RTX 40-series or newer GPU"
                            .into(),
                    );
                }
            }
            if key.ends_with("|DLSS") && text != "Off" && !gpu.contains("geforce rtx") {
                return Err("DLSS requires a compatible GeForce RTX GPU".into());
            }
            let (group, name) = key.split_once('|').ok_or("Invalid native setting key")?;
            for entry in v["data"]
                .as_array_mut()
                .ok_or("Invalid Cyberpunk configuration")?
            {
                if str(entry, "group_name") == group {
                    for option in entry["options"]
                        .as_array_mut()
                        .ok_or("Invalid native options")?
                    {
                        if str(option, "name") == name {
                            option["value"] = chosen.clone();
                            if option.get("index").is_some() {
                                if let Some(choices) = def["choices"].as_array() {
                                    option["index"] = json!(choices
                                        .iter()
                                        .position(|c| c == &chosen || c.as_str() == Some(text))
                                        .unwrap_or(0));
                                }
                            }
                        }
                    }
                }
            }
        }
        serde_json::to_vec_pretty(&v).map_err(err)?
    } else {
        ini(std::str::from_utf8(&bytes).map_err(err)?, p)?.into_bytes()
    };
    let backup = path.with_file_name(format!("{name}.playspace-backup"));
    no_links(&backup)?;
    if !backup.exists() {
        let mut f = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&backup)
            .map_err(err)?;
        std::io::Write::write_all(&mut f, &bytes).map_err(err)?;
    }
    atomic(&path, &updated)
}
pub fn checkpoint(g:&Value) -> Result<Option<(PathBuf,Vec<u8>)>> {
    let settings = profile(g);
    if settings["ApplyOnLaunch"] != true { return Ok(None); }
    let Some(path) = config(g,&settings) else { return Ok(None); };
    no_links(&path)?;
    if fs::metadata(&path).map_err(err)?.len() > 8 * 1024 * 1024 {
        return Err("Graphics configuration exceeds 8 MB".into());
    }
    let bytes = fs::read(&path).map_err(err)?;
    Ok(Some((path,bytes)))
}
pub fn prepare(g: &Value) -> Result<Vec<String>> {
    let p = profile(g);
    if p["ApplyOnLaunch"] != true {
        return Ok(vec![]);
    }
    validate(&p)?;
    let backend = backend(g);
    let w = p["ResolutionWidth"].to_string();
    let h = p["ResolutionHeight"].to_string();
    let mode = str(&p, "WindowMode");
    let args: Vec<String> = match backend {
        "Unity" => vec![
            "-screen-width".into(),
            w,
            "-screen-height".into(),
            h,
            "-screen-fullscreen".into(),
            if mode == "Оконный" { "0" } else { "1" }.into(),
            "-window-mode".into(),
            if mode == "Полноэкранный" {
                "exclusive"
            } else {
                "borderless"
            }
            .into(),
        ],
        "Source2" => {
            let mut args = vec![
                "-w".into(),
                w,
                "-h".into(),
                h,
                if mode == "Полноэкранный" {
                    "-fullscreen"
                } else {
                    "-windowed"
                }
                .into(),
            ];
            if mode == "Безрамочный" {
                args.push("-noborder".into())
            }
            args.extend([
                "+fps_max".into(),
                if p["UnlimitedFps"] == true {
                    "0".into()
                } else {
                    p["TargetFps"].to_string()
                },
            ]);
            args
        }
        "Unreal" => {
            if config(g, &p).is_some() {
                apply(g, &p)?;
                vec![format!("-ResX={w}"), format!("-ResY={h}")]
            } else {
                if mode == "Безрамочный" {
                    return Err("Select GameUserSettings.ini to use borderless mode".into());
                }
                vec![
                    format!("-ResX={w}"),
                    format!("-ResY={h}"),
                    if mode == "Оконный" {
                        "-windowed"
                    } else {
                        "-fullscreen"
                    }
                    .into(),
                ]
            }
        }
        "Cyberpunk" => {
            apply(g, &p)?;
            vec![]
        }
        _ => return Err("No graphics adapter for this game".into()),
    };
    Ok(args)
}
pub fn handle(profile_folder: &Path, p: &Value) -> Result<Value> {
    let mut g = game(profile_folder, p)?;
    let mut settings = profile(&g);
    let action = str(p, "action");
    if action == "save" {
        let values = p["value"].as_object().ok_or("Invalid graphics profile")?;
        for (key, value) in values {
            if settings.get(key).is_some() || matches!(key.as_str(), "ConfigFile" | "UnlimitedFps")
            {
                settings[key] = value.clone();
            }
        }
        validate(&settings)?;
        update_game(profile_folder, str(p, "id"), |g| {
            set(g, "Performance", settings.clone());
            Ok(())
        })?;
        set(&mut g, "Performance", settings.clone());
    }
    if action == "apply" {
        apply(&g, &settings)?;
        history(
            profile_folder,
            &g,
            "Graphics",
            "Graphics configuration applied",
        );
    }
    if action == "restore" {
        crate::saves::closed(&g)?;
        let file = config(&g, &settings).ok_or("Configuration not found")?;
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        if !matches!(name.as_ref(), "GameUserSettings.ini" | "UserSettings.json") {
            return Err("Unsupported configuration file".into());
        }
        let backup = file.with_file_name(format!("{name}.playspace-backup"));
        no_links(&backup)?;
        atomic(&file, &fs::read(&backup).map_err(err)?)?;
    }
    let config = config(&g, &settings);
    let options = config
        .as_ref()
        .filter(|p| p.file_name().is_some_and(|n| n == "UserSettings.json"))
        .and_then(|p| read(p, Value::Null).ok())
        .map(|v| native_options(&v))
        .unwrap_or_default();
    let system = sysinfo::System::new_all();
    Ok(
        json!({"profile":settings,"backend":backend(&g),"config":config,"nativeOptions":options,"hardware":{"cpus":system.cpus().iter().map(|c|c.brand()).collect::<std::collections::HashSet<_>>(),"ram":system.total_memory()},"advancedCompatibility":true}),
    )
}
