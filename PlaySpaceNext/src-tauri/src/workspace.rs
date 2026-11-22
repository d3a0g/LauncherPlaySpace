use crate::{
    common::*,
    library::{self, set, text},
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};
pub fn handle(profile: &Path, p: &Value) -> Result<Value> {
    let area = str(p, "area");
    let action = str(p, "action");
    if area == "settings" {
        let mut v = prefs(profile)?;
        if action == "save" {
            let _lock = library::lock(profile)?;
            v = prefs(profile)?;
            let changes = p["value"].as_object().ok_or("Invalid settings")?;
            for (k, x) in changes {
                match k.as_str() {
                    "Language" => {
                        if !matches!(x.as_str(), Some("ru" | "en")) {
                            return Err("Unsupported language".into());
                        }
                    }
                    "OfflineMode"
                    | "ReduceMotion"
                    | "HideWhilePlaying"
                    | "RestoreAfterPlaying"
                    | "NotificationsEnabled"
                    | "HomeBanner"
                    | "MemeQuotes" => {
                        if !x.is_boolean() {
                            return Err("Invalid preference".into());
                        }
                    }
                    "CloseBehavior" => {
                        if !x.as_u64().is_some_and(|n| n <= 2) {
                            return Err("Invalid close behavior".into());
                        }
                    }
                    "DownloadLimitBytesPerSecond" => {
                        if !x.as_u64().is_some_and(|n| n <= 10_000_000_000) {
                            return Err("Invalid speed limit".into());
                        }
                    }
                    "PosterWidth" => {
                        if !x.as_u64().is_some_and(|n| (135..=240).contains(&n)) {
                            return Err("Invalid cover size".into());
                        }
                    }
                    _ => continue,
                }
                v[k] = x.clone();
            }
            write(&profile.join("preferences.json"), &v)?;
            audit(profile, "Settings", "Preferences updated");
        }
        return Ok(v);
    }
    if area == "history" {
        let mut v = read(&profile.join("activity-history.json"), json!([]))?;
        if let Some(a) = v.as_array_mut() {
            a.reverse();
        }
        return Ok(v);
    }
    if area == "collections" {
        let file = profile.join("collections.json");
        let mut v = read(&file, json!({"Names":[],"Members":{}}))?;
        if action != "get" {
            let _lock = library::lock(profile)?;
            v = read(&file, json!({"Names":[],"Members":{}}))?;
            let name = str(p, "name").trim();
            if name.is_empty() || name.chars().count() > 36 || name.chars().any(char::is_control) {
                return Err("Collection name must contain 1–36 characters".into());
            }
            let names = v["Names"].as_array_mut().ok_or("Invalid collection file")?;
            match action {
                "create" => {
                    if names.len() >= 24
                        || names
                            .iter()
                            .any(|n| n.as_str().is_some_and(|s| s.eq_ignore_ascii_case(name)))
                    {
                        return Err("Collection already exists or limit reached".into());
                    }
                    names.push(json!(name));
                }
                "remove" => {
                    names.retain(|n| n != name);
                    if let Some(m) = v["Members"].as_object_mut() {
                        for list in m.values_mut() {
                            if let Some(a) = list.as_array_mut() {
                                a.retain(|n| n != name);
                            }
                        }
                    }
                }
                "rename" => {
                    let old = str(p, "original");
                    if names.iter().any(|n| n == name) {
                        return Err("Collection already exists".into());
                    }
                    let slot = names
                        .iter_mut()
                        .find(|n| n == &old)
                        .ok_or("Collection not found")?;
                    *slot = json!(name);
                    if let Some(m) = v["Members"].as_object_mut() {
                        for list in m.values_mut() {
                            if let Some(a) = list.as_array_mut() {
                                for n in a {
                                    if n == old {
                                        *n = json!(name);
                                    }
                                }
                            }
                        }
                    }
                }
                "set" => {
                    if !names.iter().any(|n| n == name) {
                        return Err("Collection not found".into());
                    }
                    let g = game(profile, p)?;
                    let id = text(&g, "LibraryId");
                    let m = v["Members"].as_object_mut().ok_or("Invalid collections")?;
                    let a = m
                        .entry(id)
                        .or_insert(json!([]))
                        .as_array_mut()
                        .ok_or("Invalid collection membership")?;
                    a.retain(|n| n != name);
                    if p["included"] == true {
                        a.push(json!(name));
                    }
                }
                _ => return Err("Unknown collection action".into()),
            }
            write(&file, &v)?;
        }
        return Ok(v);
    }
    if area == "duplicates" {
        let games = library::load(&profile.join("games.json"))?.0;
        let mut groups: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for g in &games {
            let key = library::identity(g);
            groups.entry(key).or_default().push(
                json!({"id":text(g,"LibraryId"),"name":text(g,"Name"),"path":text(g,"ExePath")}),
            );
        }
        if action == "merge" {
            let _lock = library::lock(profile)?;
            let games = library::load(&profile.join("games.json"))?.0;
            let keep = str(p, "id");
            let group = groups
                .values()
                .find(|a| a.len() > 1 && a.iter().any(|g| str(g, "id") == keep))
                .ok_or("Duplicate group not found")?;
            let ids: HashSet<String> = group.iter().map(|g| str(g, "id").to_string()).collect();
            let mut next = games.clone();
            let records: Vec<&Value> = games
                .iter()
                .filter(|g| ids.contains(&text(g, "LibraryId")))
                .collect();
            let target = next
                .iter_mut()
                .find(|g| text(g, "LibraryId") == keep)
                .ok_or("Game not found")?;
            if records.iter().any(|g| g["IsPinned"] == true) {
                set(target, "IsPinned", json!(true));
            }
            let seconds = records
                .iter()
                .filter_map(|g| g["PlayedSeconds"].as_f64())
                .fold(0., f64::max);
            set(target, "PlayedSeconds", json!(seconds));
            let mut collections = read(
                &profile.join("collections.json"),
                json!({"Names":[],"Members":{}}),
            )?;
            let members = collections["Members"]
                .as_object_mut()
                .ok_or("Invalid collections")?;
            let union: HashSet<ValueString> = ids
                .iter()
                .flat_map(|id| {
                    members
                        .get(id)
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                })
                .filter_map(Value::as_str)
                .map(|s| ValueString(s.into()))
                .collect();
            members.insert(
                keep.into(),
                json!(union.into_iter().map(|v| v.0).collect::<Vec<_>>()),
            );
            write(&profile.join("collections.json"), &collections)?;
            atomic(
                &profile.join(format!("library-before-merge-{}.json", id())),
                &serde_json::to_vec(&games).map_err(err)?,
            )?;
            next.retain(|g| !ids.contains(&text(g, "LibraryId")) || text(g, "LibraryId") == keep);
            library::save(profile, &next)?;
            return Ok(json!({"merged":ids.len()-1}));
        }
        return Ok(json!(groups
            .values()
            .filter(|a| a.len() > 1)
            .collect::<Vec<_>>()));
    }
    if area == "storage" {
        let folders = [
            "posters",
            "wide-posters",
            "hero-posters",
            "transfer-cache",
            "store-details",
            "mod-catalog",
        ];
        if action == "clear" {
            let name = str(p, "name");
            if !folders.contains(&name) {
                return Err("Only the image and download caches can be cleared".into());
            }
            let root = profile.join(name);
            let entries = files(&root, 20 * 1024 * 1024 * 1024)?;
            for f in entries {
                fs::remove_file(f).map_err(err)?;
            }
            audit(profile, "Storage", "Cache cleared");
        }
        let mut result = vec![];
        for name in folders {
            let root = profile.join(name);
            let entries = files(&root, 20 * 1024 * 1024 * 1024).unwrap_or_default();
            let bytes: u64 = entries
                .iter()
                .filter_map(|p| fs::metadata(p).ok())
                .map(|m| m.len())
                .sum();
            result.push(json!({"name":name,"files":entries.len(),"bytes":bytes}));
        }
        return Ok(json!(result));
    }
    if area == "backup-settings" {
        let names = ["preferences.json", "collections.json", "game-sounds.json"];
        if action == "export" {
            let path = PathBuf::from(str(p, "path"));
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                return Err("Choose a JSON file".into());
            }
            let mut value = json!({"format":"PlaySpaceNext-settings-v1","files":{}});
            for name in names {
                value["files"][name] = read(&profile.join(name), Value::Null)?;
            }
            write(&path, &value)?;
            return Ok(json!({"saved":true}));
        }
        if action == "import" {
            let path = PathBuf::from(str(p, "path"));
            let value = read(&path, Value::Null)?;
            if value["format"] != "PlaySpaceNext-settings-v1" {
                return Err("Unsupported settings backup".into());
            }
            let _lock = library::lock(profile)?;
            for name in names {
                if !value["files"][name].is_null() {
                    let old = profile.join(name);
                    if old.exists() {
                        fs::copy(&old, profile.join(format!("{name}.before-import-{}", id())))
                            .map_err(err)?;
                    }
                    write(&old, &value["files"][name])?;
                }
            }
            return Ok(json!({"imported":true}));
        }
        return Ok(json!({"profile":profile,"includes":names,"secretsIncluded":false}));
    }
    if area == "game" {
        let mut g = game(profile, p)?;
        if action == "save" {
            let v = &p["value"];
            update_game(profile, str(p, "id"), |g| {
                for k in [
                    "LaunchArguments",
                    "TrackingExecutable",
                    "CheckUbisoftBeforeLaunch",
                ] {
                    if let Some(x) = v.get(k) {
                        if x.is_string() && x.as_str().unwrap().len() > 4000 {
                            return Err("Launch settings too long".into());
                        }
                        set(g, k, x.clone());
                    }
                }
                Ok(())
            })?;
            g = game(profile, p)?;
        }
        if action == "artwork" || action == "reset_artwork" {
            let path = if action == "reset_artwork" {
                Value::Null
            } else {
                json!(crate::media::custom(profile, Path::new(str(p, "path")))?)
            };
            update_game(profile, str(p, "id"), |g| {
                set(g, "CustomArtworkPath", path);
                Ok(())
            })?;
            g = game(profile, p)?;
        }
        return Ok(g);
    }
    Err("Unknown workspace section".into())
}
#[derive(Eq, PartialEq, Hash)]
struct ValueString(String);
