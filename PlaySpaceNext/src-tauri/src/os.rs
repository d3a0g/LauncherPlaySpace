use crate::{common::*, library::text};
use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use winreg::{enums::*, RegKey};
pub fn command(path: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> Result<String> {
    let mut child = Command::new(path)
        .args(args)
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(err)?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(err)? {
            let output = child.wait_with_output().map_err(err)?;
            if !status.success() {
                return Err(String::from_utf8_lossy(&output.stdout)
                    .chars()
                    .take(2000)
                    .collect());
            }
            return Ok(String::from_utf8_lossy(&output.stdout)
                .chars()
                .take(20000)
                .collect());
        }
        if start.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            return Err("Command timed out".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
pub fn open_url(url: &str) -> Result<Value> {
    let uri = reqwest::Url::parse(url).map_err(err)?;
    if !(uri.scheme() == "https"
        && uri.username().is_empty()
        && uri.host_str().is_some_and(|h| {
            matches!(
                h,
                "store.steampowered.com"
                    | "store.epicgames.com"
                    | "one.one.one.one"
                    | "www.cloudflare.com"
                    | "developers.cloudflare.com"
                    | "thunderstore.io"
            )
        })
        || uri.scheme() == "steam"
            && matches!(
                uri.host_str(),
                Some("rungameid" | "validate" | "url" | "nav" | "open")
            ))
    {
        return Err("Unsupported store URL".into());
    }
    launch_uri(url)
}
pub fn open_project_link(p: &Value) -> Result<Value> {
    let url = match str(p, "id") {
        "support" => "https://t.me/PlaySpaceSupport_bot",
        "donate" => "https://tips.tips/000492195",
        _ => return Err("Unknown project link".into()),
    };
    launch_uri(url)
}
fn launch_uri(url: &str) -> Result<Value> {
    let operation: Vec<u16> = "open\0".encode_utf16().collect();
    let file: Vec<u16> = format!("{url}\0").encode_utf16().collect();
    let result = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if result as isize <= 32 {
        return Err("Windows could not open the application".into());
    }
    Ok(json!({"opened":true}))
}
pub fn open_store(p: &Value) -> Result<Value> {
    let id = str(p, "id");
    if !id.is_empty() {
        if !id.parse::<u32>().is_ok_and(|n| n > 0) {
            return Err("Invalid Steam ID".into());
        }
        return open_url(&format!("https://store.steampowered.com/app/{id}/"));
    }
    open_url(str(p, "url"))
}
pub fn autostart(enabled: Option<bool>) -> Result<bool> {
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
        .map_err(err)?
        .0;
    if let Some(value) = enabled {
        if value {
            let exe = std::env::current_exe().map_err(err)?;
            key.set_value("PlaySpaceNext", &format!("\"{}\"", exe.display()))
                .map_err(err)?;
        } else {
            let _ = key.delete_value("PlaySpaceNext");
        }
    }
    Ok(key.get_value::<String, _>("PlaySpaceNext").is_ok())
}
fn verify_cloudflare(path: &Path) -> Result<()> {
    let script="$p=[Console]::In.ReadToEnd();$s=Get-AuthenticodeSignature -LiteralPath $p;if($s.Status -ne 'Valid' -or $s.SignerCertificate.Subject -notmatch 'CN=Cloudflare,? (Inc\\.|Ltd\\.)'){exit 1};Write-Output 'VALID'";
    let mut child = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x08000000)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(err)?;
    child
        .stdin
        .take()
        .ok_or("Signature check unavailable")?
        .write_all(path.to_string_lossy().as_bytes())
        .map_err(err)?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(err)? {
            return if status.success() {
                Ok(())
            } else {
                Err("Cloudflare signature could not be verified".into())
            };
        }
        if start.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            return Err("Signature verification timed out".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
pub fn network(p: &Value) -> Result<Value> {
    let path = PathBuf::from(std::env::var_os("ProgramFiles").unwrap_or_default())
        .join("Cloudflare/Cloudflare WARP/warp-cli.exe");
    let action = str(p, "action");
    if action == "trace" {
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(12))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(err)?;
        let trace = client
            .get("https://www.cloudflare.com/cdn-cgi/trace")
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(err)?
            .text()
            .map_err(err)?;
        return Ok(
            json!({"tunnel":trace.lines().any(|s|matches!(s.trim(),"warp=on"|"warp=plus"))}),
        );
    }
    if action == "diagnostics" {
        let mut results = vec![];
        for (name, url) in [
            ("Steam", "https://store.steampowered.com/"),
            ("Epic Games", "https://store.epicgames.com/"),
            ("Thunderstore", "https://thunderstore.io/"),
        ] {
            let timer = Instant::now();
            let ok = http()?
                .head(url)
                .send()
                .is_ok_and(|r| r.status().as_u16() < 500);
            results.push(json!({"name":name,"ok":ok,"milliseconds":timer.elapsed().as_millis()}));
        }
        return Ok(json!(results));
    }
    if !path.exists() {
        return Ok(
            json!({"state":"missing","message":"Install the official Cloudflare WARP component"}),
        );
    }
    no_links(&path)?;
    verify_cloudflare(&path)?;
    let run = |args: &[&str]| {
        let mut all = vec!["--no-ansi", "--no-paginate"];
        all.extend_from_slice(args);
        command(path.as_os_str(), &all)
    };
    if action == "connect" {
        if p["accepted"] != true {
            return Err("Accept the Cloudflare terms before connecting".into());
        }
        let registration = run(&["--accept-tos", "registration", "show"]);
        match registration {
            Ok(v) => {
                let lower = v.to_lowercase();
                if lower.contains("organization:")
                    || lower.contains("zerotrust")
                    || lower.contains("account type: team")
                {
                    return Err(
                        "Managed WARP profiles must be controlled in the official client".into(),
                    );
                }
            }
            Err(v) => {
                let v = v.to_lowercase();
                if v.contains("registration")
                    && (v.contains("missing")
                        || v.contains("not found")
                        || v.contains("not registered"))
                {
                    run(&["--accept-tos", "registration", "new"])?;
                } else {
                    return Err("Could not verify WARP registration".into());
                }
            }
        }
        run(&["--accept-tos", "mode", "warp+doh"])?;
        run(&["--accept-tos", "connect"])?;
    }
    if action == "disconnect" {
        run(&["disconnect"])?;
    }
    let output = run(&["status"])?;
    let state = if output
        .lines()
        .any(|l| l.trim().trim_start_matches("Status update:").trim() == "Connected")
    {
        "connected"
    } else if output
        .lines()
        .any(|l| l.trim().trim_start_matches("Status update:").trim() == "Disconnected")
    {
        "disconnected"
    } else {
        "unknown"
    };
    Ok(json!({"state":state,"message":output.trim()}))
}
pub fn install_warp(profile: &Path, p: &Value) -> Result<Value> {
    if p["accepted"] != true {
        return Err("Accept the Cloudflare terms before installing".into());
    }
    let job = crate::downloads::begin("Cloudflare WARP");
    let file = profile.join(format!("warp-install/{}.msi", id()));
    let result = (|| {
        let bytes = crate::downloads::cloudflare(profile, &job)?;
        if bytes.len() < 8 || bytes[..8] != [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1] {
            return Err("The Cloudflare download is not a Windows MSI installer".into());
        }
        atomic(&file, &bytes)?;
        verify_cloudflare(&file)?;
        if crate::downloads::cancelled(&job) {
            return Err("Cancelled".into());
        }
        crate::downloads::progress(
            &job,
            bytes.len() as u64,
            Some(bytes.len() as u64),
            "Waiting for Windows installation approval",
        );
        let installer =
            PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()))
                .join("System32/msiexec.exe");
        let verb: Vec<u16> = "runas\0".encode_utf16().collect();
        let executable: Vec<u16> = format!("{}\0", installer.display())
            .encode_utf16()
            .collect();
        let parameters: Vec<u16> = format!("/i \"{}\" /qn /norestart\0", file.display())
            .encode_utf16()
            .collect();
        let mut info: windows_sys::Win32::UI::Shell::SHELLEXECUTEINFOW =
            unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of_val(&info) as u32;
        info.fMask = 0x40;
        info.lpVerb = verb.as_ptr();
        info.lpFile = executable.as_ptr();
        info.lpParameters = parameters.as_ptr();
        info.nShow = 0;
        if unsafe { windows_sys::Win32::UI::Shell::ShellExecuteExW(&mut info) } == 0 {
            return Err("Windows installation was cancelled or could not start".into());
        }
        unsafe {
            windows_sys::Win32::System::Threading::WaitForSingleObject(info.hProcess, u32::MAX);
        }
        let mut code = 0;
        let valid = unsafe {
            windows_sys::Win32::System::Threading::GetExitCodeProcess(info.hProcess, &mut code)
        };
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(info.hProcess);
        }
        if valid == 0 || !matches!(code, 0 | 3010) {
            return Err(format!("Windows Installer returned code {code}"));
        }
        Ok(json!({"installed":true,"restartRequired":code==3010}))
    })();
    crate::downloads::finish(&job, &result);
    if file.exists() {
        let _ = std::fs::remove_file(file);
    }
    result
}
pub fn game_update(profile: &Path, p: &Value) -> Result<Value> {
    let g = game(profile, p)?;
    let id = text(&g, "SteamAppId");
    if !id.parse::<u32>().is_ok_and(|n| n > 0) {
        return Err("Manage updates in the game's installed client".into());
    }
    open_url(&format!(
        "steam://{}{}",
        if str(p, "action") == "verify" {
            "validate/"
        } else {
            "rungameid/"
        },
        id
    ))
}
pub fn open_folder(profile: &Path, p: &Value) -> Result<Value> {
    let path = if str(p, "target") == "game" {
        let g = game(profile, p)?;
        PathBuf::from(text(&g, "InstallDirectory"))
    } else {
        profile.to_path_buf()
    };
    if !path.is_dir() {
        return Err("Folder does not exist".into());
    }
    no_links(&path)?;
    Command::new("explorer.exe")
        .arg(path)
        .creation_flags(0x08000000)
        .spawn()
        .map_err(err)?;
    Ok(json!({"opened":true}))
}
