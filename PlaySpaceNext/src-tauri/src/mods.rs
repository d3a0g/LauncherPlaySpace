use crate::{common::*, downloads, library::text};
use serde_json::{json, Value};
use std::os::windows::fs::OpenOptionsExt;
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Cursor, Read},
    path::{Path, PathBuf},
};
pub fn adapters() -> Value {
    serde_json::from_str(include_str!("../catalog/mod-adapters.json")).unwrap()
}
fn adapter(g: &Value, kind: &str, all: &Value) -> Option<Value> {
    all[kind]
        .as_array()?
        .iter()
        .find(|a| {
            str(a, "SteamId") == text(g, "SteamAppId")
                || (!str(a, "Executable").is_empty()
                    && Path::new(&text(g, "ExePath")).file_name().is_some_and(|n| {
                        n.to_string_lossy()
                            .eq_ignore_ascii_case(str(a, "Executable"))
                    }))
        })
        .cloned()
}
pub fn game_root(g: &Value) -> Result<PathBuf> {
    let folder = text(g, "InstallDirectory");
    let root = if !folder.is_empty() {
        PathBuf::from(folder)
    } else {
        PathBuf::from(text(g, "ExePath"))
            .parent()
            .ok_or("Game folder not found")?
            .to_path_buf()
    };
    if !root.is_absolute() || !root.is_dir() || root.parent().is_none() {
        return Err("Game folder not found".into());
    }
    specific_directory(&root)?;
    Ok(root)
}
fn documents() -> PathBuf {
    let key = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Shell Folders")
        .ok();
    key.and_then(|k| k.get_value::<String, _>("Personal").ok())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join("Documents")
        })
}
fn root(g: &Value, a: &Value, kind: &str) -> Result<PathBuf> {
    if kind == "thunderstore" {
        return game_root(g);
    }
    if kind == "paradox" {
        return Ok(documents()
            .join("Paradox Interactive")
            .join(str(a, "DocumentsFolder")));
    }
    let base = match str(a, "UserBase") {
        "appdata" => PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()),
        "local" => PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()),
        "documents" => documents(),
        _ => game_root(g)?,
    };
    if !str(a, "RequiredFile").is_empty() && !base.join(str(a, "RequiredFile")).is_file() {
        return Err(format!(
            "Required mod loader is missing: {}",
            str(a, "RequiredFile")
        ));
    }
    safe(&base, str(a, "Folder"))
}
fn state(root: &Path) -> Result<Value> {
    let value = read(
        &safe(root, ".playspace-mods/installed.json")?,
        json!({"Transaction":"","Packages":{},"Files":{}}),
    )?;
    if !value["Packages"].is_object() || !value["Files"].is_object() {
        return Err("Invalid installed mod state".into());
    }
    Ok(value)
}
fn recover(root: &Path) -> Result<()> {
    if safe(root, ".playspace-mods/pending.json")?.exists()
        || safe(root, ".playspace-mods/profile-switch.json")?.exists()
    {
        return Err("Finish the pending legacy mod transaction before continuing".into());
    }
    let path = safe(root, ".playspace-mods/native-pending.json")?;
    if !path.exists() {
        return Ok(());
    }
    let journal = read(&path, Value::Null)?;
    let current = state(root)?;
    if current["Transaction"] != journal["id"] {
        for item in journal["changes"]
            .as_array()
            .ok_or("Invalid mod journal")?
            .iter()
            .rev()
        {
            let target = safe(root, str(item, "path"))?;
            if item["remove"] == true {
                if !target.exists() {
                    let backup = safe(root, str(item, "backup"))?;
                    if hash(&backup)? != str(item, "hash") {
                        return Err("Mod rollback backup changed".into());
                    }
                    fs::create_dir_all(target.parent().unwrap()).map_err(err)?;
                    fs::copy(backup, target).map_err(err)?;
                }
            } else if target.exists() {
                if hash(&target)? != str(item, "hash") {
                    return Err("Mod rollback target changed; recovery stopped".into());
                }
                if !str(item, "backup").is_empty() {
                    let backup = safe(root, str(item, "backup"))?;
                    if hash(&backup)? != str(item, "beforeHash") {
                        return Err("Profile rollback backup changed".into());
                    }
                    atomic(&target, &fs::read(backup).map_err(err)?)?;
                } else {
                    fs::remove_file(target).map_err(err)?;
                }
            }
        }
    }
    fs::remove_file(path).map_err(err)?;
    Ok(())
}
fn lock(root: &Path) -> Result<File> {
    let path = safe(root, ".playspace-mods/manager.lock")?;
    fs::create_dir_all(path.parent().unwrap()).map_err(err)?;
    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .share_mode(0)
        .open(path)
        .map_err(|_| "Another mod operation is running".into())
}
fn catalog(profile: &Path, a: &Value, refresh: bool) -> Result<Value> {
    let community = str(a, "Community");
    if community.is_empty()
        || !community
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    {
        return Err("Invalid mod community".into());
    }
    let file = profile.join(format!("mod-catalog/{community}.json"));
    if !refresh || prefs(profile)?["OfflineMode"] == true {
        if file.exists() {
            if fs::metadata(&file).map_err(err)?.len() > 200 * 1024 * 1024 {
                return Err("Mod catalogue exceeds the limit".into());
            }
            return serde_json::from_slice(&fs::read(&file).map_err(err)?).map_err(err);
        }
    }
    if prefs(profile)?["OfflineMode"] == true {
        return Err("Catalogue is not cached offline".into());
    }
    let bytes = http_bytes(
        &format!("https://thunderstore.io/c/{community}/api/v1/package/"),
        200 * 1024 * 1024,
    )?;
    let value: Value = serde_json::from_slice(&bytes).map_err(err)?;
    if !value.as_array().is_some_and(|a| a.len() <= 100000) {
        return Err("Invalid mod catalogue".into());
    }
    atomic(&file, &bytes)?;
    Ok(value)
}
fn package_id(p: &Value) -> String {
    format!("{}-{}", str(p, "owner"), str(p, "name"))
}
fn resolve(
    selected: &str,
    version: &str,
    catalog: &Value,
    a: &Value,
    installed: &Value,
) -> Result<Vec<(Value, Value)>> {
    let mut plan = vec![];
    let mut chosen = BTreeMap::new();
    let mut visiting = HashSet::new();
    fn visit(
        id: &str,
        version: &str,
        cat: &Value,
        installed: &Value,
        plan: &mut Vec<(Value, Value)>,
        chosen: &mut BTreeMap<String, String>,
        visiting: &mut HashSet<String>,
    ) -> Result<()> {
        if let Some(old) = chosen.get(id) {
            return if old == version || version.is_empty() {
                Ok(())
            } else {
                Err(format!("Dependency version conflict: {id}"))
            };
        }
        if let Some(old) = installed["Packages"].get(id) {
            if version.is_empty() || str(old, "Version") == version {
                return Ok(());
            }
            return Err(format!(
                "Installed dependency has a different version: {id}"
            ));
        }
        if !visiting.insert(id.to_string()) {
            return Err(format!("Cyclic dependency: {id}"));
        }
        let package = cat
            .as_array()
            .ok_or("Invalid catalogue")?
            .iter()
            .find(|p| package_id(p) == id)
            .ok_or(format!("Missing dependency: {id}"))?;
        let v = package["versions"]
            .as_array()
            .ok_or("Missing mod versions")?
            .iter()
            .find(|v| {
                v["is_active"] != false
                    && (version.is_empty() || str(v, "version_number") == version)
            })
            .ok_or(format!("Missing package version: {id}-{version}"))?;
        for dependency in v["dependencies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let (dep, ver) = dependency.rsplit_once('-').ok_or("Invalid dependency ID")?;
            visit(dep, ver, cat, installed, plan, chosen, visiting)?;
        }
        visiting.remove(id);
        chosen.insert(id.to_string(), str(v, "version_number").into());
        plan.push((package.clone(), v.clone()));
        Ok(())
    }
    visit(
        selected,
        version,
        catalog,
        installed,
        &mut plan,
        &mut chosen,
        &mut visiting,
    )?;
    if !str(a, "Loader").is_empty()
        && !chosen.contains_key(str(a, "Loader"))
        && installed["Packages"].get(str(a, "Loader")).is_none()
    {
        let mut loader_plan = vec![];
        visit(
            str(a, "Loader"),
            "",
            catalog,
            installed,
            &mut loader_plan,
            &mut chosen,
            &mut visiting,
        )?;
        loader_plan.extend(plan);
        plan = loader_plan;
    }
    Ok(plan)
}
fn metadata(zip: &mut zip::ZipArchive<Cursor<Vec<u8>>>, marker: &str) -> Result<(String, Vec<u8>)> {
    let mut found = None;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(err)?;
        if !f.is_dir()
            && f.name()
                .replace('\\', "/")
                .to_lowercase()
                .ends_with(&marker.to_lowercase())
        {
            if found.is_some() || f.size() > 1024 * 1024 {
                return Err(format!("Expected one {marker}"));
            }
            let name = f.name().replace('\\', "/");
            let mut bytes = vec![];
            f.read_to_end(&mut bytes).map_err(err)?;
            found = Some((name, bytes));
        }
    }
    found.ok_or(format!("Missing {marker}"))
}
fn teardown_name(bytes: &[u8]) -> Result<String> {
    let text = std::str::from_utf8(bytes).map_err(err)?.trim_start_matches('\u{feff}');
    let names: Vec<&str> = text.lines().filter_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "name").then_some(value.trim())
    }).collect();
    if names.len() != 1 || names[0].is_empty() || names[0].chars().count() > 200 || names[0].chars().any(char::is_control) {
        return Err("Teardown info.txt must contain one non-empty name".into());
    }
    Ok(names[0].to_owned())
}
fn teardown_metadata(zip: &mut zip::ZipArchive<Cursor<Vec<u8>>>) -> Result<(String, String)> {
    let mut found = None;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(err)?;
        let name = entry.name().replace('\\', "/");
        if !entry.is_dir() && name.rsplit('/').next() == Some("info.txt") {
            if found.is_some() || entry.size() > 1024 * 1024 {
                return Err("Expected one Teardown info.txt; import each mod separately".into());
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(err)?;
            found = Some((name, teardown_name(&bytes)?));
        }
    }
    found.ok_or("Missing Teardown info.txt".into())
}
fn installed_view(root: &Path, a: &Value) -> Result<Value> {
    let mut installed = state(root)?;
    if str(a, "Layout") == "teardown" {
        if let Some(packages) = installed["Packages"].as_object_mut() {
            for package in packages.values_mut() {
                let info = package["Files"].as_array().into_iter().flatten().filter_map(Value::as_str)
                    .find(|name| name.ends_with("/info.txt")).map(String::from);
                if let Some(relative) = info {
                    let name = safe(root, &relative).ok().and_then(|path| {
                        if fs::metadata(&path).ok()?.len() > 1024 * 1024 { return None; }
                        teardown_name(&fs::read(path).ok()?).ok()
                    });
                    if let Some(name) = name { package["Name"] = json!(name); }
                }
            }
        }
    }
    Ok(installed)
}
fn map(
    name: &str,
    kind: &str,
    a: &Value,
    package: &str,
    prefix: &str,
    folder: &str,
) -> Result<Option<String>> {
    let mut path = name.to_string();
    let lower = path.to_lowercase();
    let leaf = path.rsplit('/').next().unwrap_or("");
    let ext = Path::new(leaf)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if kind == "thunderstore" {
        if !path.contains('/')
            && (matches!(lower.as_str(), "manifest.json" | "icon.png")
                || lower.ends_with(".md")
                || lower.starts_with("license"))
        {
            return Ok(None);
        }
        if a["IsUnreal"] == true {
            if matches!(ext.as_str(), "exe" | "bat" | "cmd" | "ps1" | "sh") {
                return Err("Executable installers are not supported".into());
            }
            let binary = format!("{}/Binaries/Win64/", str(a, "UnrealProject"));
            if package == "Thunderstore-unreal_shimloader" {
                if path.starts_with("UE4SS/Mods/") {
                    return Ok(Some(format!(".playspace-mods/unreal/mods/{}", &path[11..])));
                }
                if path.starts_with("UE4SS/") {
                    path = path[6..].into();
                    if path.eq_ignore_ascii_case("dwmapi.dll") {
                        return Ok(None);
                    }
                } else if !path.eq_ignore_ascii_case("dwmapi.dll") {
                    return Err("Unsupported shimloader file".into());
                }
                return Ok(Some(binary + &path));
            }
            let name = package
                .split_once('-')
                .map(|(_, name)| name)
                .unwrap_or(package);
            if let Some(suffix) = path.strip_prefix("overlay/") {
                return Ok(Some(format!(
                    ".playspace-mods/unreal/overlay/{package}/{suffix}"
                )));
            }
            if let Some(suffix) = path.strip_prefix("mod/") {
                return Ok(Some(format!(".playspace-mods/unreal/mods/{name}/{suffix}")));
            }
            if let Some(suffix) = path.strip_prefix("Mods/") {
                if !suffix.contains('/') {
                    return Err("A package cannot overwrite the shared UE4SS mods.txt".into());
                }
                return Ok(Some(format!(".playspace-mods/unreal/mods/{suffix}")));
            }
            for (source, target) in [("pak/", "paks"), ("cfg/", "cfg")] {
                if let Some(suffix) = path.strip_prefix(source) {
                    return Ok(Some(format!(".playspace-mods/unreal/{target}/{suffix}")));
                }
            }
            if !path.contains('/') && matches!(ext.as_str(), "pak" | "ucas" | "utoc") {
                return Ok(Some(format!(
                    "{}/Content/Paks/{leaf}",
                    str(a, "UnrealProject")
                )));
            }
            return Ok(Some(format!(".playspace-mods/unreal/mods/{name}/{path}")));
        }
        if package == str(a, "Loader") {
            if path
                .split('/')
                .next()
                .is_some_and(|p| p.starts_with("BepInExPack"))
            {
                path = path.split('/').skip(1).collect::<Vec<_>>().join("/");
            }
            if matches!(ext.as_str(), "sh" | "bat" | "md" | "so" | "dylib")
                || lower.starts_with("doorstop_libs/")
                || matches!(
                    lower.as_str(),
                    "readme.txt"
                        | "credits.txt"
                        | "license.txt"
                        | "changelog.txt"
                        | "steam_appid.txt"
                )
            {
                return Ok(None);
            }
            if !(path.starts_with("BepInEx/")
                || path.starts_with("dotnet/")
                || str(a, "Community") == "rounds" && path.starts_with("corlibs/")
                || matches!(
                    path.to_lowercase().as_str(),
                    "winhttp.dll" | "version.dll" | "doorstop_config.ini" | ".doorstop_version"
                ))
            {
                return Err(format!("Unsupported loader file: {path}"));
            }
            return Ok(Some(path));
        }
        if matches!(ext.as_str(), "exe" | "bat" | "cmd" | "ps1" | "sh") {
            return Err("Executable installers are not supported".into());
        }
        let path = path.strip_prefix("BepInEx/").unwrap_or(&path);
        let parts: Vec<&str> = path.split('/').collect();
        if let Some(i) = parts
            .iter()
            .position(|p| matches!(*p, "plugins" | "config" | "patchers" | "monomod"))
        {
            let suffix = parts[i + 1..].join("/");
            return Ok(Some(if parts[i] == "config" {
                format!("BepInEx/config/{suffix}")
            } else {
                format!("BepInEx/{}/{package}/{suffix}", parts[i])
            }));
        }
        return Ok(Some(format!("BepInEx/plugins/{package}/{path}")));
    }
    if matches!(ext.as_str(), "exe" | "bat" | "cmd" | "ps1" | "vbs" | "lnk") {
        return Err("Executable installers are not supported".into());
    }
    if !prefix.is_empty() {
        let Some(rest) = path.strip_prefix(prefix) else {
            return Ok(None);
        };
        path = rest.to_string();
    }
    let layout = str(a, "Layout");
    let leaf = path.rsplit('/').next().unwrap_or("");
    Ok(match layout {
        "archive" | "pak" | "vpk" | "pack" | "jar" | "tmod" => {
            let wanted = match layout {
                "archive" => "archive",
                "pak" => "pak",
                "vpk" => "vpk",
                "pack" => "pack",
                "jar" => "jar",
                _ => "tmod",
            };
            if ext == wanted {
                Some(leaf.into())
            } else {
                None
            }
        }
        "data" => {
            if ![
                "esp", "esm", "esl", "bsa", "ba2", "dds", "nif", "hkx", "pex", "psc", "json",
                "ini", "txt", "wav", "fuz", "lip", "xml",
            ]
            .contains(&ext.as_str())
            {
                return Err(format!("Unsupported Data file: {path}"));
            }
            Some(path.strip_prefix("Data/").unwrap_or(&path).to_string())
        }
        "teardown" => {
            if !["lua", "xml", "vox", "png", "jpg", "jpeg", "ogg", "wav", "txt", "json", "tde"].contains(&ext.as_str()) {
                return Err(format!("Unsupported Teardown mod file: {path}"));
            }
            Some(format!("{folder}/{path}"))
        }
        "witcher" => {
            let parts: Vec<&str> = path.split('/').collect();
            parts
                .iter()
                .position(|p| p.starts_with("mod"))
                .map(|i| parts[i..].join("/"))
                .filter(|p| p.contains("/content/"))
        }
        _ => Some(format!("{folder}/{path}")),
    })
}
fn unpack(
    bytes: Vec<u8>,
    kind: &str,
    a: &Value,
    package: &str,
    stage: &Path,
) -> Result<BTreeMap<String, PathBuf>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(err)?;
    if zip.is_empty() || zip.len() > 30000 {
        return Err("Archive is empty or too large".into());
    }
    let mut total = 0;
    let mut original_names = HashSet::new();
    for i in 0..zip.len() {
        let f = zip.by_index(i).map_err(err)?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().replace('\\', "/");
        safe(stage, &name)?;
        if !original_names.insert(name.to_lowercase()) {
            return Err("Duplicate ZIP entry".into());
        }
        if f.unix_mode().is_some_and(|m| m & 0xf000 == 0xa000) || f.size() > 512 * 1024 * 1024 {
            return Err("Invalid ZIP entry".into());
        }
        total += f.size();
        if total
            > if kind == "thunderstore" {
                256 * 1024 * 1024
            } else {
                4 * 1024 * 1024 * 1024
            }
        {
            return Err("Unpacked archive exceeds size limit".into());
        }
    }
    let mut prefix = String::new();
    let mut folder = format!("PlaySpace_{}", id());
    let layout = str(a, "Layout");
    if kind == "native" {
        if layout == "teardown" {
            let (name, _) = teardown_metadata(&mut zip)?;
            prefix = name.rfind('/').map(|i| name[..=i].to_string()).unwrap_or_default();
            folder = format!("playspace{}", id().replace('-', ""));
        }
        let marker = match layout {
            "smapi" => Some("manifest.json"),
            "rimworld" => Some("About/About.xml"),
            "xcom" => Some(".XComMod"),
            "gmod" => Some("addon.json"),
            "dontstarve" => Some("modinfo.lua"),
            "7days" => Some("ModInfo.xml"),
            "bannerlord" => Some("SubModule.xml"),
            "civ" => Some(".modinfo"),
            "darkest" => Some("project.xml"),
            _ => None,
        };
        if let Some(marker) = marker {
            let (name, bytes) = metadata(&mut zip, marker)?;
            prefix = name
                .rfind('/')
                .map(|i| name[..=i].to_string())
                .unwrap_or_default();
            if layout == "rimworld" {
                prefix = name[..name.len() - marker.len()].into();
            }
            if layout == "smapi" {
                let v: Value = serde_json::from_slice(&bytes).map_err(err)?;
                if str(&v, "UniqueID").is_empty() {
                    return Err("Missing SMAPI UniqueID".into());
                }
            }
            if layout == "gmod" {
                let v: Value = serde_json::from_slice(&bytes).map_err(err)?;
                if str(&v, "title").is_empty() {
                    return Err("Missing addon title".into());
                }
            }
            if layout == "bannerlord" {
                let xml = String::from_utf8(bytes).map_err(err)?;
                let re = regex::Regex::new(r#"<Id\s+value="([A-Za-z0-9_-]{1,100})""#).unwrap();
                folder = re.captures(&xml).ok_or("Missing Bannerlord module ID")?[1].into();
            }
        }
        if layout == "factorio" {
            let (name, bytes) = metadata(&mut zip, "info.json")?;
            let v: Value = serde_json::from_slice(&bytes).map_err(err)?;
            let id = str(&v, "name");
            let version = str(&v, "version");
            if id.is_empty()
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                || !regex::Regex::new(r"^\d+\.\d+\.\d+$")
                    .unwrap()
                    .is_match(version)
                || name != format!("{id}_{version}/info.json")
            {
                return Err("Invalid Factorio mod structure".into());
            }
            let relative = format!("{id}_{version}.zip");
            let staged = safe(stage, &relative)?;
            let bytes = zip.into_inner().into_inner();
            atomic(&staged, &bytes)?;
            return Ok(BTreeMap::from([(relative, staged)]));
        }
    }
    let mut mapped = BTreeMap::new();
    let mut case = HashSet::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(err)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        let Some(relative) = map(&name, kind, a, package, &prefix, &folder)? else {
            continue;
        };
        if !case.insert(relative.to_lowercase()) {
            return Err("Archive mapping creates conflicting paths".into());
        }
        let destination = safe(stage, &relative)?;
        fs::create_dir_all(destination.parent().unwrap()).map_err(err)?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&destination)
            .map_err(err)?;
        let copied = std::io::copy(&mut entry, &mut file).map_err(err)?;
        if copied != entry.size() {
            return Err("Incomplete ZIP entry".into());
        }
        file.sync_all().map_err(err)?;
        let ext = Path::new(&relative)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if matches!(layout, "vpk" | "pack" | "jar" | "tmod") {
            let mut header = [0u8; 12];
            let count = File::open(&destination)
                .map_err(err)?
                .read(&mut header)
                .map_err(err)?;
            let valid = match ext {
                "vpk" => {
                    regex::Regex::new(r"_\d{3}\.vpk$")
                        .unwrap()
                        .is_match(&relative)
                        || count >= 12 && header[..4] == [0x34, 0x12, 0xaa, 0x55]
                }
                "pack" => count >= 8 && &header[..3] == b"PFH",
                "jar" => count >= 4 && &header[..2] == b"PK",
                "tmod" => count >= 4 && &header[..4] == b"TMOD",
                _ => true,
            };
            if !valid {
                return Err("Invalid mod container header".into());
            }
        }
        mapped.insert(relative, destination);
    }
    if layout == "teardown" && !mapped.keys().any(|path| !path.ends_with("/info.txt") && !path.ends_with("/preview.jpg") && !path.ends_with("/readme.txt")) {
        return Err("Teardown mod has no content files".into());
    }
    if mapped.is_empty() {
        return Err("No supported files in the archive".into());
    }
    Ok(mapped)
}
pub(crate) fn apply(
    root: &Path,
    prepared: &[(String, String, Vec<String>, BTreeMap<String, PathBuf>)],
) -> Result<Value> {
    let _guard = lock(root)?;
    recover(root)?;
    let mut state = state(root)?;
    let mut writes: BTreeMap<String, (String, PathBuf, String)> = BTreeMap::new();
    let mut changes = vec![];
    for (package, version, dependencies, files) in prepared {
        if package.is_empty()
            || package.len() > 180
            || !package
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        {
            return Err("Invalid package identifier".into());
        }
        if state["Packages"].get(package).is_some() {
            return Err(format!("Already installed: {package}"));
        }
        for (relative, source) in files {
            let target = safe(root, relative)?;
            let digest = hash(source)?;
            let lower = relative.to_lowercase();
            if let Some((previous, _, old)) = writes.get(&lower) {
                if old != &digest || previous != relative {
                    return Err("Conflicting package files".into());
                }
            }
            if state["Files"]
                .as_object()
                .unwrap()
                .keys()
                .any(|key| key.eq_ignore_ascii_case(relative) && key != relative)
            {
                return Err("Case-insensitive mod path conflict".into());
            }
            if let Some(old) = state["Files"].get(relative) {
                if str(old, "Hash") != digest {
                    return Err(format!("Installed mod needs a different file: {relative}"));
                }
            }
            if target.exists() && hash(&target)? != digest {
                return Err(format!("Existing file will not be overwritten: {relative}"));
            }
            if state["Files"].get(relative).is_none() {
                state["Files"][relative] = json!({"Hash":digest,"Created":!target.exists()});
            }
            if !target.exists() && !writes.contains_key(&lower) {
                changes.push(json!({"path":relative,"hash":digest}));
            }
            writes.insert(lower, (relative.clone(), source.clone(), digest));
        }
        state["Packages"][package] = json!({"Id":package,"Version":version,"Dependencies":dependencies,"Files":files.keys().collect::<Vec<_>>()});
    }
    let token = id();
    let journal = safe(root, ".playspace-mods/native-pending.json")?;
    write(&journal, &json!({"id":token,"changes":changes}))?;
    let result = (|| {
        for (relative, source, digest) in writes.values() {
            let target = safe(root, relative)?;
            if target.exists() {
                continue;
            }
            fs::create_dir_all(target.parent().unwrap()).map_err(err)?;
            let mut input = File::open(source).map_err(err)?;
            let mut output = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&target)
                .map_err(err)?;
            std::io::copy(&mut input, &mut output).map_err(err)?;
            output.sync_all().map_err(err)?;
            drop(output);
            if hash(&target)? != *digest {
                return Err("Installed file hash mismatch".into());
            }
        }
        state["Transaction"] = json!(token);
        write(&safe(root, ".playspace-mods/installed.json")?, &state)?;
        Ok(json!({"installed":prepared.len(),"files":writes.len()}))
    })();
    if result.is_err() {
        recover(root)?;
    } else {
        fs::remove_file(journal).map_err(err)?;
    }
    result
}
pub(crate) fn uninstall(root: &Path, package: &str) -> Result<Value> {
    let _guard = lock(root)?;
    recover(root)?;
    let mut state = state(root)?;
    let selected = state["Packages"]
        .get(package)
        .cloned()
        .ok_or("Mod not installed")?;
    let dependency = format!("{package}-{}", str(&selected, "Version"));
    let frameworks: HashSet<String> = adapters()["thunderstore"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|a| str(a, "Loader").to_string())
        .chain(["Thunderstore-unreal_shimloader".into()])
        .collect();
    if frameworks.contains(package)
        && state["Packages"]
            .as_object()
            .unwrap()
            .keys()
            .any(|id| id != package && !frameworks.contains(id))
    {
        return Err("Remove other mods before removing their loader".into());
    }
    let others = state["Packages"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(id, _)| id.as_str() != package)
        .collect::<Vec<_>>();
    if others.iter().any(|(_, p)| {
        p["Dependencies"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|v| v == &dependency)
    }) {
        return Err("Remove dependent mods first".into());
    }
    let shared: HashSet<String> = others
        .iter()
        .flat_map(|(_, p)| p["Files"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .map(String::from)
        .collect();
    let token = id();
    let mut changes = vec![];
    for name in selected["Files"]
        .as_array()
        .ok_or("Invalid mod record")?
        .iter()
        .filter_map(Value::as_str)
        .filter(|n| !shared.contains(*n))
    {
        let file = safe(root, name)?;
        if state["Files"][name]["Created"] != true || !file.exists() {
            continue;
        }
        let current = hash(&file)?;
        if current != str(&state["Files"][name], "Hash") {
            if name.starts_with("BepInEx/config/")
                || name.starts_with(".playspace-mods/unreal/cfg/")
            {
                continue;
            }
            return Err(format!("Modified mod file was preserved: {name}"));
        }
        let relative = format!(".playspace-mods/undo/{token}-{}.bin", changes.len());
        let backup = safe(root, &relative)?;
        fs::create_dir_all(backup.parent().unwrap()).map_err(err)?;
        fs::copy(&file, &backup).map_err(err)?;
        changes.push(json!({"path":name,"hash":current,"remove":true,"backup":relative}));
    }
    let journal = safe(root, ".playspace-mods/native-pending.json")?;
    write(&journal, &json!({"id":token,"changes":changes}))?;
    let result = (|| {
        for change in &changes {
            fs::remove_file(safe(root, str(change, "path"))?).map_err(err)?;
        }
        state["Packages"].as_object_mut().unwrap().remove(package);
        for file in selected["Files"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
        {
            if !shared.contains(file) {
                state["Files"].as_object_mut().unwrap().remove(file);
            }
        }
        state["Transaction"] = json!(token);
        write(&safe(root, ".playspace-mods/installed.json")?, &state)?;
        Ok(json!({"removed":package}))
    })();
    if result.is_err() {
        recover(root)?;
    } else {
        fs::remove_file(journal).map_err(err)?;
    }
    result
}
fn profiles(root: &Path, g: &Value, p: &Value) -> Result<Value> {
    use sha2::{Digest, Sha256};
    let area = safe(root, ".playspace-mods/profiles")?;
    let fingerprint = format!(
        "{:X}",
        Sha256::digest(root.to_string_lossy().to_uppercase().as_bytes())
    );
    if str(p, "action") == "profile_list" {
        let mut list = vec![];
        if let Ok(dirs) = fs::read_dir(&area) {
            for d in dirs.flatten() {
                let path = d.path().join("profile.json");
                if let Ok(v) = read(&path, Value::Null) {
                    if v["RootHash"] == fingerprint {
                        list.push(json!({"id":v["Id"],"name":v["Name"],"packages":v["State"]["Packages"].as_object().map(|a|a.len()).unwrap_or(0)}));
                    }
                }
            }
        }
        return Ok(json!(list));
    }
    crate::saves::closed(g)?;
    let _guard = lock(root)?;
    recover(root)?;
    let before = state(root)?;
    if str(p, "action") == "profile_save" {
        let name = str(p, "name").trim();
        if name.is_empty() || name.chars().count() > 36 {
            return Err("Profile name must contain 1–36 characters".into());
        }
        let token = id();
        let point = area.join(&token);
        let mut bytes = 0;
        let result = (|| {
            for (relative, file) in before["Files"].as_object().unwrap() {
                let source = safe(root, relative)?;
                if hash(&source)? != str(file, "Hash") {
                    return Err("Mod files changed outside PlaySpace".into());
                }
                if file["Created"] != true {
                    continue;
                }
                bytes += fs::metadata(&source).map_err(err)?.len();
                if bytes > 512 * 1024 * 1024 {
                    return Err("Mod profile exceeds 512 MB".into());
                }
                let target = safe(&point.join("files"), relative)?;
                fs::create_dir_all(target.parent().unwrap()).map_err(err)?;
                fs::copy(source, &target).map_err(err)?;
                if hash(&target)? != str(file, "Hash") {
                    return Err("Profile copy changed".into());
                }
            }
            let v = json!({"Id":token,"Name":name,"RootHash":fingerprint,"State":before});
            write(&point.join("profile.json"), &v)?;
            Ok(json!({"saved":true,"id":token}))
        })();
        if result.is_err() && point.exists() {
            no_links(&point)?;
            fs::remove_dir_all(point).map_err(err)?;
        }
        return result;
    }
    let profile_id = str(p, "profile");
    uuid::Uuid::parse_str(profile_id).map_err(err)?;
    let point = area.join(profile_id);
    let selected = read(&point.join("profile.json"), Value::Null)?;
    if selected["RootHash"] != fingerprint {
        return Err("Mod profile belongs to another game folder".into());
    }
    let mut after = selected["State"].clone();
    let after_files = after["Files"].as_object().ok_or("Invalid mod profile")?;
    for package in after["Packages"]
        .as_object()
        .ok_or("Invalid mod profile")?
        .values()
    {
        for dependency in package["Dependencies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let (id, version) = dependency
                .rsplit_once('-')
                .ok_or("Invalid profile dependency")?;
            if str(&after["Packages"][id], "Version") != version {
                return Err("Missing mod profile dependency".into());
            }
        }
    }
    let mut paths: HashSet<String> = before["Files"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    paths.extend(after_files.keys().cloned());
    let token = id();
    let mut changes = vec![];
    for relative in paths {
        let old = &before["Files"][&relative];
        let new = &after_files.get(&relative).unwrap_or(&Value::Null);
        let target = safe(root, &relative)?;
        if !old.is_null() && target.exists() && hash(&target)? != str(old, "Hash") {
            return Err(format!("Modified mod file was preserved: {relative}"));
        }
        if old.is_null() && target.exists() && hash(&target)? != str(new, "Hash") {
            return Err(format!("Existing file will not be overwritten: {relative}"));
        }
        if new.is_null() {
            if old["Created"] != true || !target.exists() {
                continue;
            }
            let backup = format!(".playspace-mods/undo/{token}-{}.bin", changes.len());
            let file = safe(root, &backup)?;
            fs::create_dir_all(file.parent().unwrap()).map_err(err)?;
            fs::copy(&target, file).map_err(err)?;
            changes.push(json!({"path":relative,"remove":true,"hash":old["Hash"],"backup":backup}));
        } else if new["Created"] == true {
            let payload = safe(&point.join("files"), &relative)?;
            if hash(&payload)? != str(new, "Hash") {
                return Err("Saved mod profile failed integrity verification".into());
            }
            if target.exists() && hash(&target)? == str(new, "Hash") {
                continue;
            }
            let backup = if target.exists() {
                let relative = format!(".playspace-mods/undo/{token}-{}.bin", changes.len());
                let file = safe(root, &relative)?;
                fs::create_dir_all(file.parent().unwrap()).map_err(err)?;
                fs::copy(&target, file).map_err(err)?;
                Some(relative)
            } else {
                None
            };
            changes.push(json!({"path":relative,"hash":new["Hash"],"beforeHash":old["Hash"],"backup":backup,"payload":payload}));
        } else if !target.exists() || hash(&target)? != str(new, "Hash") {
            return Err("Shared original library is unavailable".into());
        }
    }
    if p["confirmed"] != true {
        return Ok(
            json!({"changes":changes.iter().map(|c|json!({"path":c["path"],"action":if c["remove"]==true{"remove"}else{"write"}})).collect::<Vec<_>>()}),
        );
    }
    let journal = safe(root, ".playspace-mods/native-pending.json")?;
    write(&journal, &json!({"id":token,"changes":changes}))?;
    let result = (|| {
        for change in &changes {
            let target = safe(root, str(change, "path"))?;
            if change["remove"] == true {
                fs::remove_file(target).map_err(err)?;
            } else {
                let payload = Path::new(str(change, "payload"));
                if hash(payload)? != str(change, "hash") {
                    return Err("Profile payload changed".into());
                }
                atomic(&target, &fs::read(payload).map_err(err)?)?;
            }
        }
        after["Transaction"] = json!(token);
        write(&safe(root, ".playspace-mods/installed.json")?, &after)?;
        Ok(json!({"activated":selected["Name"]}))
    })();
    if result.is_err() {
        recover(root)?;
    } else {
        fs::remove_file(journal).map_err(err)?;
    }
    result
}
pub fn handle(profile: &Path, p: &Value) -> Result<Value> {
    let all=crate::remote::catalog(profile,"modAdapters").unwrap_or_else(adapters);
    if str(p, "action") == "adapters" {
        return Ok(all);
    }
    let g = game(profile, p)?;
    let kind = if str(p, "kind").is_empty() {
        if adapter(&g, "thunderstore", &all).is_some() {
            "thunderstore"
        } else if adapter(&g, "paradox", &all).is_some() {
            "paradox"
        } else {
            "native"
        }
    } else {
        str(p, "kind")
    };
    let Some(a) = adapter(&g,kind,&all) else {
        if str(p,"action")=="get" {return Ok(json!({"unsupported":true,"available":all}))}
        return Err("No mod adapter for this game".into());
    };
    if kind == "paradox" {
        return crate::paradox::handle(profile, &g, &a, p);
    }
    let action = str(p, "action");
    if action == "workshop_catalog" {
        return crate::workshop::catalog(profile, str(&a, "WorkshopAppId"), p);
    }
    if action == "workshop_open" {
        if str(&a, "WorkshopAppId") != "1167630" { return Err("Workshop is not supported for this adapter".into()); }
        let item = str(p, "item");
        if item.is_empty() || item.len() > 20 || !item.bytes().all(|b| b.is_ascii_digit()) {
            return Err("Invalid Steam Workshop item".into());
        }
        return crate::os::open_url(&format!("steam://url/CommunityFilePage/{item}"));
    }
    let root = root(&g, &a, kind)?;
    if action.starts_with("profile_") {
        return profiles(&root, &g, p);
    }
    if action == "catalog" {
        let catalog = catalog(profile, &a, p["refresh"] == true)?;
        let query = str(p, "query").to_lowercase();
        let mut list: Vec<Value> = catalog
            .as_array()
            .unwrap()
            .iter()
            .filter(|x| {
                x["is_deprecated"] != true
                    && x["has_nsfw_content"] != true
                    && (query.is_empty() || package_id(x).to_lowercase().contains(&query))
            })
            .cloned()
            .collect();
        list.sort_by_key(|x| {
            std::cmp::Reverse(x["versions"][0]["downloads"].as_u64().unwrap_or(0))
        });
        let total = list.len();
        let start = p["offset"].as_u64().unwrap_or(0) as usize;
        return Ok(
            json!({"total":total,"items":list.into_iter().skip(start).take(100).map(|x|json!({"id":package_id(&x),"name":x["name"],"owner":x["owner"],"version":x["versions"][0]["version_number"],"description":x["versions"][0]["description"]})).collect::<Vec<_>>()}),
        );
    }
    if action == "plan" {
        let catalog = catalog(profile, &a, false)?;
        let plan = resolve(
            str(p, "package"),
            str(p, "version"),
            &catalog,
            &a,
            &state(&root)?,
        )?;
        return Ok(json!(plan.iter().map(|(package,version)|json!({"id":package_id(package),"version":version["version_number"],"bytes":version["file_size"],"dependencies":version["dependencies"],"loader":package_id(package)==str(&a,"Loader")})).collect::<Vec<_>>()));
    }
    if action == "uninstall" {
        crate::saves::closed(&g)?;
        let result = uninstall(&root, str(p, "package"))?;
        history(profile, &g, "Mods", "Managed mod removed");
        return Ok(result);
    }
    if matches!(action, "install" | "import" | "inspect") {
        if action != "inspect" && p["confirmed"] != true {
            return Err("Review the installation plan first".into());
        }
        crate::saves::closed(&g)?;
        let job = downloads::begin("Mod installation");
        let stage = profile.join(format!("mod-staging/{}", id()));
        fs::create_dir_all(&stage).map_err(err)?;
        let result = (|| {
            let mut prepared = vec![];
            if action == "import" || action == "inspect" {
                let path = Path::new(str(p, "path"));
                if fs::metadata(path).map_err(err)?.len() > 1024 * 1024 * 1024 {
                    return Err("ZIP exceeds 1 GB".into());
                }
                let package = format!("Local-{}", id());
                let files = unpack(fs::read(path).map_err(err)?, kind, &a, &package, &stage)?;
                if action == "inspect" {
                    return Ok(
                        json!({"root":root,"files":files.keys().take(200).collect::<Vec<_>>(),"total":files.len(),"sha256":hash(path)?}),
                    );
                }
                prepared.push((package, "local".into(), vec![], files));
            } else {
                let catalog = catalog(profile, &a, false)?;
                let plan = resolve(
                    str(p, "package"),
                    str(p, "version"),
                    &catalog,
                    &a,
                    &state(&root)?,
                )?;
                if plan
                    .iter()
                    .any(|(package, _)| package_id(package) == str(&a, "Loader"))
                    && p["allowLoader"] != true
                {
                    return Err("Confirm installation of the mod loader binaries".into());
                }
                for (package, version) in plan {
                    if downloads::cancelled(&job) {
                        return Err("Cancelled".into());
                    }
                    let id = package_id(&package);
                    if !id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                        || id.len() > 180
                    {
                        return Err("Invalid package ID".into());
                    }
                    let bytes = downloads::fetch(
                        profile,
                        &job,
                        str(&version, "download_url"),
                        100 * 1024 * 1024,
                    )?;
                    let files = unpack(bytes, kind, &a, &id, &stage.join(&id))?;
                    prepared.push((
                        id,
                        str(&version, "version_number").into(),
                        version["dependencies"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                            .map(String::from)
                            .collect(),
                        files,
                    ));
                }
            }
            if kind == "thunderstore" {
                let mut total = 0u64;
                for (_, _, _, files) in &prepared {
                    for file in files.values() {
                        total += fs::metadata(file).map_err(err)?.len();
                        if total > 256 * 1024 * 1024 {
                            return Err("Combined mod installation exceeds 256 MB".into());
                        }
                    }
                }
            }
            crate::saves::closed(&g)?;
            if downloads::cancelled(&job) {
                return Err("Cancelled".into());
            }
            downloads::progress(&job, 0, None, "Installing verified files");
            let result = apply(&root, &prepared)?;
            history(profile, &g, "Mods", "Managed mods installed");
            Ok(result)
        })();
        downloads::finish(&job, &result);
        if stage.exists() {
            no_links(&stage)?;
            fs::remove_dir_all(stage).map_err(err)?;
        }
        return result;
    }
    Ok(
        json!({"adapter":a,"kind":kind,"root":root,"installed":installed_view(&root, &a)?,"available":all}),
    )
}

#[cfg(test)]
mod dependency_tests {
    use super::*;
    use std::io::Write;
    fn teardown_zip(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, contents) in entries {
            writer.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(contents.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }
    fn test_root() -> PathBuf {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/native-tests").join(id());
        fs::create_dir_all(&path).unwrap();
        path
    }
    #[test]
    fn teardown_wrapped_map_installs_and_removes_only_managed_files() {
        let area = test_root();
        let stage = area.join("stage");
        let root = area.join("mods");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("existing.txt"), "untouched").unwrap();
        let adapter = json!({"Layout":"teardown"});
        let files = unpack(teardown_zip(&[
            ("Map/info.txt", "name = Test map\nauthor = Fixture\ntags = Map"),
            ("Map/main.xml", "<scene/>"),
            ("Map/scripts/main.lua", "function init() end"),
            ("Map/vox/test.vox", "fixture"),
            ("README.md", "outside wrapper"),
        ]), "native", &adapter, "Local-Test", &stage).unwrap();
        assert_eq!(files.len(), 4);
        let folder = files.keys().next().unwrap().split('/').next().unwrap().to_string();
        assert!(folder.bytes().all(|b| b.is_ascii_alphanumeric()));
        assert!(files.contains_key(&format!("{folder}/main.xml")));
        apply(&root, &[("Local-Test".into(), "local".into(), vec![], files)]).unwrap();
        assert_eq!(installed_view(&root, &adapter).unwrap()["Packages"]["Local-Test"]["Name"], "Test map");
        fs::write(root.join(&folder).join("main.xml"), "user edit").unwrap();
        assert!(uninstall(&root, "Local-Test").is_err());
        fs::write(root.join(&folder).join("main.xml"), "<scene/>").unwrap();
        uninstall(&root, "Local-Test").unwrap();
        assert!(!root.join(&folder).join("info.txt").exists());
        assert_eq!(fs::read_to_string(root.join("existing.txt")).unwrap(), "untouched");
    }
    #[test]
    fn teardown_rejects_bad_archives_before_installation() {
        let adapter = json!({"Layout":"teardown"});
        let cases: Vec<Vec<(&str, &str)>> = vec![
            vec![("badinfo.txt", "name = Fake"), ("main.xml", "<scene/>")],
            vec![("a/info.txt", "name = A"), ("b/info.txt", "name = B")],
            vec![("info.txt", "name = "), ("main.lua", "return")],
            vec![("info.txt", "name = A\nname = B"), ("main.lua", "return")],
            vec![("info.txt", "name = A"), ("plugin.dll", "bad")],
            vec![("info.txt", "name = A"), ("setup.exe", "bad")],
            vec![("info.txt", "name = A"), ("../escape.lua", "bad")],
            vec![("info.txt", "name = Empty")],
        ];
        for entries in cases {
            assert!(unpack(teardown_zip(&entries), "native", &adapter, "Local-Test", &test_root().join("stage")).is_err(), "accepted {entries:?}");
        }
    }
    #[test]
    fn teardown_accepts_global_tools_and_asset_overrides() {
        for payload in ["main.lua", "data/sounds/test.ogg"] {
            let files = unpack(teardown_zip(&[("info.txt", "\u{feff}name = Fixture\ntags = Tool"), (payload, "fixture")]), "native", &json!({"Layout":"teardown"}), "Local-Test", &test_root().join("stage")).unwrap();
            assert_eq!(files.len(), 2);
        }
        let game = json!({"SteamAppId":"1167630"});
        assert_eq!(adapter(&game,"native",&adapters()).unwrap()["Layout"], "teardown");
        assert_eq!(adapter(&json!({"ExePath":"C:\\Games\\Teardown\\teardown.exe"}),"native",&adapters()).unwrap()["Name"], "Teardown");
    }
    #[test]
    fn exact_loader_dependency_wins_over_implicit_latest() {
        let catalogue = json!([
            {"owner":"BepInEx","name":"Pack","versions":[{"version_number":"2.0.0"},{"version_number":"1.0.0"}]},
            {"owner":"Author","name":"Example","versions":[{"version_number":"1.0.0","dependencies":["BepInEx-Pack-1.0.0"]}]}
        ]);
        let plan = resolve(
            "Author-Example",
            "1.0.0",
            &catalogue,
            &json!({"Loader":"BepInEx-Pack"}),
            &json!({"Packages":{}}),
        )
        .unwrap();
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].1["version_number"], "1.0.0");
        assert_eq!(package_id(&plan[0].0), "BepInEx-Pack");
    }
    #[test]
    fn cycles_are_rejected_before_any_download() {
        let catalogue = json!([
            {"owner":"A","name":"One","versions":[{"version_number":"1","dependencies":["B-Two-1"]}]},
            {"owner":"B","name":"Two","versions":[{"version_number":"1","dependencies":["A-One-1"]}]}
        ]);
        assert!(resolve(
            "A-One",
            "1",
            &catalogue,
            &json!({}),
            &json!({"Packages":{}})
        )
        .is_err());
    }
    #[test]
    fn unreal_routes_match_the_existing_shim_contract() {
        let adapter = json!({"IsUnreal":true,"UnrealProject":"Pal"});
        assert_eq!(
            map(
                "mod/Scripts/main.lua",
                "thunderstore",
                &adapter,
                "Author-Example",
                "",
                ""
            )
            .unwrap(),
            Some(".playspace-mods/unreal/mods/Example/Scripts/main.lua".into())
        );
        assert_eq!(
            map(
                "pak/content.pak",
                "thunderstore",
                &adapter,
                "Author-Example",
                "",
                ""
            )
            .unwrap(),
            Some(".playspace-mods/unreal/paks/content.pak".into())
        );
        assert!(map(
            "Mods/mods.txt",
            "thunderstore",
            &adapter,
            "Author-Example",
            "",
            ""
        )
        .is_err());
    }
}
