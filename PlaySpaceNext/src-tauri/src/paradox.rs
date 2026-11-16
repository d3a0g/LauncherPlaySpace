use crate::{common::*, library::text};
use serde_json::{json, Value};
use std::os::windows::fs::OpenOptionsExt;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};
fn property(metadata: &str, key: &str) -> String {
    regex::Regex::new(&format!(
        r#"(?m)^\s*{}\s*=\s*"([^"\r\n]*)""#,
        regex::escape(key)
    ))
    .unwrap()
    .captures(metadata)
    .map(|c| c[1].to_string())
    .unwrap_or_default()
}
fn dependencies(metadata: &str) -> Vec<String> {
    let Some(c) = regex::Regex::new(r"(?s)dependencies\s*=\s*\{([^}]*)\}")
        .unwrap()
        .captures(metadata)
    else {
        return vec![];
    };
    regex::Regex::new(r#""([^"\r\n]+)""#)
        .unwrap()
        .captures_iter(&c[1])
        .map(|c| c[1].to_string())
        .collect()
}
fn directory(a: &Value) -> PathBuf {
    let docs = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Shell Folders")
        .ok()
        .and_then(|k| k.get_value::<String, _>("Personal").ok())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join("Documents")
        });
    docs.join("Paradox Interactive")
        .join(str(a, "DocumentsFolder"))
}
fn list(root: &Path, g: &Value) -> Result<Vec<Value>> {
    let load = read(&root.join("dlc_load.json"), json!({}))?;
    let enabled = load["enabled_mods"].as_array().cloned().unwrap_or_default();
    let mut list = vec![];
    if let Ok(entries) = fs::read_dir(root.join("mod")) {
        for e in entries.flatten() {
            let path = e.path();
            no_links(&path)?;
            if path.extension().is_some_and(|s| s == "mod")
                && fs::metadata(&path).map_err(err)?.len() < 2 * 1024 * 1024
            {
                let metadata = fs::read_to_string(&path).map_err(err)?;
                let descriptor = format!("mod/{}", path.file_name().unwrap().to_string_lossy());
                let content = property(&metadata, "path");
                let content = if Path::new(&content).is_absolute() {
                    PathBuf::from(content)
                } else {
                    root.join(content)
                };
                list.push(json!({"name":property(&metadata,"name"),"descriptor":descriptor,"metadata":metadata,"enabled":enabled.iter().any(|v|v==&descriptor),"content":content,"dependencies":dependencies(&metadata)}));
            }
        }
    }
    let game_root = crate::mods::game_root(g).ok();
    let mut steamapps = game_root
        .as_ref()
        .and_then(|r| {
            r.ancestors()
                .find(|p| p.file_name().is_some_and(|n| n == "steamapps"))
        })
        .map(Path::to_path_buf)
        .into_iter()
        .collect::<Vec<_>>();
    if let Some(steam) = crate::discovery::steam() {
        steamapps.push(steam.join("steamapps"));
    }
    let mut seen: HashSet<String> = list
        .iter()
        .map(|v| str(v, "content").to_lowercase())
        .collect();
    for apps in steamapps {
        let folder = apps.join("workshop/content").join(text(g, "SteamAppId"));
        if let Ok(entries) = fs::read_dir(folder) {
            for e in entries.flatten() {
                let content = e.path();
                let file = content.join("descriptor.mod");
                if no_links(&file).is_ok()
                    && file.is_file()
                    && fs::metadata(&file).map_err(err)?.len() < 2 * 1024 * 1024
                    && seen.insert(content.to_string_lossy().to_lowercase())
                {
                    let metadata = fs::read_to_string(file).map_err(err)?;
                    list.push(json!({"name":property(&metadata,"name"),"descriptor":null,"metadata":metadata,"enabled":false,"content":content,"workshop":content.file_name().unwrap_or_default().to_string_lossy(),"dependencies":dependencies(&metadata)}));
                }
            }
        }
    }
    Ok(list)
}
pub fn handle(profile: &Path, g: &Value, a: &Value, p: &Value) -> Result<Value> {
    let root = directory(a);
    no_links(&root)?;
    let action = str(p, "action");
    let mut mods = list(&root, g)?;
    if action == "get" {
        return Ok(
            json!({"adapter":a,"kind":"paradox","root":root,"strategy":mods,"installed":{"Packages":{}}}),
        );
    }
    if action == "inspect" {
        let path = Path::new(str(p, "path"));
        let mut zip = zip::ZipArchive::new(fs::File::open(path).map_err(err)?).map_err(err)?;
        let mut names = vec![];
        let mut bytes = 0;
        let mut marker = 0;
        if zip.len() > 30000 {
            return Err("Too many ZIP entries".into());
        }
        for i in 0..zip.len() {
            let entry = zip.by_index(i).map_err(err)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().replace('\\', "/");
            safe(&profile.join("zip-review"), &name)?;
            if entry.unix_mode().is_some_and(|m| m & 0xf000 == 0xa000) {
                return Err("Linked ZIP entry".into());
            }
            bytes += entry.size();
            if bytes > 4 * 1024 * 1024 * 1024 {
                return Err("ZIP is too large".into());
            }
            if name.ends_with("descriptor.mod") {
                marker += 1;
            }
            names.push(name);
        }
        if marker != 1 {
            return Err("Expected one descriptor.mod".into());
        }
        return Ok(
            json!({"root":root,"files":names.iter().take(200).collect::<Vec<_>>(),"total":names.len(),"sha256":hash(path)?}),
        );
    }
    crate::saves::closed(g)?;
    let processes = sysinfo::System::new_all();
    if processes.processes().values().any(|p| {
        p.name()
            .to_string_lossy()
            .to_lowercase()
            .contains("paradox launcher")
            || p.name()
                .to_string_lossy()
                .eq_ignore_ascii_case("dowser.exe")
    }) {
        return Err("Close Paradox Launcher before changing the mod list".into());
    }
    let lock = safe(&root, ".playspace-mods/strategy.lock")?;
    fs::create_dir_all(lock.parent().unwrap()).map_err(err)?;
    let _guard = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .share_mode(0)
        .open(lock)
        .map_err(err)?;
    let file = root.join("dlc_load.json");
    let mut load = read(&file, json!({}))?;
    let mut enabled = load["enabled_mods"].as_array().cloned().unwrap_or_default();
    if action == "toggle" {
        let selected = mods
            .iter()
            .find(|m| str(m, "name") == str(p, "name"))
            .cloned()
            .ok_or("Mod not found")?;
        let name = str(&selected, "name");
        let include = p["enabled"] == true;
        if include {
            let content = Path::new(str(&selected, "content"));
            no_links(content)?;
            if !content.exists() {
                return Err("Mod content is missing".into());
            }
            for dependency in selected["dependencies"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if !mods.iter().any(|m| {
                    m["enabled"] == true && str(m, "name").eq_ignore_ascii_case(dependency)
                }) {
                    return Err(format!("Enable dependency first: {dependency}"));
                }
            }
        } else if mods.iter().any(|m| {
            m["enabled"] == true
                && str(m, "name") != name
                && m["dependencies"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|v| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case(name)))
        }) {
            return Err("Disable dependent mods first".into());
        }
        let mut descriptor = str(&selected, "descriptor").to_string();
        if descriptor.is_empty() && include {
            let workshop = str(&selected, "workshop");
            if workshop.is_empty() || !workshop.bytes().all(|b| b.is_ascii_digit()) {
                return Err("Invalid Workshop ID".into());
            }
            descriptor = format!("mod/playspace_ws_{workshop}.mod");
            let metadata = format!(
                "{}\npath=\"{}\"\n",
                str(&selected, "metadata")
                    .lines()
                    .filter(|l| !l.trim_start().starts_with("path=")
                        && !l.trim_start().starts_with("archive="))
                    .collect::<Vec<_>>()
                    .join("\n"),
                str(&selected, "content").replace('\\', "/")
            );
            let stage = profile.join(format!("paradox-registration/{}", id()));
            atomic(&stage, metadata.as_bytes())?;
            let prepared = BTreeMap::from([(descriptor.clone(), stage.clone())]);
            crate::mods::apply(
                &root,
                &[(
                    format!("Workshop-{workshop}"),
                    "local".into(),
                    vec![],
                    prepared,
                )],
            )?;
            let _ = fs::remove_file(stage);
        }
        enabled.retain(|v| v != &descriptor);
        if include {
            enabled.push(json!(descriptor));
        }
    } else if action == "move" {
        let descriptor = str(p, "descriptor");
        let index = enabled
            .iter()
            .position(|v| v == descriptor)
            .ok_or("Enabled mod not found")?;
        let next = (index as i64 + p["direction"].as_i64().unwrap_or(0).signum())
            .clamp(0, enabled.len().saturating_sub(1) as i64) as usize;
        enabled.swap(index, next);
        for (i, item) in enabled.iter().enumerate() {
            if let Some(m) = mods.iter().find(|m| m["descriptor"] == *item) {
                for dependency in m["dependencies"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    let position = enabled
                        .iter()
                        .position(|d| {
                            mods.iter().any(|m| {
                                m["descriptor"] == *d
                                    && str(m, "name").eq_ignore_ascii_case(dependency)
                            })
                        })
                        .ok_or("Missing enabled dependency")?;
                    if position > i {
                        return Err(format!("Dependency must load first: {dependency}"));
                    }
                }
            }
        }
    } else if action == "profile_save" {
        let name = str(p, "name").trim();
        if name.is_empty() || name.chars().count() > 36 {
            return Err("Invalid profile name".into());
        }
        let file = root.join(".playspace-mods/strategy-named-profiles.json");
        let mut profiles = read(&file, json!({}))?;
        profiles[name] = json!(enabled);
        write(&file, &profiles)?;
        return Ok(profiles);
    } else if action == "profile_list" {
        return read(
            &root.join(".playspace-mods/strategy-named-profiles.json"),
            json!({}),
        );
    } else if action == "profile_apply" {
        let profiles = read(
            &root.join(".playspace-mods/strategy-named-profiles.json"),
            json!({}),
        )?;
        enabled = profiles[str(p, "name")]
            .as_array()
            .cloned()
            .ok_or("Mod profile not found")?;
        for descriptor in &enabled {
            let m = mods
                .iter()
                .find(|m| m["descriptor"] == *descriptor)
                .ok_or("Profile mod is missing")?;
            if !Path::new(str(m, "content")).exists() {
                return Err("Profile mod content is missing".into());
            }
            for dependency in m["dependencies"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if !mods.iter().any(|m| {
                    str(m, "name").eq_ignore_ascii_case(dependency)
                        && enabled.iter().any(|d| d == &m["descriptor"])
                }) {
                    return Err("Profile dependency is missing".into());
                }
            }
        }
    } else if action == "import" {
        if p["confirmed"] != true {
            return Err("Confirm the local mod import".into());
        }
        let path = Path::new(str(p, "path"));
        let mut zip = zip::ZipArchive::new(fs::File::open(path).map_err(err)?).map_err(err)?;
        if zip.len() > 30000 {
            return Err("Too many ZIP entries".into());
        }
        let mut descriptor = None;
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(err)?;
            if f.name().replace('\\', "/").ends_with("descriptor.mod") {
                if descriptor.is_some() || f.size() > 2 * 1024 * 1024 {
                    return Err("Expected one descriptor.mod".into());
                }
                let name = f.name().replace('\\', "/");
                let mut text = String::new();
                f.read_to_string(&mut text).map_err(err)?;
                descriptor = Some((name, text));
            }
        }
        let (name, metadata) = descriptor.ok_or("Missing descriptor.mod")?;
        if property(&metadata, "name").is_empty() {
            return Err("Missing mod name".into());
        }
        let prefix = name.strip_suffix("descriptor.mod").unwrap();
        let token = id();
        let stage = profile.join(format!("paradox-stage/{token}"));
        fs::create_dir_all(&stage).map_err(err)?;
        let mut sources = BTreeMap::new();
        let mut total = 0;
        let mut seen = HashSet::new();
        let result = (|| {
            for i in 0..zip.len() {
                let mut entry = zip.by_index(i).map_err(err)?;
                if entry.is_dir() {
                    continue;
                }
                let name = entry.name().replace('\\', "/");
                safe(&stage, &name)?;
                if entry.unix_mode().is_some_and(|m| m & 0xf000 == 0xa000) {
                    return Err("Linked ZIP entry".into());
                }
                let Some(relative) = name.strip_prefix(prefix) else {
                    continue;
                };
                if relative.is_empty() {
                    continue;
                }
                if matches!(
                    Path::new(relative)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase()
                        .as_str(),
                    "exe" | "bat" | "cmd" | "ps1" | "vbs"
                ) {
                    return Err("Executable installers are not supported".into());
                }
                total += entry.size();
                if total > 4 * 1024 * 1024 * 1024 || entry.size() > 512 * 1024 * 1024 {
                    return Err("ZIP is too large".into());
                }
                let destination = format!("mod/playspace_{token}/{relative}");
                if !seen.insert(destination.to_lowercase()) {
                    return Err("Duplicate archive path".into());
                }
                let file = safe(&stage, &destination)?;
                fs::create_dir_all(file.parent().unwrap()).map_err(err)?;
                let mut output = fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&file)
                    .map_err(err)?;
                let written = std::io::copy(&mut entry, &mut output).map_err(err)?;
                if written != entry.size() {
                    return Err("Incomplete ZIP entry".into());
                }
                sources.insert(destination, file);
            }
            let registration = format!("mod/playspace_{token}.mod");
            let file = safe(&stage, &registration)?;
            let content = root
                .join(format!("mod/playspace_{token}"))
                .to_string_lossy()
                .replace('\\', "/");
            let metadata = format!(
                "{}\npath=\"{content}\"\n",
                metadata
                    .lines()
                    .filter(|l| !l.trim_start().starts_with("path")
                        && !l.trim_start().starts_with("archive"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            atomic(&file, metadata.as_bytes())?;
            sources.insert(registration, file);
            crate::mods::apply(
                &root,
                &[(format!("Local-{token}"), "local".into(), vec![], sources)],
            )
        })();
        if stage.exists() {
            no_links(&stage)?;
            fs::remove_dir_all(stage).map_err(err)?;
        }
        return result;
    } else {
        return Err("Unknown Paradox mod action".into());
    }
    let backup = root.join(".playspace-mods/dlc_load.original.json");
    if !backup.exists() {
        write(&backup, &load)?;
    }
    load["enabled_mods"] = json!(enabled);
    write(&file, &load)?;
    history(profile, g, "Mods", "Paradox mod load order updated");
    mods = list(&root, g)?;
    Ok(
        json!({"adapter":a,"kind":"paradox","root":root,"strategy":mods,"installed":{"Packages":{}}}),
    )
}
