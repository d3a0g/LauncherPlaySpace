use crate::{
    common::*,
    library::{self, set, text},
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
const MAX: u64 = 2 * 1024 * 1024 * 1024;
pub fn closed(g: &Value) -> Result<()> {
    let path = PathBuf::from(text(g, "ExePath"));
    let tracking = text(g, "TrackingExecutable");
    let system = sysinfo::System::new_all();
    let root = crate::mods::game_root(g)
        .ok()
        .map(|p| p.to_string_lossy().to_lowercase() + "\\");
    for process in system.processes().values() {
        if process.exe().is_some_and(|p| {
            p.to_string_lossy()
                .eq_ignore_ascii_case(&path.to_string_lossy())
                || root
                    .as_ref()
                    .is_some_and(|root| p.to_string_lossy().to_lowercase().starts_with(root))
        }) || (!tracking.is_empty()
            && process
                .name()
                .to_string_lossy()
                .eq_ignore_ascii_case(&tracking))
        {
            return Err("Close the game before changing its files".into());
        }
    }
    Ok(())
}
fn data_only(path: &Path) -> Result<()> {
    if matches!(
        path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase()
            .as_str(),
        "exe" | "dll" | "sys" | "msi" | "bat" | "cmd" | "ps1" | "vbs" | "lnk" | "scr"
    ) {
        return Err("Select a folder containing only saves or settings".into());
    }
    Ok(())
}
fn care(g: &Value) -> Value {
    if g["Care"].is_object() {
        g["Care"].clone()
    } else {
        json!({"Id":text(g,"LibraryId"),"SaveDirectory":"","Notes":"","BackupBeforeLaunch":false})
    }
}
fn source(profile: &Path, c: &Value) -> Result<PathBuf> {
    let path = PathBuf::from(str(c, "SaveDirectory"));
    if !path.is_absolute() || path.parent().is_none() || path.to_string_lossy().starts_with("\\\\")
    {
        return Err("Select the specific game's save folder".into());
    }
    specific_directory(&path)?;
    let path = normalize(&path);
    let profile = normalize(profile);
    if profile.starts_with(&path) || path.starts_with(&profile) {
        return Err("Save folder overlaps the launcher profile".into());
    }
    for key in [
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "PROGRAMDATA",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "WINDIR",
    ] {
        if let Some(v) = std::env::var_os(key) {
            if path == normalize(Path::new(&v)) {
                return Err("Select the specific game's save folder".into());
            }
        }
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        for folder in ["Documents", "Desktop", "Downloads"] {
            if path == normalize(&PathBuf::from(&home).join(folder)) {
                return Err("Select the specific game's save folder".into());
            }
        }
    }
    Ok(path)
}
fn normalize(path: &Path) -> PathBuf {
    PathBuf::from(
        path.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_lowercase(),
    )
}
fn root(profile: &Path, c: &Value) -> Result<PathBuf> {
    let id = str(c, "Id");
    uuid::Uuid::parse_str(id).map_err(err)?;
    let path = profile.join("game-backups").join(id);
    specific_directory(&path)?;
    Ok(path)
}
fn snapshots(root: &Path) -> Result<Vec<Value>> {
    let mut list = vec![];
    if root.exists() {
        for dir in fs::read_dir(root).map_err(err)? {
            let path = dir.map_err(err)?.path();
            if path.is_dir()
                && path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
            {
                if let Ok(v) = read(&path.join("snapshot.json"), Value::Null) {
                    if v.is_object() {
                        list.push(v)
                    }
                }
            }
        }
    }
    list.sort_by(|a, b| str(b, "Created").cmp(str(a, "Created")));
    Ok(list)
}
fn snapshot(root: &Path, c: &Value, id: &str) -> Result<Value> {
    uuid::Uuid::parse_str(id).map_err(err)?;
    let path = root.join(id).join("snapshot.json");
    specific_directory(&path)?;
    let v = read(&path, Value::Null)?;
    if v["Id"] != id || v["Owner"] != str(c, "Id") {
        return Err("Backup belongs to another game".into());
    }
    let entries = v["Files"].as_array().ok_or("Invalid backup manifest")?;
    if entries.len() > 10000 {
        return Err("Backup has too many files".into());
    }
    let mut seen = std::collections::HashSet::new();
    let mut total = 0;
    for entry in entries {
        let relative = str(entry, "Path");
        data_only(Path::new(relative))?;
        let file = safe(&root.join(id).join("files"), relative)?;
        if !seen.insert(relative.to_lowercase()) {
            return Err("Duplicate path in backup".into());
        }
        let size = entry["Size"].as_u64().ok_or("Invalid backup size")?;
        total += size;
        if total > MAX
            || str(entry, "Sha256").len() != 64
            || !str(entry, "Sha256").bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("Invalid backup manifest".into());
        }
        if fs::metadata(&file).map_err(err)?.len() != size
            || !hash(&file)?.eq_ignore_ascii_case(str(entry, "Sha256"))
        {
            return Err("Backup integrity check failed".into());
        }
    }
    Ok(v)
}
fn create(profile: &Path, g: &Value, c: &Value, kind: &str) -> Result<Value> {
    closed(g)?;
    let source = source(profile, c)?;
    let root = root(profile, c)?;
    if snapshots(&root)?.len() >= 50 {
        return Err("Backup limit reached: 50 per game".into());
    }
    let entries = files(&source, MAX)?;
    if entries.len() > 10000 {
        return Err("Too many save files".into());
    }
    for file in &entries {
        data_only(file)?;
    }
    let id = id();
    let stage = root.join(format!(".stage-{id}"));
    fs::create_dir_all(&stage).map_err(err)?;
    let outcome = (|| {
        let mut manifest = vec![];
        for file in entries {
            no_links(&file)?;
            let relative = file
                .strip_prefix(&source)
                .map_err(err)?
                .to_string_lossy()
                .replace('\\', "/");
            let target = safe(&stage.join("files"), &relative)?;
            fs::create_dir_all(target.parent().unwrap()).map_err(err)?;
            fs::copy(&file, &target).map_err(err)?;
            let size = fs::metadata(&target).map_err(err)?.len();
            manifest.push(json!({"Path":relative,"Size":size,"Sha256":hash(&target)?}));
        }
        let snapshot =
            json!({"Id":id,"Owner":c["Id"],"Created":now(),"Kind":kind,"Files":manifest});
        write(&stage.join("snapshot.json"), &snapshot)?;
        fs::rename(&stage, root.join(&id)).map_err(err)?;
        history(profile, g, "Backup", kind);
        Ok(snapshot)
    })();
    if stage.exists() {
        no_links(&stage)?;
        fs::remove_dir_all(&stage).map_err(err)?;
    }
    outcome
}
pub fn before_launch(profile: &Path, g: &Value) -> Result<()> {
    let c = care(g);
    if c["BackupBeforeLaunch"] == true {
        create(profile, g, &c, "Before launch")?;
    }
    Ok(())
}
pub fn handle(profile: &Path, p: &Value) -> Result<Value> {
    let mut g = game(profile, p)?;
    let mut c = care(&g);
    let action = str(p, "action");
    if action == "configure" {
        c["SaveDirectory"] = p["directory"].clone();
        c["Notes"] = json!(str(p, "notes").chars().take(4000).collect::<String>());
        c["BackupBeforeLaunch"] = json!(p["automatic"] == true);
        source(profile, &c)?;
        update_game(profile, str(p, "id"), |g| {
            set(g, "Care", c.clone());
            Ok(())
        })?;
        set(&mut g, "Care", c.clone());
    }
    if action == "create" {
        return create(profile, &g, &c, "Manual");
    }
    let root = root(profile, &c)?;
    if matches!(action, "verify" | "preview" | "restore") {
        let _guard = library::lock(profile)?;
        let source = source(profile, &c)?;
        let point = str(p, "snapshot");
        let snapshot = snapshot(&root, &c, point)?;
        if action == "verify" {
            return Ok(json!({"valid":true,"files":snapshot["Files"].as_array().unwrap().len()}));
        }
        let mut changes = vec![];
        for entry in snapshot["Files"].as_array().unwrap() {
            let target = safe(&source, str(entry, "Path"))?;
            if target.is_dir() {
                return Err("Restore target conflicts with a directory".into());
            }
            let action = if !target.exists() {
                "create"
            } else if hash(&target)?.eq_ignore_ascii_case(str(entry, "Sha256")) {
                "unchanged"
            } else {
                "replace"
            };
            changes.push(json!({"path":entry["Path"],"action":action}));
        }
        if action == "preview" {
            return Ok(json!(changes));
        }
        if p["confirmed"] != true {
            return Err("Review the restore plan first".into());
        }
        closed(&g)?;
        let safety = create(profile, &g, &c, "Before restore")?;
        let mut touched: Vec<(PathBuf, Option<Vec<u8>>, String)> = vec![];
        let result = (|| -> Result<()> {
            closed(&g)?;
            for change in changes.iter().filter(|e| e["action"] != "unchanged") {
                let relative = str(change, "path");
                let original = safe(&root.join(point).join("files"), relative)?;
                let target = safe(&source, relative)?;
                let previous = if target.exists() {
                    Some(fs::read(&target).map_err(err)?)
                } else {
                    None
                };
                let expected = hash(&original)?;
                atomic(&target, &fs::read(original).map_err(err)?)?;
                touched.push((target, previous, expected));
            }
            Ok(())
        })();
        if let Err(error) = result {
            for (target, previous, expected) in touched.iter().rev() {
                if hash(target).ok().is_some_and(|h| h == *expected) {
                    if let Some(bytes) = previous {
                        let _ = atomic(target, bytes);
                    } else {
                        let _ = fs::remove_file(target);
                    }
                }
            }
            return Err(format!("{error}; safety backup: {}", str(&safety, "Id")));
        }
        history(profile, &g, "Restore", "Save backup restored");
        return Ok(json!({"restored":touched.len(),"safety":safety["Id"]}));
    }
    Ok(json!({"options":c,"snapshots":snapshots(&root)?}))
}
