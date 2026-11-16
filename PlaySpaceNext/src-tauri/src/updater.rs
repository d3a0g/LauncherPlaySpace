use crate::{common::*, downloads};
use serde_json::{json, Value};
#[cfg(windows)]
use std::os::windows::{fs::OpenOptionsExt, process::CommandExt};
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
};
use tauri_plugin_updater::{Update, UpdaterExt};
const UPDATE_URL: &str =
    "https://github.com/d3a0g/playspace-releases/releases/latest/download/latest-v2.json";
const PUBLIC_KEY: &str = include_str!("../update-public-key.pub");
static PENDING: Mutex<Option<Update>> = Mutex::new(None);
pub fn configured() -> bool {
    !UPDATE_URL.trim().is_empty() && !PUBLIC_KEY.trim().is_empty()
}

fn trusted_download(url: &reqwest::Url) -> Result<()> {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || !matches!(
            url.host_str(),
            Some("github.com" | "release-assets.githubusercontent.com")
        )
    {
        return Err("The update installer must use trusted GitHub HTTPS hosting".into());
    }
    Ok(())
}

fn installation_directory(exe: &Path) -> Result<PathBuf> {
    if !exe.is_absolute()
        || !exe
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("LauncherPlaySpace.exe"))
    {
        return Err("The Inno updater requires LauncherPlaySpace.exe. Install the production package or use the correctly named portable EXE.".into());
    }
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "Cannot locate the application directory".into())
}

fn installer_arguments(directory: &Path, log: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "/VERYSILENT",
        "/SUPPRESSMSGBOXES",
        "/SP-",
        "/NORESTART",
        "/CLOSEAPPLICATIONS",
        "/NORESTARTAPPLICATIONS",
        "/PLAYSPACERESTART=1",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    // Command::args handles Windows quoting; never interpolate paths into a shell.
    for (prefix, value) in [("/DIR=", directory), ("/LOG=", log)] {
        let mut arg = OsString::from(prefix);
        arg.push(value);
        args.push(arg);
    }
    args
}

