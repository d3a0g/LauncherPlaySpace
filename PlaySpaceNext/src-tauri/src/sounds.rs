use crate::common::*;
use serde_json::{json, Value};
use std::{fs, path::Path};
pub fn validate(path: &Path) -> Result<()> {
    if fs::metadata(path).map_err(err)?.len() > 10 * 1024 * 1024 {
        return Err("WAV must be under 10 MB".into());
    }
    let b = fs::read(path).map_err(err)?;
    if b.len() < 44 || &b[..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("A PCM WAV file is required".into());
    }
    let mut at = 12;
    let mut rate = 0;
    let mut data = 0;
    while at + 8 <= b.len() {
        let size = u32::from_le_bytes(b[at + 4..at + 8].try_into().unwrap()) as usize;
        let start = at + 8;
        let end = start.checked_add(size).ok_or("Invalid WAV chunk")?;
        if end > b.len() {
            return Err("Invalid WAV chunk".into());
        }
        if &b[at..at + 4] == b"fmt " {
            if size < 16 {
                return Err("Invalid WAV format".into());
            }
            let format = u16::from_le_bytes(b[start..start + 2].try_into().unwrap());
            let channels = u16::from_le_bytes(b[start + 2..start + 4].try_into().unwrap()) as u32;
            let hz = u32::from_le_bytes(b[start + 4..start + 8].try_into().unwrap());
            rate = u32::from_le_bytes(b[start + 8..start + 12].try_into().unwrap());
            let align = u16::from_le_bytes(b[start + 12..start + 14].try_into().unwrap()) as u32;
            let bits = u16::from_le_bytes(b[start + 14..start + 16].try_into().unwrap()) as u32;
            if format != 1
                || !(1..=2).contains(&channels)
                || !matches!(bits, 8 | 16)
                || !(8000..=96000).contains(&hz)
                || align != channels * bits / 8
                || rate != hz * align
            {
                return Err("Unsupported PCM WAV format".into());
            }
        }
        if &b[at..at + 4] == b"data" {
            data = size;
        }
        at = end + size % 2;
    }
    if rate == 0 || data == 0 || data as f64 / rate as f64 > 60. {
        return Err("WAV duration must be 1–60 seconds".into());
    }
    Ok(())
}
pub fn play(profile: &Path, name: &str) -> Result<()> {
    let file = safe(&profile.join("game-sounds"), name)?;
    validate(&file)?;
    let path: Vec<u16> = format!("{}\0", file.display()).encode_utf16().collect();
    unsafe {
        windows_sys::Win32::Media::Audio::PlaySoundW(
            path.as_ptr(),
            std::ptr::null_mut(),
            0x00020000 | 0x00000001 | 0x00000002,
        );
    }
    Ok(())
}
pub fn stop() {
    unsafe {
        windows_sys::Win32::Media::Audio::PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0);
    }
}
pub fn handle(profile: &Path, p: &Value) -> Result<Value> {
    let path = profile.join("game-sounds.json");
    let mut settings = read(&path, json!({"Enabled":false,"Games":[]}))?;
    let action = str(p, "action");
    if action == "stop" {
        settings["Enabled"] = json!(false);
        write(&path, &settings)?;
        stop();
    }
    if action == "configure" {
        let g = game(profile, p)?;
        let id = crate::library::text(&g, "LibraryId");
        let min = p["minimum"].as_u64().unwrap_or(45).clamp(10, 3600);
        let max = p["maximum"].as_u64().unwrap_or(180).clamp(min, 3600);
        let games = settings["Games"]
            .as_array_mut()
            .ok_or("Invalid sound settings")?;
        let index = if let Some(index) = games.iter().position(|g| g["GameId"] == id) {
            index
        } else {
            games.push(json!({"GameId":id,"Files":[]}));
            games.len() - 1
        };
        let rule = &mut games[index];
        rule["Enabled"] = json!(p["enabled"] == true);
        rule["MinimumSeconds"] = json!(min);
        rule["MaximumSeconds"] = json!(max);
        settings["Enabled"] = json!(p["master"] == true);
        write(&path, &settings)?;
        if settings["Enabled"] != true {
            stop();
        }
    }
    if action == "import" {
        game(profile, p)?;
        let source = Path::new(str(p, "path"));
        validate(source)?;
        let name = format!("{}.wav", id());
        let target = safe(&profile.join("game-sounds"), &name)?;
        atomic(&target, &fs::read(source).map_err(err)?)?;
        let rule = settings["Games"]
            .as_array_mut()
            .ok_or("Invalid sound settings")?
            .iter_mut()
            .find(|g| str(g, "GameId") == str(p, "id"))
            .ok_or("Configure this game's sounds first")?;
        let files = rule["Files"].as_array_mut().ok_or("Invalid sound files")?;
        if files.len() >= 50 {
            return Err("Sound limit reached".into());
        }
        files.push(json!(name));
        write(&path, &settings)?;
    }
    if action == "remove" {
        let rule = settings["Games"]
            .as_array_mut()
            .ok_or("Invalid sound settings")?
            .iter_mut()
            .find(|g| str(g, "GameId") == str(p, "id"))
            .ok_or("Sound rule not found")?;
        rule["Files"]
            .as_array_mut()
            .ok_or("Invalid sound files")?
            .retain(|x| x != str(p, "name"));
        write(&path, &settings)?;
    }
    if action == "preview" {
        play(profile, str(p, "name"))?;
    }
    Ok(settings)
}
