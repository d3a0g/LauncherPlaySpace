use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
pub type Result<T> = std::result::Result<T, String>;
static HISTORY_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
static AUDIT_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
pub fn id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
pub fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub fn str<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
pub fn read(path: &Path, fallback: Value) -> Result<Value> {
    if !path.exists() {
        return Ok(fallback);
    }
    if fs::metadata(path).map_err(err)?.len() > 32 * 1024 * 1024 {
        return Err("File exceeds the size limit".into());
    }
    let bytes = fs::read(path).map_err(err)?;
    serde_json::from_slice(bytes.strip_prefix(&[239, 187, 191]).unwrap_or(&bytes)).map_err(err)
}
pub fn write(path: &Path, value: &Value) -> Result<()> {
    atomic(path, &serde_json::to_vec_pretty(value).map_err(err)?)
}
pub fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    no_links(path)?;
    let parent = path.parent().ok_or("Invalid destination")?;
    fs::create_dir_all(parent).map_err(err)?;
    let tmp = parent.join(format!(".next-{}.tmp", id()));
    let result = (|| {
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp)
            .map_err(err)?;
        f.write_all(bytes).and_then(|_| f.sync_all()).map_err(err)?;
        drop(f);
        fs::rename(&tmp, path).map_err(err)
    })();
    if tmp.exists() {
        let _ = fs::remove_file(tmp);
    }
    result
}
pub fn no_links(path: &Path) -> Result<()> {
    for part in path.ancestors() {
        if let Ok(m) = fs::symlink_metadata(part) {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if m.file_attributes() & 0x400 != 0 {
                    return Err("Linked paths are not supported".into());
                }
            }
            if m.file_type().is_symlink() {
                return Err("Linked paths are not supported".into());
            }
        }
    }
    Ok(())
}
pub fn specific_directory(path: &Path) -> Result<()> {
    if !path.is_absolute() || path.parent().is_none() {
        return Err("Choose a specific application folder".into());
    }
    no_links(path)?;
    let normalized = path
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_lowercase();
    for name in [
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "PROGRAMDATA",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "WINDIR",
    ] {
        if let Some(root) = std::env::var_os(name) {
            let root = Path::new(&root)
                .to_string_lossy()
                .trim_end_matches(['\\', '/'])
                .to_lowercase();
            if normalized == root || name == "WINDIR" && normalized.starts_with(&(root + "\\")) {
                return Err(
                    "Protected or shared folders cannot be managed as a game folder".into(),
                );
            }
        }
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        for folder in ["Desktop", "Documents", "Downloads"] {
            if normalized
                == PathBuf::from(&home)
                    .join(folder)
                    .to_string_lossy()
                    .to_lowercase()
            {
                return Err("Choose the specific game's folder, not a shared user folder".into());
            }
        }
    }
    Ok(())
}
pub fn safe(root: &Path, relative: &str) -> Result<PathBuf> {
    let normalized = relative.replace('\\', "/");
    if normalized.is_empty()
        || normalized.len() > 1000
        || normalized.split('/').any(|p| {
            p.is_empty()
                || p == "."
                || p == ".."
                || p.contains(':')
                || p.chars().any(|c| c < ' ' || "<>|?*\"".contains(c))
                || p.ends_with(['.', ' '])
                || matches!(
                    p.split('.').next().unwrap_or("").to_uppercase().as_str(),
                    "CON"
                        | "PRN"
                        | "AUX"
                        | "NUL"
                        | "COM1"
                        | "COM2"
                        | "COM3"
                        | "COM4"
                        | "COM5"
                        | "COM6"
                        | "COM7"
                        | "COM8"
                        | "COM9"
                        | "LPT1"
                        | "LPT2"
                        | "LPT3"
                        | "LPT4"
                        | "LPT5"
                        | "LPT6"
                        | "LPT7"
                        | "LPT8"
                        | "LPT9"
                )
        })
    {
        return Err("Unsafe relative path".into());
    }
    let path = root.join(normalized);
    no_links(&path)?;
    Ok(path)
}
pub fn files(root: &Path, max: u64) -> Result<Vec<PathBuf>> {
    no_links(root)?;
    let mut queue = vec![root.to_path_buf()];
    let mut files = vec![];
    let mut total = 0;
    let mut entries = 0;
    while let Some(dir) = queue.pop() {
        if !dir.exists() {
            continue;
        }
        for item in fs::read_dir(dir).map_err(err)? {
            let p = item.map_err(err)?.path();
            no_links(&p)?;
            entries += 1;
            if entries > 30000 {
                return Err("Too many files".into());
            }
            if p.is_dir() {
                queue.push(p)
            } else {
                total += fs::metadata(&p).map_err(err)?.len();
                if total > max {
                    return Err("Files exceed the size limit".into());
                }
                files.push(p)
            }
        }
    }
    Ok(files)
}
pub fn hash(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).map_err(err)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).map_err(err)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:X}", h.finalize()))
}
pub fn game(profile: &Path, p: &Value) -> Result<Value> {
    crate::library::load(&profile.join("games.json"))?
        .0
        .into_iter()
        .find(|g| crate::library::text(g, "LibraryId") == str(p, "id"))
        .ok_or("Game not found".into())
}
pub fn update_game(
    profile: &Path,
    id: &str,
    update: impl FnOnce(&mut Value) -> Result<()>,
) -> Result<()> {
    let _guard = crate::library::lock(profile)?;
    let mut games = crate::library::load(&profile.join("games.json"))?.0;
    let g = games
        .iter_mut()
        .find(|g| crate::library::text(g, "LibraryId") == id)
        .ok_or("Game not found")?;
    update(g)?;
    crate::library::save(profile, &games)
}
pub fn history(profile: &Path, g: &Value, kind: &str, text: &str) {
    let _guard = HISTORY_GATE.lock().unwrap();
    let path = profile.join("activity-history.json");
    if let Ok(mut value) = read(&path, json!([])) {
        if let Some(list) = value.as_array_mut() {
            list.push(json!({"Time":now(),"GameId":crate::library::text(g,"LibraryId"),"GameName":crate::library::text(g,"Name"),"Kind":kind,"Text":text}));
            if list.len() > 1000 {
                list.drain(..list.len() - 1000);
            }
            let _ = write(&path, &value);
        }
    }
    audit(profile, kind, text);
}
pub fn audit(profile: &Path, area: &str, text: &str) {
    let _guard = AUDIT_GATE.lock().unwrap();
    let path = profile.join("audit/events.jsonl");
    let _ = fs::create_dir_all(path.parent().unwrap());
    if fs::metadata(&path)
        .map(|m| m.len() > 2 * 1024 * 1024)
        .unwrap_or(false)
    {
        let _ = fs::rename(&path, profile.join("audit/previous.jsonl"));
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(
            file,
            "{}",
            json!({"Time":now(),"Area":area,"Action":text.chars().take(2000).collect::<String>()})
        );
    }
}
pub fn http() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("PlaySpaceNext/0.2")
        .redirect(reqwest::redirect::Policy::limited(4))
        .build()
        .map_err(err)
}
pub fn http_bytes(url: &str, max: u64) -> Result<Vec<u8>> {
    let response = http()?
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(err)?;
    if response.content_length().is_some_and(|n| n > max) {
        return Err("Download exceeds the size limit".into());
    }
    let mut bytes = vec![];
    response
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(err)?;
    if bytes.len() as u64 > max {
        return Err("Download exceeds the size limit".into());
    }
    Ok(bytes)
}
pub fn prefs(profile: &Path) -> Result<Value> {
    let mut v = read(&profile.join("preferences.json"), json!({}))?;
    for (k, x) in [
        ("Language", json!("ru")),
        ("OfflineMode", json!(false)),
        ("ReduceMotion", json!(false)),
        ("CloseBehavior", json!(2)),
        ("DownloadLimitBytesPerSecond", json!(0)),
        ("HideWhilePlaying", json!(false)),
        ("RestoreAfterPlaying", json!(true)),
    ] {
        if v.get(k).is_none() {
            v[k] = x;
        }
    }
    Ok(v)
}
pub fn language(profile: &Path) -> String {
    prefs(profile)
        .ok()
        .and_then(|v| v["Language"].as_str().map(|s| s.to_string()))
        .unwrap_or("ru".into())
}
pub fn pe64(path: &Path, dll: bool) -> Result<()> {
    use std::io::{Seek, SeekFrom};
    no_links(path)?;
    let mut file = File::open(path).map_err(err)?;
    let mut dos = [0u8; 64];
    file.read_exact(&mut dos).map_err(err)?;
    if &dos[..2] != b"MZ" {
        return Err("A Windows x64 PE file is required".into());
    }
    let offset = u32::from_le_bytes(dos[60..64].try_into().unwrap()) as u64;
    if offset > 1024 * 1024 {
        return Err("Invalid PE header offset".into());
    }
    file.seek(SeekFrom::Start(offset)).map_err(err)?;
    let mut pe = [0u8; 26];
    file.read_exact(&mut pe).map_err(err)?;
    let flags = u16::from_le_bytes(pe[22..24].try_into().unwrap());
    if &pe[..4] != b"PE\0\0"
        || u16::from_le_bytes(pe[4..6].try_into().unwrap()) != 0x8664
        || u16::from_le_bytes(pe[24..26].try_into().unwrap()) != 0x20b
        || (flags & 0x2000 != 0) != dll
    {
        return Err("A Windows x64 PE file of the expected kind is required".into());
    }
    Ok(())
}
