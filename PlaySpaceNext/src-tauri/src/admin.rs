use crate::common::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use rsa::{pkcs1v15::VerifyingKey, signature::Verifier, BigUint, RsaPublicKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};
use winreg::{enums::*, RegKey};
fn subject() -> Result<String> {
    let user = crate::os::command("whoami.exe", &["/user", "/fo", "csv", "/nh"])?;
    let sid = regex::Regex::new(r"S-1-[0-9-]+")
        .unwrap()
        .find(&user)
        .ok_or("Windows user SID unavailable")?
        .as_str();
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(
            "SOFTWARE\\Microsoft\\Cryptography",
            KEY_READ | KEY_WOW64_64KEY,
        )
        .map_err(err)?;
    let guid: String = key.get_value("MachineGuid").map_err(err)?;
    Ok(format!(
        "{:X}",
        Sha256::digest(format!("{sid}|{guid}").as_bytes())
    ))
}
fn verify(bytes: &[u8]) -> Result<Value> {
    if bytes.len() > 16384 {
        return Err("Grant file is too large".into());
    }
    let envelope: Value = serde_json::from_slice(bytes).map_err(err)?;
    let payload = STANDARD.decode(str(&envelope, "Payload")).map_err(err)?;
    let signature = STANDARD.decode(str(&envelope, "Signature")).map_err(err)?;
    let key = include_str!("../catalog/admin-authority.xml");
    let get = |name: &str| -> Result<Vec<u8>> {
        let re = regex::Regex::new(&format!("<{name}>([^<]+)</{name}>")).map_err(err)?;
        let value = re.captures(key).ok_or("Admin public key unavailable")?;
        STANDARD.decode(&value[1]).map_err(err)
    };
    let key = RsaPublicKey::new(
        BigUint::from_bytes_be(&get("Modulus")?),
        BigUint::from_bytes_be(&get("Exponent")?),
    )
    .map_err(err)?;
    VerifyingKey::<Sha256>::new(key)
        .verify(
            &payload,
            &rsa::pkcs1v15::Signature::try_from(signature.as_slice()).map_err(err)?,
        )
        .map_err(|_| "Invalid grant signature")?;
    let grant: Value = serde_json::from_slice(&payload).map_err(err)?;
    let expires = chrono::DateTime::parse_from_rfc3339(str(&grant, "ExpiresUtc")).map_err(err)?;
    if str(&grant, "Subject") != subject()?
        || str(&grant, "Role") != "admin"
        || expires <= chrono::Utc::now()
        || uuid::Uuid::parse_str(str(&grant, "Id")).is_err()
    {
        return Err("Grant expired or belongs to a different user or device".into());
    }
    Ok(grant)
}
pub fn handle(profile: &Path, p: &Value) -> Result<Value> {
    let action = str(p, "action");
    if action == "import" {
        let bytes = fs::read(str(p, "path")).map_err(err)?;
        verify(&bytes)?;
        atomic(&profile.join("admin-access.json"), &bytes)?;
        audit(profile, "Access", "Personal grant imported");
    }
    let grant = fs::read(profile.join("admin-access.json"))
        .ok()
        .and_then(|b| verify(&b).ok());
    if action == "audit" || action == "review" {
        if grant.is_none() {
            return Err("A personal administrator grant is required".into());
        }
        if action == "audit" {
            let mut entries = vec![];
            for name in ["previous.jsonl", "events.jsonl"] {
                if let Ok(text) = fs::read_to_string(profile.join("audit").join(name)) {
                    entries.extend(
                        text.lines()
                            .filter_map(|line| serde_json::from_str::<Value>(line).ok()),
                    );
                }
            }
            entries.reverse();
            entries.truncate(2000);
            return Ok(json!(entries));
        }
        let path = Path::new(str(p, "path"));
        let input = fs::File::open(path).map_err(err)?;
        let mut zip = zip::ZipArchive::new(input).map_err(err)?;
        if zip.len() > 30000 {
            return Err("Archive has too many entries".into());
        }
        let mut size = 0;
        let mut findings = vec![];
        let mut names = std::collections::HashSet::new();
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(err)?;
            let name = entry.name().replace('\\', "/");
            if entry.is_dir() {
                continue;
            }
            size += entry.size();
            if size > 4 * 1024 * 1024 * 1024 {
                return Err("Archive exceeds 4 GB".into());
            }
            safe(&profile.join("review-sandbox"), &name)?;
            if !names.insert(name.to_lowercase()) {
                return Err("Duplicate archive path".into());
            }
            if entry.unix_mode().is_some_and(|m| m & 0xf000 == 0xa000) {
                return Err("Linked archive entries are forbidden".into());
            }
            if matches!(
                Path::new(&name)
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_lowercase()
                    .as_str(),
                "exe" | "dll" | "bat" | "cmd" | "ps1" | "vbs"
            ) {
                findings.push(json!({"path":name,"kind":"executable"}));
            }
            if name.ends_with("manifest.json") && entry.size() < 1024 * 1024 {
                let mut text = String::new();
                entry.read_to_string(&mut text).map_err(err)?;
                if serde_json::from_str::<Value>(&text).is_err() {
                    findings.push(json!({"path":name,"kind":"invalid_manifest"}));
                }
            }
        }
        audit(profile, "Build review", "Mod archive inspected");
        return Ok(
            json!({"files":zip.len(),"bytes":size,"sha256":hash(path)?,"findings":findings,"installed":false}),
        );
    }
    Ok(json!({"subject":subject()?,"grant":grant}))
}
