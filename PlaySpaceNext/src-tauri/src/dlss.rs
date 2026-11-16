use crate::{common::*, downloads, library::text};
use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub fn gpu() -> String {
    let mut names = vec![];
    for index in 0..16 {
        let mut device: windows_sys::Win32::Graphics::Gdi::DISPLAY_DEVICEW =
            unsafe { std::mem::zeroed() };
        device.cb = std::mem::size_of_val(&device) as u32;
        if unsafe {
            windows_sys::Win32::Graphics::Gdi::EnumDisplayDevicesW(
                std::ptr::null(),
                index,
                &mut device,
                0,
            )
        } == 0
        {
            break;
        }
        let len = device
            .DeviceString
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(device.DeviceString.len());
        let name = String::from_utf16_lossy(&device.DeviceString[..len]);
        if !name.is_empty() {
            names.push(name);
        }
    }
    names.join(" / ")
}
pub(crate) fn blocked(g: &Value) -> Result<bool> {
    if [
        "730", "570", "1172470", "252490", "578080", "359550", "594650", "1240440", "1085660",
        "1938090",
    ]
    .contains(&text(g, "SteamAppId").as_str())
    {
        return Ok(true);
    }
    let name = (text(g, "Name") + &text(g, "ExePath")).to_lowercase();
    if [
        "valorant",
        "fortnite",
        "rainbowsix",
        "rainbow six",
        "destiny 2",
        "apex legends",
        "escape from tarkov",
        "call of duty",
        "warzone",
    ]
    .iter()
    .any(|v| name.contains(v))
    {
        return Ok(true);
    }
    let root = crate::mods::game_root(g)?;
    for file in files(&root, 4 * 1024 * 1024 * 1024 * 1024)? {
        let name = file
            .to_string_lossy()
            .to_lowercase()
            .replace(['_', '-'], "");
        if ["easyanticheat", "battleye", "anticheat", "vanguard"]
            .iter()
            .any(|v| name.contains(v))
        {
            return Ok(true);
        }
    }
    Ok(false)
}
pub(crate) fn verify_nvidia(path: &Path) -> Result<()> {
    let script="$p=[Console]::In.ReadToEnd();$s=Get-AuthenticodeSignature -LiteralPath $p;if($s.Status -ne 'Valid' -or $s.SignerCertificate.Subject -notmatch 'NVIDIA CORPORATION'){exit 1}";
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
        .ok_or("Signature verification unavailable")?
        .write_all(path.to_string_lossy().as_bytes())
        .map_err(err)?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(err)? {
            return if status.success() {
                Ok(())
            } else {
                Err("NVIDIA signature verification failed".into())
            };
        }
        if start.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            return Err("Signature verification timed out".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
fn library(profile: &Path, job: &str, tag: &str) -> Result<PathBuf> {
    if !["v310.9.16", "v310.5.3"].contains(&tag) {
        return Err("Unsupported NVIDIA library release".into());
    }
    let path = profile.join(format!("dlss/{tag}-nvngx_dlss.dll"));
    if !path.exists() {
        let bytes=downloads::trusted(profile,job,&format!("https://raw.githubusercontent.com/NVIDIA/DLSS/{tag}/lib/Windows_x86_64/rel/nvngx_dlss.dll"),100*1024*1024)?;
        atomic(&path, &bytes)?;
    }
    verify_nvidia(&path)?;
    Ok(path)
}
pub(crate) fn sevenzip() -> Result<PathBuf> {
    for key in ["ProgramFiles", "ProgramFiles(x86)"] {
        let path = PathBuf::from(std::env::var_os(key).unwrap_or_default()).join("7-Zip/7z.exe");
        if path.is_file() {
            return Ok(path);
        }
    }
    Err("Install 7-Zip to extract this upstream component".into())
}
fn configure(input: &str, changes: &[(&str, &str, String)]) -> Result<String> {
    let mut lines: Vec<String> = input
        .replace("\r\n", "\n")
        .split('\n')
        .map(String::from)
        .collect();
    for (section, key, value) in changes {
        let mut active = false;
        let mut found = false;
        for line in &mut lines {
            if line.trim().starts_with('[') {
                active = line.trim().eq_ignore_ascii_case(&format!("[{section}]"));
            } else if active
                && line
                    .split('=')
                    .next()
                    .is_some_and(|s| s.trim().eq_ignore_ascii_case(key))
            {
                if found {
                    return Err("Duplicate OptiScaler setting".into());
                }
                *line = format!("{key}={value}");
                found = true;
            }
        }
        if !found {
            return Err(format!("Missing OptiScaler setting: {section}/{key}"));
        }
    }
    Ok(lines.join("\r\n"))
}
pub fn handle(profile: &Path, p: &Value) -> Result<Value> {
    let g = game(profile, p)?;
    let root = crate::mods::game_root(&g)?;
    let gpu = gpu();
    let rtx = gpu.to_lowercase().contains("geforce rtx");
    let restricted = blocked(&g).unwrap_or(true);
    let action = str(p, "action");
    let executable = PathBuf::from(text(&g, "ExePath"));
    let binary_root = executable
        .parent()
        .filter(|path| path.starts_with(&root))
        .unwrap_or(&root)
        .to_path_buf();
    let detected: Vec<PathBuf> = files(&root, 4 * 1024 * 1024 * 1024 * 1024)?
        .into_iter()
        .filter(|f| {
            f.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("nvngx_dlss.dll"))
        })
        .filter(|f| !f.to_string_lossy().contains(".playspace-mods"))
        .collect();
    if action == "get" {
        return Ok(
            json!({"gpu":gpu,"compatible":rtx,"restricted":restricted,"files":detected,"versions":["v310.9.16","v310.5.3"],"optiInstalled":read(&binary_root.join(".playspace-mods/installed.json"),Value::Null)?["Packages"].get("PlaySpace-OptiScaler").is_some()}),
        );
    }
    crate::saves::closed(&g)?;
    if action == "opti-remove" {
        return crate::mods::uninstall(&binary_root, "PlaySpace-OptiScaler");
    }
    let target = PathBuf::from(str(p, "target"));
    if matches!(action, "restore" | "install") {
        if !detected.contains(&target) {
            return Err("Choose the game's detected nvngx_dlss.dll".into());
        }
        let ledger = target.with_file_name("nvngx_dlss.dll.playspace-update.json");
        let backup = target.with_file_name("nvngx_dlss.dll.playspace-original");
        no_links(&target)?;
        no_links(&backup)?;
        if action == "restore" {
            let state = read(&ledger, Value::Null)?;
            if hash(&backup)? != str(&state, "OriginalHash")
                || !matches!(hash(&target)?.as_str(),h if h==str(&state,"UpdatedHash")||h==str(&state,"OriginalHash"))
            {
                return Err("DLSS files changed outside PlaySpace; restore cancelled".into());
            }
            atomic(&target, &fs::read(&backup).map_err(err)?)?;
            fs::remove_file(ledger).map_err(err)?;
            fs::remove_file(backup).map_err(err)?;
            return Ok(json!({"restored":true}));
        }
        if !rtx || restricted {
            return Err("DLSS updates are disabled for this GPU or protected game".into());
        }
        if p["confirmed"] != true {
            return Err("Confirm the DLSS update".into());
        }
        let job = downloads::begin("NVIDIA DLSS SR");
        let result = (|| {
            let source = library(profile, &job, str(p, "version"))?;
            let update_hash = hash(&source)?;
            let current = hash(&target)?;
            let original = if ledger.exists() {
                let old = read(&ledger, Value::Null)?;
                if hash(&backup)? != str(&old, "OriginalHash")
                    || current != str(&old, "UpdatedHash") && current != str(&old, "OriginalHash")
                {
                    return Err("DLSS files changed outside PlaySpace".into());
                }
                str(&old, "OriginalHash").to_string()
            } else {
                if backup.exists() {
                    return Err("An untracked original DLSS backup exists".into());
                }
                fs::copy(&target, &backup).map_err(err)?;
                current
            };
            crate::saves::closed(&g)?;
            verify_nvidia(&source)?;
            let bytes = fs::read(&source).map_err(err)?;
            use sha2::{Digest, Sha256};
            if format!("{:X}", Sha256::digest(&bytes)) != update_hash {
                return Err("NVIDIA library changed after verification".into());
            }
            write(
                &ledger,
                &json!({"OriginalHash":original,"UpdatedHash":update_hash}),
            )?;
            atomic(&target, &bytes)?;
            history(
                profile,
                &g,
                "DLSS",
                "NVIDIA library updated with original backup",
            );
            Ok(json!({"updated":true}))
        })();
        downloads::finish(&job, &result);
        return result;
    }
    if action == "opti-install" {
        if !rtx || restricted || p["confirmed"] != true || p["dx12"] != true {
            return Err(
                "OptiScaler requires confirmed DX12, a compatible RTX GPU and an unprotected game"
                    .into(),
            );
        }
        pe64(&executable, false)?;
        let root = binary_root;
        specific_directory(&root)?;
        if root.join("dxgi.dll").exists() || root.join("OptiScaler.ini").exists() {
            return Err("Remove the previous graphics proxy first".into());
        }
        let stage = profile.join(format!("optiscaler-stage/{}", id()));
        fs::create_dir_all(&stage).map_err(err)?;
        let job = downloads::begin("OptiScaler 0.9.4");
        let result = (|| {
            let bytes=downloads::trusted(profile,&job,"https://github.com/optiscaler/OptiScaler/releases/download/v0.9.4/Optiscaler_0.9.4-final.20260718._MM.7z",64*1024*1024)?;
            let archive = stage.join("package.7z");
            atomic(&archive, &bytes)?;
            if hash(&archive)?.to_lowercase()
                != "575cb4df866116093df75af607e37fd70e10f5163e0f23fd5c804142e80ef0ad"
            {
                return Err("OptiScaler package hash mismatch".into());
            }
            let extract = stage.join("payload");
            let output = format!("-o{}", extract.display());
            crate::os::command(
                sevenzip()?,
                &["x", "-y", "-bd", &output, &archive.to_string_lossy()],
            )?;
            let files = files(&extract, 256 * 1024 * 1024)?;
            let expected = [
                "OptiScaler.dll",
                "OptiScaler.ini",
                "amd_fidelityfx_dx12.dll",
                "amd_fidelityfx_framegeneration_dx12.dll",
                "amd_fidelityfx_upscaler_dx12.dll",
                "amd_fidelityfx_vk.dll",
                "dlssg_to_fsr3_amd_is_better.dll",
                "fakenvapi.dll",
                "fakenvapi.ini",
                "libxell.dll",
                "libxess.dll",
                "libxess_dx11.dll",
                "libxess_fg.dll",
                "D3D12_Optiscaler/D3D12Core.dll",
                "Licenses/DirectX_LICENSE.txt",
                "Licenses/FidelityFX_v1_LICENSE.md",
                "Licenses/FidelityFX_v2_LICENSE.md",
                "Licenses/XeSS_LICENSE.txt",
            ];
            let mut mapped = BTreeMap::new();
            for name in expected {
                let file = safe(&extract, name)?;
                if !files.contains(&file) {
                    return Err(format!("Missing OptiScaler payload: {name}"));
                }
                mapped.insert(
                    if name == "OptiScaler.dll" {
                        "dxgi.dll".into()
                    } else {
                        name.into()
                    },
                    file,
                );
            }
            let mode = str(p, "mode");
            let input = match mode {
                "disabled" => "nofg",
                "streamline" => "dlssg",
                "fsr30" => "fsrfg30",
                "nukem" => "nukems",
                "upscaler" => "upscaler",
                _ => "fsrfg",
            };
            let preset = match str(p, "preset") {
                "K" => "11",
                "L" => "12",
                "M" => "13",
                _ => "0",
            };
            let runtime = library(profile, &job, "v310.5.3")?;
            let destination = "PlaySpace.OptiScaler/nvngx_dlss.dll";
            mapped.insert(destination.into(), runtime);
            let file = mapped.get("OptiScaler.ini").unwrap();
            let input_text = fs::read_to_string(file).map_err(err)?;
            let configured = configure(
                &input_text,
                &[
                    ("Upscalers", "Dx12Upscaler", "dlss".into()),
                    ("FrameGen", "Enabled", (mode != "disabled").to_string()),
                    ("FrameGen", "FGInput", input.into()),
                    (
                        "FrameGen",
                        "FGOutput",
                        if mode == "disabled" {
                            "nofg"
                        } else if mode == "nukem" {
                            "nukems"
                        } else {
                            "fsrfg"
                        }
                        .into(),
                    ),
                    ("DLSS", "RenderPresetOverride", (preset != "0").to_string()),
                    ("DLSS", "RenderPresetForAll", preset.into()),
                    (
                        "Libraries",
                        "NvngxDlssPath",
                        root.join(destination).to_string_lossy().to_string(),
                    ),
                ],
            )?;
            atomic(file, configured.as_bytes())?;
            for (name, bytes) in [
                (
                    "OptiScaler-LICENSE.txt",
                    include_bytes!("../catalog/OptiScaler-GPL-3.0.txt").as_slice(),
                ),
                (
                    "NVIDIA-DLSS-LICENSE.txt",
                    include_bytes!("../catalog/NVIDIA-DLSS-LICENSE.txt").as_slice(),
                ),
            ] {
                let relative = format!("PlaySpace.OptiScaler/{name}");
                let path = safe(&stage, &relative)?;
                atomic(&path, bytes)?;
                mapped.insert(relative, path);
            }
            let notice = stage.join("SOURCES.txt");
            atomic(&notice,b"OptiScaler v0.9.4: https://github.com/optiscaler/OptiScaler/tree/v0.9.4\nNVIDIA DLSS: https://github.com/NVIDIA/DLSS/tree/v310.5.3\nUpstream components retain their own licenses.\n")?;
            mapped.insert("PlaySpace.OptiScaler/SOURCES.txt".into(), notice);
            crate::saves::closed(&g)?;
            crate::mods::apply(
                &root,
                &[(
                    "PlaySpace-OptiScaler".into(),
                    "0.9.4".into(),
                    vec![],
                    mapped,
                )],
            )
        })();
        downloads::finish(&job, &result);
        if stage.exists() {
            no_links(&stage)?;
            fs::remove_dir_all(stage).map_err(err)?;
        }
        return result;
    }
    Err("Unknown graphics runtime action".into())
}
