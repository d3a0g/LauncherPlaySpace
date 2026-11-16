use crate::{
    common::*,
    downloads,
    library::{set, text},
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
pub fn handle(profile: &Path, p: &Value) -> Result<Value> {
    let g = game(profile, p)?;
    let source = crate::mods::game_root(&g)?;
    if str(p, "action") == "get" {
        return Ok(json!({"source":source}));
    }
    crate::saves::closed(&g)?;
    let destination = PathBuf::from(str(p, "destination"));
    if !destination.is_absolute() || !destination.is_dir() {
        return Err("Choose an existing destination folder".into());
    }
    no_links(&destination)?;
    let target = destination.join(source.file_name().ok_or("Invalid game folder")?);
    let from = source
        .canonicalize()
        .map_err(err)?
        .to_string_lossy()
        .to_lowercase();
    let to = destination
        .canonicalize()
        .map_err(err)?
        .join(source.file_name().unwrap())
        .to_string_lossy()
        .to_lowercase();
    if from == to
        || from.starts_with(&(to.clone() + "\\"))
        || to.starts_with(&(from.clone() + "\\"))
        || target.exists()
    {
        return Err("Destination overlaps the source or already exists".into());
    }
    for key in [
        "WINDIR",
        "USERPROFILE",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "LOCALAPPDATA",
        "APPDATA",
        "PROGRAMDATA",
    ] {
        if let Some(folder) = std::env::var_os(key) {
            let protected = Path::new(&folder).to_string_lossy().to_lowercase();
            if from == protected || key == "WINDIR" && from.starts_with(&(protected + "\\")) {
                return Err("Choose a specific game folder".into());
            }
        }
    }
    specific_directory(&source)?;
    specific_directory(&target)?;
    if target.starts_with(profile) || profile.starts_with(&target) {
        return Err("Transfer destination overlaps the launcher profile".into());
    }
    let files = files(&source, 4 * 1024 * 1024 * 1024 * 1024)?;
    let bytes: u64 = files
        .iter()
        .filter_map(|p| fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    if str(p, "action") == "preview" {
        return Ok(
            json!({"source":source,"destination":target,"files":files.len(),"bytes":bytes,"sourceRetained":true}),
        );
    }
    if p["confirmed"] != true {
        return Err("Review the transfer plan first".into());
    }
    let job = downloads::begin("Verified game transfer");
    let stage = destination.join(format!(".playspace-transfer-{}", id()));
    fs::create_dir(&stage).map_err(err)?;
    let result = (|| {
        let mut copied = 0;
        for file in &files {
            if downloads::cancelled(&job) {
                return Err("Cancelled".into());
            }
            no_links(file)?;
            let relative = file
                .strip_prefix(&source)
                .map_err(err)?
                .to_string_lossy()
                .replace('\\', "/");
            let output = safe(&stage, &relative)?;
            fs::create_dir_all(output.parent().unwrap()).map_err(err)?;
            fs::copy(file, &output).map_err(err)?;
            if hash(file)? != hash(&output)? {
                return Err(format!("Transfer integrity check failed: {relative}"));
            }
            copied += fs::metadata(&output).map_err(err)?.len();
            downloads::progress(
                &job,
                copied,
                Some(bytes),
                "Copying and verifying game files",
            );
        }
        crate::saves::closed(&g)?;
        fs::rename(&stage, &target).map_err(err)?;
        let exe = PathBuf::from(text(&g, "ExePath"));
        let result = update_game(profile, str(p, "id"), |g| {
            set(g, "InstallDirectory", json!(target));
            if let Ok(relative) = exe.strip_prefix(&source) {
                set(g, "ExePath", json!(target.join(relative)));
            }
            Ok(())
        });
        if result.is_err() {
            return Err("Game copy is verified, but the library path could not be updated. Both copies were preserved".into());
        }
        history(
            profile,
            &g,
            "Transfer",
            "Game copied and verified; source retained for rollback",
        );
        Ok(json!({"destination":target,"bytes":bytes,"sourceRetained":true}))
    })();
    downloads::finish(&job, &result);
    if stage.exists() {
        no_links(&stage)?;
        fs::remove_dir_all(stage).map_err(err)?;
    }
    result
}
