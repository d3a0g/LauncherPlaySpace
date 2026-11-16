use crate::{
    common::*,
    library::{self, set, text},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{path::Path, ptr};
use windows_sys::Win32::Security::Credentials::*;
fn target(profile: &Path) -> Vec<u16> {
    format!(
        "PlaySpaceNext.Steam.{:X}\0",
        Sha256::digest(profile.to_string_lossy().as_bytes())
    )
    .encode_utf16()
    .collect()
}
fn credential(profile: &Path) -> Result<Option<Value>> {
    let target = target(profile);
    let mut pointer = ptr::null_mut();
    if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut pointer) } == 0 {
        return Ok(None);
    }
    let result = unsafe {
        let credential = &*pointer;
        let bytes = std::slice::from_raw_parts(
            credential.CredentialBlob,
            credential.CredentialBlobSize as usize,
        );
        serde_json::from_slice(bytes)
            .map_err(|_| "Stored Steam credentials are invalid".to_string())
    };
    unsafe {
        CredFree(pointer as _);
    }
    result.map(Some)
}
pub fn handle(profile: &Path, p: &Value) -> Result<Value> {
    let action = str(p, "action");
    if action == "save" {
        let user = str(p, "user");
        let key = str(p, "key");
        if !regex::Regex::new(r"^\d{17}$").unwrap().is_match(user)
            || key.len() != 32
            || !key.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err("Enter a 17-digit Steam ID and a 32-character API key".into());
        }
        let bytes = serde_json::to_vec(&json!({"user":user,"key":key})).map_err(err)?;
        let mut bytes = bytes;
        let mut target = target(profile);
        let mut credential: CREDENTIALW = unsafe { std::mem::zeroed() };
        credential.Type = CRED_TYPE_GENERIC;
        credential.TargetName = target.as_mut_ptr();
        credential.CredentialBlob = bytes.as_mut_ptr();
        credential.CredentialBlobSize = bytes.len() as u32;
        credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
        if unsafe { CredWriteW(&credential, 0) } == 0 {
            return Err("Windows could not store Steam credentials".into());
        }
    }
    if action == "remove" {
        unsafe {
            CredDeleteW(target(profile).as_ptr(), CRED_TYPE_GENERIC, 0);
        }
    }
    if action == "sync" {
        let credentials = credential(profile)?.ok_or("Connect Steam first")?;
        if prefs(profile)?["OfflineMode"] == true {
            return Err("Steam sync is unavailable offline".into());
        }
        let response = http()?
            .get("https://api.steampowered.com/IPlayerService/GetOwnedGames/v0001/")
            .query(&[
                ("key", str(&credentials, "key")),
                ("steamid", str(&credentials, "user")),
                ("include_appinfo", "1"),
                ("include_played_free_games", "1"),
                ("format", "json"),
            ])
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|_| "Steam sync failed; check the account privacy settings and API key")?;
        let value: Value = response
            .json()
            .map_err(|_| "Steam returned an invalid response")?;
        let owned = value["response"]["games"]
            .as_array()
            .ok_or("Steam library is private or unavailable")?;
        let _guard = library::lock(profile)?;
        let mut games = library::load(&profile.join("games.json"))?.0;
        let mut count = 0;
        for game in &mut games {
            let id = text(game, "SteamAppId");
            if let Some(record) = owned
                .iter()
                .find(|v| v["appid"].as_u64().is_some_and(|n| n.to_string() == id))
            {
                set(
                    game,
                    "SteamPlaytimeMinutes",
                    record["playtime_forever"].clone(),
                );
                set(game, "SteamPlaytimeUser", credentials["user"].clone());
                set(game, "SteamPlaytimeUpdatedUtc", json!(now()));
                count += 1;
            }
        }
        library::save(profile, &games)?;
        audit(profile, "Steam", "Steam library playtime updated");
        return Ok(json!({"updated":count}));
    }
    if action == "achievements" {
        let game = game(profile, p)?;
        let credentials = credential(profile)?.ok_or("Connect Steam first")?;
        let steam_id = text(&game, "SteamAppId");
        if steam_id.parse::<u32>().is_err() {
            return Err("Steam game ID required".into());
        }
        let lang = if language(profile) == "en" {
            "english"
        } else {
            "russian"
        };
        let response = http()?
            .get("https://api.steampowered.com/ISteamUserStats/GetPlayerAchievements/v0001/")
            .query(&[
                ("key", str(&credentials, "key")),
                ("steamid", str(&credentials, "user")),
                ("appid", &steam_id),
                ("l", lang),
            ])
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|_| "Steam achievements are unavailable")?;
        let value: Value = response.json().map_err(|_| "Invalid Steam response")?;
        let entries = value["playerstats"]["achievements"]
            .as_array()
            .ok_or("No achievement data for this game")?;
        let unlocked = entries.iter().filter(|e| e["achieved"] == 1).count();
        let summary = format!("{unlocked}/{}", entries.len());
        update_game(profile, str(p, "id"), |g| {
            set(g, "AchievementSummary", json!(summary));
            Ok(())
        })?;
        return Ok(json!({"summary":summary,"items":entries}));
    }
    let credentials = credential(profile)?;
    Ok(json!({"connected":credentials.is_some(),"user":credentials.as_ref().map(|v|str(v,"user"))}))
}
