use crate::{common::*, dlss, downloads, library::text};
use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
fn info() -> Value {
    serde_json::from_str(include_str!("../catalog/universal-dlss.json")).unwrap()
}
fn validate(root: &Path) -> Result<()> {
    for (name, expected) in info()["files"].as_object().unwrap() {
        let path = safe(root, name)?;
        if !path.is_file()
            || hash(&path)?.to_lowercase() != expected.as_str().unwrap_or("").to_lowercase()
        {
            return Err(format!("UniversalDLSS5 component hash mismatch: {name}"));
        }
    }
    Ok(())
}
fn controller(profile: &Path) -> Option<PathBuf> {
    let root = profile.join("tools/UniversalDLSS5/v0.5.1");
    let registered = read(
        &profile.join("tools/UniversalDLSS5/controller.json"),
        Value::Null,
    )
    .ok()
    .and_then(|v| v.as_str().map(PathBuf::from));
    let mut paths = registered.into_iter().collect::<Vec<_>>();
    paths.push(root.join("UniversalDLSS5.exe"));
    for key in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = std::env::var_os(key) {
            paths.push(PathBuf::from(root).join("UniversalDLSS5/UniversalDLSS5.exe"));
        }
    }
    paths.into_iter().find(|p| {
        p.file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("UniversalDLSS5.exe"))
            && p.parent().is_some_and(|r| validate(r).is_ok())
    })
}
fn runtime(controller: Option<&Path>) -> PathBuf {
    if let Ok(key) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey("Software\\UniversalDLSS5")
    {
        if let Ok(path) = key.get_value::<String, _>("RuntimePath") {
            if Path::new(&path).is_absolute() {
                return PathBuf::from(path);
            }
        }
    }
    if let Some(controller) = controller {
        let folder = controller.parent().unwrap().join("runtime");
        if folder.join("nvngx_dlssnr.dll").is_file() {
            return folder;
        }
    }
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default())
        .join("UniversalDLSS5/runtime")
}
pub fn handle(profile: &Path, p: &Value) -> Result<Value> {
    let action = str(p, "action");
    if action == "register" {
        let path = Path::new(str(p, "path"));
        if !path.is_file()
            || !path
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("UniversalDLSS5.exe"))
        {
            return Err("Select UniversalDLSS5.exe".into());
        }
        validate(path.parent().unwrap())?;
        write(
            &profile.join("tools/UniversalDLSS5/controller.json"),
            &json!(path),
        )?;
    }
    if action == "install" {
        let data = info();
        let job = downloads::begin("UniversalDLSS5 0.5.1");
        let stage = profile.join(format!("tools/UniversalDLSS5/.stage-{}", id()));
        let destination = profile.join("tools/UniversalDLSS5/v0.5.1");
        let result = (|| {
            if destination.exists() {
                validate(&destination)?;
                return Ok(json!({"controller":destination.join("UniversalDLSS5.exe")}));
            }
            let bytes = downloads::trusted(profile, &job, str(&data, "url"), 16 * 1024 * 1024)?;
            let installer = profile.join("tools/UniversalDLSS5/installer-0.5.1.exe");
            atomic(&installer, &bytes)?;
            if hash(&installer)?.to_lowercase() != str(&data, "sha256") {
                return Err("UniversalDLSS5 installer hash mismatch".into());
            }
            let zip = dlss::sevenzip()?;
            fs::create_dir_all(&stage).map_err(err)?;
            let output = format!("-o{}", stage.display());
            crate::os::command(
                zip,
                &[
                    "x",
                    "-y",
                    "-bd",
                    &output,
                    "-xr!$PLUGINSDIR",
                    &installer.to_string_lossy(),
                ],
            )?;
            files(&stage, 128 * 1024 * 1024)?;
            validate(&stage)?;
            let uninstall = stage.join("Uninstall.exe");
            if uninstall.exists() {
                fs::remove_file(uninstall).map_err(err)?;
            }
            fs::rename(&stage, &destination).map_err(err)?;
            let controller = destination.join("UniversalDLSS5.exe");
            write(
                &profile.join("tools/UniversalDLSS5/controller.json"),
                &json!(controller),
            )?;
            Ok(json!({"controller":controller}))
        })();
        downloads::finish(&job, &result);
        if stage.exists() {
            no_links(&stage)?;
            fs::remove_dir_all(&stage).map_err(err)?;
        }
        return result;
    }
    let controller = controller(profile);
    let runtime = runtime(controller.as_deref());
    if action == "runtime" {
        let source = Path::new(str(p, "path"));
        if !source
            .file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("nvngx_dlssnr.dll"))
        {
            return Err("Select NVIDIA nvngx_dlssnr.dll, not Super Resolution".into());
        }
        pe64(source, true)?;
        let verified = hash(source)?;
        dlss::verify_nvidia(source)?;
        no_links(&runtime)?;
        let target = runtime.join("nvngx_dlssnr.dll");
        if target.exists() {
            if hash(&target)? == verified {
                return Ok(json!({"imported":true}));
            }
            return Err("A different NVIDIA runtime already exists; use the upstream controller to replace it".into());
        }
        let bytes = fs::read(source).map_err(err)?;
        use sha2::{Digest, Sha256};
        if format!("{:X}", Sha256::digest(&bytes)) != verified {
            return Err("NVIDIA runtime changed after verification".into());
        }
        atomic(&target, &bytes)?;
    }
    if action == "open" || action == "launch" {
        let controller = controller
            .as_ref()
            .ok_or("Install or register the verified controller first")?;
        validate(controller.parent().unwrap())?;
        let system = sysinfo::System::new_all();
        if system.processes().values().any(|p| {
            p.name()
                .to_string_lossy()
                .eq_ignore_ascii_case("UniversalDLSS5.exe")
        }) {
            return Err("The controller is already running; use its existing window".into());
        }
        let mut command = Command::new(controller);
        command
            .current_dir(controller.parent().unwrap())
            .creation_flags(0x08000000);
        if action == "launch" {
            if p["confirmed"] != true {
                return Err("Confirm experimental single-player mode".into());
            }
            let mut game = game(profile, p)?;
            let executable = if str(p, "executable").is_empty() {
                PathBuf::from(text(&game, "ExePath"))
            } else {
                PathBuf::from(str(p, "executable"))
            };
            let root = crate::mods::game_root(&game)?;
            no_links(&executable)?;
            let full = executable.canonicalize().map_err(err)?;
            let root = root.canonicalize().map_err(err)?;
            if !full.starts_with(&root)
                || !executable
                    .extension()
                    .is_some_and(|s| s.eq_ignore_ascii_case("exe"))
            {
                return Err("The selected executable is outside the game folder".into());
            }
            let name = executable
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            if [
                "steam",
                "epicgameslauncher",
                "riotclientservices",
                "dowser",
                "universaldlss5",
            ]
            .contains(&name.as_str())
            {
                return Err("Select the game process, not a launcher or controller".into());
            }
            let windows = PathBuf::from(std::env::var_os("WINDIR").unwrap_or_default());
            if full
                .to_string_lossy()
                .to_lowercase()
                .starts_with(&(windows.to_string_lossy().to_lowercase() + "\\"))
            {
                return Err("System applications are not supported".into());
            }
            if dlss::blocked(&game).unwrap_or(true) {
                return Err("Experimental injection is disabled for this protected game".into());
            }
            crate::saves::closed(&game)?;
            game["ExePath"] = json!(executable);
            pe64(&executable, false)?;
            let file = runtime.join("nvngx_dlssnr.dll");
            pe64(&file, true)?;
            dlss::verify_nvidia(&file)?;
            if !text(&game, "SteamAppId").is_empty()
                && !system
                    .processes()
                    .values()
                    .any(|p| p.name().to_string_lossy().eq_ignore_ascii_case("steam.exe"))
            {
                return Err("Open Steam before launching this Steam game".into());
            }
            command.arg("--launch").arg(executable);
        }
        command.spawn().map_err(err)?;
        return Ok(json!({"accepted":true,"neuralActiveConfirmed":false}));
    }
    Ok(
        json!({"controller":controller,"runtime":runtime,"hasRuntime":runtime.join("nvngx_dlssnr.dll").exists()}),
    )
}
