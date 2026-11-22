use serde_json::{json, Value};
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 32 * 1024 * 1024;
pub fn handles(operation: &str) -> bool {
    matches!(
        operation,
        "bootstrap" | "import_legacy" | "save_game" | "remove_game" | "pin"
    )
}
pub(crate) fn field<'a>(game: &'a Value, key: &str) -> Option<&'a Value> {
    game.as_object()?
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}
pub(crate) fn text(game: &Value, key: &str) -> String {
    field(game, key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}
pub(crate) fn set(game: &mut Value, key: &str, value: Value) {
    if let Some(object) = game.as_object_mut() {
        let existing = object.keys().find(|k| k.eq_ignore_ascii_case(key)).cloned();
        object.insert(existing.unwrap_or_else(|| key.to_string()), value);
    }
}
fn parse(path: &Path) -> Result<Vec<Value>, String> {
    let size = fs::metadata(path)
        .map_err(|_| "Не удалось прочитать файл библиотеки")?
        .len();
    if size > MAX_BYTES {
        return Err("Файл библиотеки слишком большой".into());
    }
    let bytes = fs::read(path).map_err(|_| "Не удалось прочитать файл библиотеки")?;
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    let games: Vec<Value> =
        serde_json::from_slice(bytes).map_err(|_| "Повреждён формат библиотеки")?;
    if games.iter().any(|g| {
        !g.is_object() || text(g, "Name").trim().is_empty() || text(g, "ExePath").trim().is_empty()
    }) {
        return Err("В библиотеке есть некорректные записи".into());
    }
    Ok(games)
}
pub(crate) fn load(path: &Path) -> Result<(Vec<Value>, Option<String>), String> {
    if !path.exists() && !path.with_extension("json.bak").exists() {
        return Ok((vec![], None));
    }
    match parse(path) {
        Ok(games) => Ok((games, None)),
        Err(_) => parse(&path.with_extension("json.bak"))
            .map(|games| {
                (
                    games,
                    Some("Библиотека прочитана из резервной копии. Исходный файл сохранён.".into()),
                )
            })
            .map_err(|_| {
                "Не удалось прочитать библиотеку и резервную копию. Исходные файлы сохранены."
                    .into()
            }),
    }
}
pub(crate) fn lock(profile: &Path) -> Result<File, String> {
    fs::create_dir_all(profile).map_err(|_| "Не удалось создать папку профиля")?;
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(windows)]
    options.share_mode(0);
    options.open(profile.join("next-runtime.lock")).map_err(|_|"Сейчас работает игровой сеанс или открыто окно инструмента. Повторите изменение библиотеки после его завершения.".into())
}
pub(crate) fn save(profile: &Path, games: &[Value]) -> Result<(), String> {
    let path = profile.join("games.json");
    let token = uuid::Uuid::new_v4().simple().to_string();
    let temporary = profile.join(format!(".games-{token}.tmp"));
    let result = (|| {
        let bytes =
            serde_json::to_vec_pretty(games).map_err(|_| "Не удалось сохранить библиотеку")?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|_| "Не удалось создать временный файл библиотеки")?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Не удалось записать библиотеку")?;
        drop(file);
        if path.exists() {
            let backup = if parse(&path).is_ok() {
                profile.join("games.json.bak")
            } else {
                profile.join(format!("games-corrupt-{token}.json"))
            };
            let backup_temp = profile.join(format!(".games-backup-{token}.tmp"));
            fs::copy(&path, &backup_temp)
                .map_err(|_| "Не удалось сохранить прежний файл библиотеки")?;
            fs::rename(&backup_temp, backup).map_err(|_| "Не удалось заменить резервную копию")?;
        }
        fs::rename(&temporary, &path).map_err(|_| "Не удалось заменить файл библиотеки")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
fn ensure_ids(games: &mut [Value]) {
    for game in games {
        let id = uuid::Uuid::parse_str(&text(game, "LibraryId"))
            .unwrap_or_else(|_| uuid::Uuid::new_v4());
        set(game, "LibraryId", json!(id.simple().to_string()));
    }
}
fn view(game: &Value) -> Value {
    let steam = text(game, "SteamAppId");
    let path = text(game, "ExePath");
    let source = if !steam.is_empty() && steam.bytes().all(|b| b.is_ascii_digit()) {
        "Steam"
    } else if path.to_lowercase().contains("riot") {
        "Riot Games"
    } else {
        "Локальная игра"
    };
    let minutes = field(game, "SteamPlaytimeMinutes").and_then(Value::as_f64);
    let seconds = minutes.map(|m| m * 60.).unwrap_or_else(|| {
        field(game, "PlayedSeconds")
            .and_then(Value::as_f64)
            .unwrap_or(0.)
    });
    let playtime = if minutes.is_some() || source != "Steam" {
        format!("{:.1} ч", seconds / 3600.).replace('.', ",")
    } else {
        "время не загружено".into()
    };
    json!({"id":text(game,"LibraryId"),"name":text(game,"Name"),"exePath":path,"steamAppId":steam,"isPinned":field(game,"IsPinned").and_then(Value::as_bool).unwrap_or(false),"playtime":playtime,"playedSeconds":seconds,"lastLaunchUtc":field(game,"LastLaunchUtc"),"source":source,"achievementSummary":field(game,"AchievementSummary"),"playStatus":text(game,"PlayStatus"),"hidden":field(game,"Hidden")==Some(&Value::Bool(true)),"customArtwork":field(game,"CustomArtworkPath")})
}
pub(crate) fn identity(game: &Value) -> String {
    let normalize = |s: String| s.replace('\\', "/").trim_start_matches("//?/").trim_end_matches('/').to_lowercase();
    let steam = text(game, "SteamAppId");
    if !steam.is_empty() { return format!("steam:{steam}"); }
    for value in [text(game,"InstallDirectory"), text(game,"ExePath")] {
        let value = normalize(value);
        if let Some(at) = value.find("/valorant/live") {
            let end = at + "/valorant/live".len();
            if value.len() == end || value.as_bytes().get(end) == Some(&b'/') {
                return format!("riot:{}", &value[..end]);
            }
        }
    }
    format!("exe:{}:{}",normalize(text(game,"ExePath")),text(game,"LaunchArguments").trim())
}
pub(crate) fn same(a: &Value, b: &Value) -> bool {
    identity(a) == identity(b)
}
pub fn handle(
    profile: &Path,
    legacy: &Path,
    operation: &str,
    payload: &Value,
) -> Result<Value, String> {
    if operation == "bootstrap" {
        let (games, notice) = load(&profile.join("games.json"))?;
        let tools: Value = serde_json::from_str(include_str!("../../src/features.json"))
            .map_err(|_| "Некорректный реестр инструментов")?;
        return Ok(
            json!({"games":games.iter().map(view).collect::<Vec<_>>(),"tools":crate::remote::tools(profile,tools),"legacyAvailable":legacy.exists()||legacy.with_extension("json.bak").exists(),"profileFolder":profile,"notice":notice}),
        );
    }
    let _guard = lock(profile)?;
    let (mut games, _) = load(&profile.join("games.json"))?;
    ensure_ids(&mut games);
    let response = match operation {
        "import_legacy" => {
            let (mut imported, _) = load(legacy)?;
            ensure_ids(&mut imported);
            let mut count = 0;
            for game in imported {
                if !games.iter().any(|existing| same(existing, &game)) {
                    games.push(game);
                    count += 1;
                }
            }
            json!({"imported":count})
        }
        "pin" => {
            let id = payload["id"].as_str().ok_or("Не выбрана игра")?;
            let game = games
                .iter_mut()
                .find(|g| text(g, "LibraryId") == id)
                .ok_or("Игра не найдена")?;
            let pinned = !field(game, "IsPinned")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            set(game, "IsPinned", json!(pinned));
            view(game)
        }
        "remove_game" => {
            let id = payload["id"].as_str().ok_or("Не выбрана игра")?;
            let before = games.len();
            let removed:Vec<_> = games.iter().filter(|g| text(g,"LibraryId") == id).cloned().collect();
            if !removed.is_empty() {
                crate::common::write(&profile.join(format!("removed-games/{}.json",uuid::Uuid::new_v4())), &json!(removed))?;
            }
            games.retain(|g| text(g, "LibraryId") != id);
            json!({"removed":games.len()!=before})
        }
        "save_game" => {
            let path = PathBuf::from(payload["path"].as_str().ok_or("Не выбран EXE")?);
            if !path.is_file()
                || !path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
            {
                return Err("Выберите существующий EXE игры.".into());
            }
            let path = fs::canonicalize(path).map_err(|_| "Не удалось прочитать путь игры")?;
            #[cfg(windows)]
            let path = {
                let value = path.to_string_lossy();
                if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
                    PathBuf::from(format!(r"\\{rest}"))
                } else if let Some(rest) = value.strip_prefix(r"\\?\") {
                    PathBuf::from(rest)
                } else {
                    path
                }
            };
            let game = json!({"Name":path.file_stem().unwrap_or_default().to_string_lossy(),"ExePath":path,"InstallDirectory":path.parent(),"LibraryId":uuid::Uuid::new_v4().simple().to_string(),"SteamAppId":"","IsPinned":false,"PlayedSeconds":0.0});
            let existing = games.iter().find(|g| same(g, &game));
            let output = existing.map(view).unwrap_or_else(|| view(&game));
            if existing.is_none() {
                games.push(game);
            }
            output
        }
        _ => return Err("Неизвестная операция библиотеки".into()),
    };
    save(profile, &games)?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn valorant_identity_is_installation_based_and_does_not_merge_other_riot_games() {
        let client=json!({"Name":"VALORANT","ExePath":"C:/Riot Games/Riot Client/RiotClientServices.exe","InstallDirectory":"D:/Riot Games/VALORANT/live","LaunchArguments":"--launch-product=valorant --launch-patchline=live"});
        let direct=json!({"Name":"VALORANT","ExePath":"d:\\Riot Games\\VALORANT\\live\\ShooterGame\\Binaries\\Win64\\VALORANT-Win64-Shipping.exe"});
        assert!(same(&client,&direct));
        let other=json!({"Name":"League of Legends","ExePath":"C:/Riot Games/Riot Client/RiotClientServices.exe","LaunchArguments":"--launch-product=league_of_legends --launch-patchline=live"});
        assert!(!same(&client,&other));
        assert!(!same(&client,&json!({"ExePath":"E:/Riot Games/VALORANT/live/VALORANT.exe"})));
    }
    fn area() -> PathBuf {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/library-tests")
            .join(uuid::Uuid::new_v4().simple().to_string());
        fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn importing_twice_preserves_original_and_extension_fields() {
        let root = area();
        let original = root.join("legacy.json");
        let profile = root.join("next");
        let record = json!([{"Name":"Portal 2","ExePath":"C:\\Games\\portal2.exe","SteamAppId":"620","LaunchArguments":"-novid","futureMetadata":{"keep":true},"SteamPlaytimeMinutes":1430,"PlayedSeconds":30}]);
        let bytes = serde_json::to_vec(&record).unwrap();
        fs::write(&original, &bytes).unwrap();
        assert_eq!(
            handle(&profile, &original, "import_legacy", &json!({})).unwrap()["imported"],
            1
        );
        assert_eq!(
            handle(&profile, &original, "import_legacy", &json!({})).unwrap()["imported"],
            0
        );
        let items = parse(&profile.join("games.json")).unwrap();
        let id = text(&items[0], "LibraryId");
        handle(&profile, &original, "pin", &json!({"id":id})).unwrap();
        let updated = parse(&profile.join("games.json")).unwrap();
        assert_eq!(updated[0]["futureMetadata"]["keep"], true);
        assert_eq!(updated[0]["LaunchArguments"], "-novid");
        assert_eq!(view(&updated[0])["playedSeconds"], 85800.);
        assert_eq!(fs::read(original).unwrap(), bytes);
    }
    #[test]
    fn corrupted_primary_uses_backup_without_destroying_corrupted_bytes() {
        let root = area();
        let profile = root.join("next");
        fs::create_dir_all(&profile).unwrap();
        let bad = b"invalid json";
        fs::write(profile.join("games.json"), bad).unwrap();
        let record = json!([{"Name":"Valheim","ExePath":"C:\\Games\\valheim.exe","LibraryId":"11111111111111111111111111111111"}]);
        fs::write(
            profile.join("games.json.bak"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        let boot = handle(
            &profile,
            &root.join("missing.json"),
            "bootstrap",
            &json!({}),
        )
        .unwrap();
        assert_eq!(boot["games"].as_array().unwrap().len(), 1);
        assert!(boot["notice"].as_str().is_some());
        handle(
            &profile,
            &root.join("missing.json"),
            "pin",
            &json!({"id":"11111111111111111111111111111111"}),
        )
        .unwrap();
        assert!(fs::read_dir(&profile)
            .unwrap()
            .filter_map(Result::ok)
            .any(|f| f
                .file_name()
                .to_string_lossy()
                .starts_with("games-corrupt-")
                && fs::read(f.path()).unwrap() == bad));
        assert_eq!(parse(&profile.join("games.json.bak")).unwrap().len(), 1);
    }
    #[cfg(windows)]
    #[test]
    fn compatibility_session_blocks_native_mutation_but_not_reading() {
        let root = area();
        let profile = root.join("next");
        let _guard = lock(&profile).unwrap();
        assert!(handle(
            &profile,
            &root.join("legacy.json"),
            "pin",
            &json!({"id":"x"})
        )
        .is_err());
        assert!(handle(&profile, &root.join("legacy.json"), "bootstrap", &json!({})).is_ok());
    }
}
