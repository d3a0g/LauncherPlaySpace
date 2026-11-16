use crate::common::*;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::Read,
    path::Path,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};
static JOBS: LazyLock<Mutex<BTreeMap<String, Value>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));
pub fn items() -> Value {
    json!(JOBS
        .lock()
        .unwrap()
        .values()
        .rev()
        .cloned()
        .collect::<Vec<_>>())
}
pub fn cancel(id: &str) {
    if let Some(job) = JOBS.lock().unwrap().get_mut(id) {
        job["cancelled"] = json!(true);
    }
}
pub fn cancelled(id: &str) -> bool {
    JOBS.lock()
        .unwrap()
        .get(id)
        .is_some_and(|j| j["cancelled"] == true)
}
pub fn begin(title: &str) -> String {
    let id = id();
    let mut jobs = JOBS.lock().unwrap();
    jobs.insert(id.clone(),json!({"id":id,"title":title,"state":"running","received":0,"total":null,"speed":0,"detail":"","cancelled":false}));
    id
}
pub fn progress(id: &str, received: u64, total: Option<u64>, detail: &str) {
    if let Some(job) = JOBS.lock().unwrap().get_mut(id) {
        job["received"] = json!(received);
        job["total"] = json!(total);
        job["detail"] = json!(detail);
    }
}
pub fn finish(id: &str, result: &Result<Value>) {
    if let Some(job) = JOBS.lock().unwrap().get_mut(id) {
        match result {
            Ok(v) => {
                job["state"] = json!("completed");
                job["result"] = v.clone();
            }
            Err(e) => {
                job["state"] = json!(if e == "Cancelled" {
                    "cancelled"
                } else {
                    "failed"
                });
                job["detail"] = json!(e);
            }
        }
    }
}
fn allowed(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.host_str().is_some_and(|h| {
            h == "thunderstore.io"
                || h.ends_with(".thunderstore.io")
                || h.ends_with(".thunderstore.com")
        })
}
pub fn fetch(profile: &Path, job: &str, url: &str, max: u64) -> Result<Vec<u8>> {
    fetch_with(profile, job, url, max, false)
}
pub fn trusted(profile: &Path, job: &str, url: &str, max: u64) -> Result<Vec<u8>> {
    fetch_with(profile, job, url, max, true)
}
pub fn cloudflare(profile: &Path, job: &str) -> Result<Vec<u8>> {
    fetch_with(
        profile,
        job,
        "https://downloads.cloudflareclient.com/v1/download/windows/ga",
        180 * 1024 * 1024,
        true,
    )
}
fn fetch_with(profile: &Path, job: &str, url: &str, max: u64, upstream: bool) -> Result<Vec<u8>> {
    let cloudflare = url.starts_with("https://downloads.cloudflareclient.com/");
    let trusted = move |url: &reqwest::Url| {
        if cloudflare {
            return url.scheme() == "https"
                && url.username().is_empty()
                && url.port().is_none()
                && url.host_str() == Some("downloads.cloudflareclient.com");
        }
        allowed(url)
            || upstream
                && url.scheme() == "https"
                && url.username().is_empty()
                && url.host_str().is_some_and(|h| {
                    matches!(
                        h,
                        "raw.githubusercontent.com"
                            | "github.com"
                            | "objects.githubusercontent.com"
                            | "release-assets.githubusercontent.com"
                    )
                })
    };
    let url = reqwest::Url::parse(url).map_err(err)?;
    if !trusted(&url) {
        return Err("Package download must come from a trusted upstream".into());
    }
    let key = format!("{:X}", sha2::Sha256::digest(url.as_str().as_bytes()));
    let file = profile.join(format!("transfer-cache/{key}.bin"));
    let index = file.with_extension("json");
    if let Ok(info) = read(&index, Value::Null) {
        if info["size"].as_u64().is_some_and(|s| s <= max)
            && file.exists()
            && hash(&file).ok().is_some_and(|h| h == str(&info, "hash"))
        {
            return std::fs::read(&file).map_err(err);
        }
    }
    if prefs(profile)?["OfflineMode"] == true {
        return Err("Package is not cached offline".into());
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(180))
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 5 || !trusted(attempt.url()) {
                attempt.error("Untrusted package redirect")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(err)?;
    let mut response = client
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(err)?;
    let total = response.content_length();
    if total.is_some_and(|s| s > max) {
        return Err("Package exceeds size limit".into());
    }
    let limit = prefs(profile)?["DownloadLimitBytesPerSecond"]
        .as_u64()
        .unwrap_or(0);
    let start = Instant::now();
    let mut bytes = vec![];
    let mut chunk = [0u8; 65536];
    loop {
        if cancelled(job) {
            return Err("Cancelled".into());
        }
        let read = response.read(&mut chunk).map_err(err)?;
        if read == 0 {
            break;
        }
        if bytes.len() as u64 + read as u64 > max {
            return Err("Package exceeds size limit".into());
        }
        bytes.extend_from_slice(&chunk[..read]);
        progress(job, bytes.len() as u64, total, "Downloading package");
        if limit > 0 {
            let due = Duration::from_secs_f64(bytes.len() as f64 / limit as f64);
            while start.elapsed() < due {
                if cancelled(job) {
                    return Err("Cancelled".into());
                }
                std::thread::sleep(Duration::from_millis(30));
            }
        }
        if let Some(j) = JOBS.lock().unwrap().get_mut(job) {
            j["speed"] =
                json!((bytes.len() as f64 / start.elapsed().as_secs_f64().max(0.001)) as u64);
        }
    }
    atomic(&file, &bytes)?;
    write(&index, &json!({"hash":hash(&file)?,"size":bytes.len()}))?;
    Ok(bytes)
}
use sha2::Digest;
