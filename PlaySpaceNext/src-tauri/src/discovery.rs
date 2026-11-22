use crate::{common::*, library};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
use winreg::{enums::*, RegKey};
pub fn steam() -> Option<PathBuf> {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Valve\\Steam")
        .ok()
        .and_then(|k| k.get_value::<String, _>("SteamPath").ok())
        .map(PathBuf::from)
}
pub(crate) fn vdf(text: &str, key: &str) -> Option<String> {
    let re = regex::Regex::new(&format!(r#""{}"\s*"([^"]*)""#, regex::escape(key))).ok()?;
    re.captures(text).map(|c| c[1].replace("\\\\", "\\"))
}
fn exe(folder: &Path) -> Option<PathBuf> {
    let mut queue = vec![(folder.to_path_buf(), 0)];
    let mut paths = vec![];
    while let Some((dir, depth)) = queue.pop() {
        if depth > 4 {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for item in entries.flatten().take(500) {
            let path = item.path();
            if no_links(&path).is_err() {
                continue;
            }
            if path.is_dir() {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase();
                if ![
                    "redist",
                    "_commonredist",
                    "installer",
                    "support",
                    "engine",
                    "bepinex",
                    "dotnet",
                ]
                .contains(&name.as_str())
                {
                    queue.push((path, depth + 1));
                }
            } else if path
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("exe"))
            {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase();
                if ![
                    "unins",
                    "crash",
                    "setup",
                    "redist",
                    "unitycrash",
                    "easyanticheat",
                    "battleye",
                ]
                .iter()
                .any(|v| name.contains(v))
                {
                    let score = if name.contains("shipping") {
                        100
                    } else if path.parent() == Some(folder) {
                        80
                    } else {
                        20
                    };
                    paths.push((score, path));
                }
            }
        }
        if paths.len() > 200 {
            break;
        }
    }
    paths.sort_by_key(|(score, path)| {
        (
            std::cmp::Reverse(*score),
            std::cmp::Reverse(fs::metadata(path).map(|m| m.len()).unwrap_or(0)),
        )
    });
    paths.into_iter().next().map(|(_, p)| p)
}
pub fn scan(profile: &Path) -> Result<Value> {
    let mut found = vec![];
    if let Some(steam) = steam() {
        let mut roots = vec![steam.clone()];
        if let Ok(content) = fs::read_to_string(steam.join("steamapps/libraryfolders.vdf")) {
            for c in regex::Regex::new(r#""path"\s*"([^"]+)""#)
                .unwrap()
                .captures_iter(&content)
            {
                roots.push(PathBuf::from(c[1].replace("\\\\", "\\")));
            }
        }
        let mut seen = HashSet::new();
        for library in roots {
            if !seen.insert(library.clone()) {
                continue;
            }
            let Ok(entries) = fs::read_dir(library.join("steamapps")) else {
                continue;
            };
            for item in entries.flatten() {
                let path = item.path();
                if !path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("appmanifest_"))
                {
                    continue;
                }
                let Ok(content) = fs::read_to_string(&path) else {
                    continue;
                };
                let Some(id) = vdf(&content, "appid") else {
                    continue;
                };
                let Some(name) = vdf(&content, "name") else {
                    continue;
                };
                let Some(install) = vdf(&content, "installdir") else {
                    continue;
                };
                let Ok(root) = safe(&library.join("steamapps/common"), &install) else {
                    continue;
                };
                if !root.is_dir() {
                    continue;
                }
                let path = exe(&root)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| format!("steam://rungameid/{id}"));
                found.push(json!({"Name":name,"ExePath":path,"SteamAppId":id,"InstallDirectory":root,"LibraryId":crate::common::id()}));
            }
        }
    }
    let manifests = PathBuf::from(std::env::var_os("PROGRAMDATA").unwrap_or_default())
        .join("Epic/EpicGamesLauncher/Data/Manifests");
    if let Ok(entries) = fs::read_dir(manifests) {
        for item in entries.flatten() {
            if item.path().extension().is_some_and(|s| s == "item") {
                if let Ok(v) = read(&item.path(), Value::Null) {
                    if v["bIsIncompleteInstall"] == true {
                        continue;
                    }
                    let root = PathBuf::from(str(&v, "InstallLocation"));
                    if let Ok(path) = safe(&root, str(&v, "LaunchExecutable")) {
                        if path.is_file() {
                            found.push(json!({"Name":v["DisplayName"],"ExePath":path,"InstallDirectory":root,"SteamAppId":"","LibraryId":id(),"LaunchArguments":v["LaunchCommand"]}));
                        }
                    }
                }
            }
        }
    }
    let riot = PathBuf::from(std::env::var_os("PROGRAMDATA").unwrap_or_default())
        .join("Riot Games/RiotClientInstalls.json");
    if let Ok(v) = read(&riot, Value::Null) {
        if let Some(clients) = v["associated_client"].as_object() {
            for (root, client) in clients {
                if root
                    .replace('\\', "/")
                    .to_lowercase()
                    .trim_end_matches('/')
                    .ends_with("/valorant/live")
                {
                    let exe = client.as_str().unwrap_or("");
                    if Path::new(exe).is_file() && (Path::new(root).join("VALORANT.exe").is_file() || Path::new(root).join("ShooterGame/Binaries/Win64/VALORANT-Win64-Shipping.exe").is_file()) {
                        found.push(json!({"Name":"VALORANT","ExePath":exe,"InstallDirectory":root,"SteamAppId":"","LibraryId":id(),"LaunchArguments":"--launch-product=valorant --launch-patchline=live"}));
                    }
                }
            }
        }
    }
    let _guard = library::lock(profile)?;
    let (mut games, _) = library::load(&profile.join("games.json"))?;
    let mut count = 0;
    let mut repaired=0;
    for game in found {
        if let Some(existing) = games.iter_mut().find(|g| library::same(g, &game)) {
            let old=library::text(existing,"ExePath");
            let next=library::text(&game,"ExePath");
            if (!Path::new(&old).is_file() || (library::text(&game,"LaunchArguments").contains("--launch-product=valorant") && !library::text(existing,"LaunchArguments").contains("--launch-product=valorant"))) && Path::new(&next).is_file() {
                library::set(existing,"ExePath",json!(next));
                library::set(existing,"InstallDirectory",game["InstallDirectory"].clone());
                if library::text(&game,"LaunchArguments").contains("--launch-product=valorant") {
                    library::set(existing,"LaunchArguments",game["LaunchArguments"].clone());
                }
                repaired+=1;
            }
        } else {
            games.push(game);
            count += 1;
        }
    }
    library::save(profile, &games)?;
    drop(_guard);
    let removed=crate::installation::reconcile(profile)?;
    Ok(json!({"added":count,"repaired":repaired,"removed":removed}))
}