fn launch_installer(update: &Update, bytes: &[u8], profile: &Path, directory: &Path) -> Result<()> {
    if !bytes.starts_with(b"MZ") {
        return Err("The signed update is not a Windows installer EXE".into());
    }
    let staging = profile.join("update-staging");
    no_links(&staging)?;
    fs::create_dir_all(&staging).map_err(err)?;
    no_links(&staging)?;
    let folder = staging.join(uuid::Uuid::new_v4().simple().to_string());
    fs::create_dir(&folder).map_err(err)?;
    let setup = folder.join("setup.exe");
    let result = (|| {
        no_links(&setup)?;
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&setup)
            .map_err(err)?;
        output
            .write_all(bytes)
            .and_then(|_| output.sync_all())
            .map_err(err)?;
        drop(output);
        let mut options = OpenOptions::new();
        options.read(true);
        // Keep this handle alive through CreateProcess: deny writes AND deletion,
        // including replacement/rename, while allowing the installer to read itself.
        #[cfg(windows)]
        options.share_mode(1); // FILE_SHARE_READ
        let mut locked = options.open(&setup).map_err(err)?;
        no_links(&setup)?;
        let mut staged = Vec::new();
        locked.read_to_end(&mut staged).map_err(err)?;
        update.verify(&staged).map_err(err)?;
        let mut command = Command::new(&setup);
        command
            .args(installer_arguments(directory, &folder.join("install.log")))
            .current_dir(&folder)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        let child = command.spawn().map_err(err)?;
        drop(locked);
        drop(child);
        Ok(())
    })();
    if result.is_err() {
        // Only our two known paths; never recursively remove a profile/staging tree.
        let _ = fs::remove_file(&setup);
        let _ = fs::remove_dir(&folder);
    }
    // Keep successful staging and install.log for diagnosing a failed installation.
    result
}
pub async fn handle(app: tauri::AppHandle, profile: &std::path::Path, p: &Value) -> Result<Value> {
    if !configured() {
        return Ok(
            json!({"configured":false,"version":app.package_info().version.to_string(),"available":false}),
        );
    }
    let action = str(p, "action");
    if action == "status" {
        return Ok(json!({"configured":true,"version":app.package_info().version.to_string()}));
    }
    if prefs(profile)?["OfflineMode"] == true {
        return Err("Updates are unavailable offline".into());
    }
    if action == "check" {
        let endpoint = reqwest::Url::parse(UPDATE_URL).map_err(err)?;
        trusted_download(&endpoint)?;
        let update = app
            .updater_builder()
            .pubkey(PUBLIC_KEY.trim())
            .endpoints(vec![endpoint])
            .map_err(err)?
            .build()
            .map_err(err)?
            .check()
            .await
            .map_err(err)?;
        let metadata = update
            .as_ref()
            .map(|u| json!({"version":u.version,"notes":u.body}));
        *PENDING.lock().unwrap() = update;
        return Ok(json!({"configured":true,"available":metadata.is_some(),"release":metadata}));
    }
    if action == "install" {
        if p["confirmed"] != true {
            return Err("Confirm the reviewed native update".into());
        }
        let directory = installation_directory(&std::env::current_exe().map_err(err)?)?;
        no_links(&directory)?;
        let update = {
            PENDING
                .lock()
                .unwrap()
                .take()
                .ok_or("Check for a native update first")?
        };
        if update.version != str(p, "version") {
            return Err("The available update changed; check it again".into());
        }
        trusted_download(&update.download_url)?;
        if crate::runtime::sessions()
            .as_array()
            .is_some_and(|v| !v.is_empty())
        {
            return Err("Close active game sessions before updating".into());
        }
        let job = downloads::begin("PlaySpace Next update");
        let mut received = 0u64;
        let result = update
            .download(
                |bytes, total| {
                    received += bytes as u64;
                    downloads::progress(&job, received, total, "Downloading signed update");
                },
                || {},
            )
            .await
            .map_err(err);
        let bytes = match result {
            Ok(bytes) => bytes,
            Err(e) => {
                downloads::finish(&job, &Err(e.clone()));
                return Err(e);
            }
        };
        if downloads::cancelled(&job) {
            let result = Err("Cancelled".to_string());
            downloads::finish(&job, &result);
            return result;
        }
        if crate::runtime::sessions()
            .as_array()
            .is_some_and(|v| !v.is_empty())
        {
            let result = Err("Close active game sessions before updating".to_string());
            downloads::finish(&job, &result);
            return result;
        }
        // download() verifies raw bytes. Do not use install(): it adds NSIS/MSI
        // arguments, which are incompatible with our single Inno release package.
        downloads::progress(&job, received, Some(received), "Installing signed update");
        let installed =
            launch_installer(&update, &bytes, profile, &directory).map(|_| json!({"started":true}));
        downloads::finish(&job, &installed);
        installed?;
        // Inno restarts the new EXE only after successful installation.
        app.exit(0);
        return Ok(json!({"started":true}));
    }
    Err("Unknown native updater action".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_urls_reject_credentials_http_and_lookalike_hosts() {
        for url in [
            "http://github.com/owner/repo/setup.exe",
            "https://github.com.attacker.test/setup.exe",
            "https://github.com@attacker.test/setup.exe",
            "https://user:password@github.com/setup.exe",
            "https://github.com:444/setup.exe",
            "https://raw.githubusercontent.com/setup.exe",
            "file:///C:/setup.exe",
        ] {
            assert!(
                trusted_download(&reqwest::Url::parse(url).unwrap()).is_err(),
                "{url}"
            );
        }
        for url in [UPDATE_URL, "https://github.com/d3a0g/playspace-releases/releases/download/v2.0.0/setup.exe", "https://release-assets.githubusercontent.com/github-production-release-asset/setup.exe"] {
            assert!(trusted_download(&reqwest::Url::parse(url).unwrap()).is_ok());
        }
    }

    #[test]
    fn updater_requires_production_filename_and_absolute_directory() {
        assert!(installation_directory(Path::new("LauncherPlaySpace.exe")).is_err());
        #[cfg(windows)]
        {
            assert!(installation_directory(Path::new(r"C:\PlaySpace\playspace-next.exe")).is_err());
            assert_eq!(
                installation_directory(Path::new(r"C:\Игры и программы\LauncherPlaySpace.exe"))
                    .unwrap(),
                PathBuf::from(r"C:\Игры и программы")
            );
        }
    }

    #[test]
    fn inno_arguments_preserve_unicode_spaces_and_do_not_use_nsis_switches() {
        let directory = Path::new(r"C:\Игры и программы\PlaySpace");
        let log = Path::new(r"C:\Профиль пользователя\update-staging\job\install.log");
        let args = installer_arguments(directory, log);
        assert_eq!(args.len(), 9);
        assert_eq!(
            args[7],
            OsString::from(format!("/DIR={}", directory.display()))
        );
        assert_eq!(args[8], OsString::from(format!("/LOG={}", log.display())));
        assert!(args.iter().any(|arg| arg == "/PLAYSPACERESTART=1"));
        assert!(args.iter().any(|arg| arg == "/NORESTARTAPPLICATIONS"));
        assert!(!args
            .iter()
            .any(|arg| arg == "/S" || arg == "/UPDATE" || arg == "/NOCLOSEAPPLICATIONS"));
    }
}
