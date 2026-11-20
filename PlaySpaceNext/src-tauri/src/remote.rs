//! Signed data only: no commands, code, installation paths, credentials or permissions.
use crate::common::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use rsa::{pkcs1v15::VerifyingKey, signature::Verifier, BigUint, RsaPublicKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::{HashMap, HashSet}, fs, io::Read, path::{Path, PathBuf}, sync::{Mutex, OnceLock}, time::{Duration, Instant}};

const ENDPOINT: &str = "https://raw.githubusercontent.com/d3a0g/playspace-releases/main/remote/stable/manifest.json";
const PUBLIC_KEY: &str = include_str!("../remote-public-key.xml");
const MAX_ENVELOPE: usize = 4 * 1024 * 1024;
const MAX_PAYLOAD: usize = 2 * 1024 * 1024;
const DOMAIN: &[u8] = b"PlaySpaceRemoteConfig/v1\n";
static STATES: OnceLock<Mutex<HashMap<PathBuf, State>>> = OnceLock::new();
struct State { payload: Option<Value>, digest: String, checked: Option<Instant>, refreshing: bool, error: Option<String>, source: &'static str }
fn states() -> &'static Mutex<HashMap<PathBuf, State>> { STATES.get_or_init(|| Mutex::new(HashMap::new())) }
fn cache(profile: &Path) -> PathBuf { profile.join("remote/verified-envelope.json") }
fn timestamp(value: &Value, field: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(str(value, field)).map(|v| v.with_timezone(&chrono::Utc)).map_err(err)
}
fn version(value: &str) -> Result<[u64; 3]> {
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != 3 { return Err("Expected a three-part client version".into()); }
    Ok([parts[0].parse().map_err(err)?, parts[1].parse().map_err(err)?, parts[2].parse().map_err(err)?])
}
fn public_key(xml: &str) -> Result<RsaPublicKey> {
    let part = |name: &str| -> Result<Vec<u8>> {
        let pattern = regex::Regex::new(&format!("<{name}>([^<]+)</{name}>")).map_err(err)?;
        STANDARD.decode(&pattern.captures(xml).ok_or("Missing public key component")?[1]).map_err(err)
    };
    RsaPublicKey::new(BigUint::from_bytes_be(&part("Modulus")?), BigUint::from_bytes_be(&part("Exponent")?)).map_err(err)
}
fn object_keys(value: &Value, keys: &[&str]) -> Result<()> {
    let object = value.as_object().ok_or("Expected a JSON object")?;
    if object.keys().any(|key| !keys.contains(&key.as_str())) { return Err("Unknown remote configuration field".into()); }
    Ok(())
}
fn text(value: &Value, max: usize) -> Result<()> {
    let value = value.as_str().ok_or("Expected text")?;
    if value.len() > max || value.chars().any(|c| c.is_control() && c != '\n' && c != '\t') { return Err("Invalid or oversized text".into()); }
    Ok(())
}
fn localized(value: &Value, max: usize) -> Result<()> {
    object_keys(value, &["ru", "en"])?;
    text(&value["ru"], max)?; text(&value["en"], max)
}
pub fn valid_link(id: &str, value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else { return false; };
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() || url.port().is_some() || url.fragment().is_some() { return false; }
    match id {
        "donate" => url.host_str() == Some("auth.robokassa.ru") && url.path().to_ascii_lowercase().starts_with("/merchant/invoice/") && url.query().is_none() && url.path().split('/').next_back().is_some_and(|s| !s.is_empty() && s.len() < 100 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')),
        "support" | "news" => url.host_str() == Some("t.me") && url.query().is_none() && url.path().trim_start_matches('/').chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && url.path().len() > 1,
        "offer" | "project" => url.host_str() == Some("github.com") && (url.path()=="/d3a0g/LauncherPlaySpace" || url.path().starts_with("/d3a0g/LauncherPlaySpace/")) && url.query().is_none(),
        _ => false,
    }
}
fn validate_config(config: &Value) -> Result<()> {
    object_keys(config, &["links", "features", "branding", "timing", "tools"])?;
    object_keys(&config["links"], &["donate", "support", "news", "offer", "project"])?;
    for (id, langs) in config["links"].as_object().ok_or("Missing project links")? {
        localized(langs, 512)?;
        for lang in ["ru", "en"] { if !valid_link(id, str(langs, lang)) { return Err(format!("Disallowed {id} URL")); } }
    }
    for id in ["donate", "support", "offer"] { if config["links"][id].is_null() { return Err("Required project link missing".into()); } }
    object_keys(&config["features"], &["donate", "announcements", "recommendations", "deals", "mods", "network", "sounds", "quotes", "controller", "graphics", "universal"])?;
    if config["features"].as_object().ok_or("Invalid feature switches")?.values().any(|v| !v.is_boolean()) { return Err("Feature switch must be boolean".into()); }
    object_keys(&config["branding"], &["accent", "tagline", "logoPng"])?;
    let accent = str(&config["branding"], "accent");
    if accent.len() != 7 || !accent.starts_with('#') || !accent[1..].bytes().all(|b| b.is_ascii_hexdigit()) { return Err("Invalid accent colour".into()); }
    localized(&config["branding"]["tagline"], 160)?;
    if !config["branding"]["logoPng"].is_null() {
        let bytes = STANDARD.decode(str(&config["branding"], "logoPng")).map_err(err)?;
        if bytes.len() > 256 * 1024 { return Err("Remote logo too large".into()); }
        let decoder = image::ImageReader::with_format(std::io::Cursor::new(&bytes), image::ImageFormat::Png);
        let (w,h) = decoder.into_dimensions().map_err(err)?;
        if w == 0 || h == 0 || w > 512 || h > 512 { return Err("Remote logo dimensions exceed limits".into()); }
        image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).map_err(err)?;
    }
    object_keys(&config["timing"], &["refreshSeconds", "recommendationsCacheSeconds", "workshopCacheSeconds"])?;
    for (key, min, max) in [("refreshSeconds",300,86400),("recommendationsCacheSeconds",300,86400),("workshopCacheSeconds",60,3600)] {
        let n = config["timing"][key].as_u64().ok_or("Missing timing parameter")?;
        if !(min..=max).contains(&n) { return Err("Timing outside safe range".into()); }
    }
    let tools = config["tools"].as_object().ok_or("Expected tool descriptions")?;
    let baseline: Value = serde_json::from_str(include_str!("../../src/features.json")).map_err(err)?;
    for (id, item) in tools {
        if !baseline.as_array().unwrap().iter().any(|t| str(t,"id") == id) { return Err("Unknown tool identifier".into()); }
        object_keys(item, &["title", "description"])?;
        localized(&item["title"], 160)?; localized(&item["description"], 800)?;
    }
    Ok(())
}
fn validate_catalogs(catalogs: &Value) -> Result<()> {
    object_keys(catalogs, &["recommendations", "modAdapters"])?;
    if !catalogs["recommendations"].is_null() {
        let rows = catalogs["recommendations"].as_array().ok_or("Invalid recommendations")?;
        if rows.len() > 5000 { return Err("Too many recommendations".into()); }
        let mut ids = HashSet::new();
        for row in rows {
            object_keys(row, &["id", "name", "categories"])?;
            if !str(row,"id").parse::<u32>().is_ok_and(|n| n>0) || !ids.insert(str(row,"id")) { return Err("Invalid or duplicate Steam ID".into()); }
            text(&row["name"],240)?;
            let categories = row["categories"].as_array().ok_or("Invalid categories")?;
            if categories.len() > 20 { return Err("Too many categories".into()); }
            for category in categories { text(category,100)?; }
        }
    }
    // Structural installation rules remain compiled in. Only existing adapter labels,
    // hints and availability can be updated remotely, never paths or executable rules.
    if !catalogs["modAdapters"].is_null() {
        object_keys(&catalogs["modAdapters"], &["thunderstore","native","paradox"])?;
        let baseline: Value = serde_json::from_str(include_str!("../catalog/mod-adapters.json")).map_err(err)?;
        for kind in ["thunderstore","native","paradox"] {
            let rows = catalogs["modAdapters"][kind].as_array().ok_or("Invalid adapter catalogue")?;
            let mut ids = HashSet::new();
            for row in rows {
                let identity = (str(row,"SteamId"),str(row,"Executable"));
                if !ids.insert(identity) { return Err("Duplicate adapter".into()); }
                let original = baseline[kind].as_array().unwrap().iter().find(|v| str(v,"SteamId")==identity.0 && str(v,"Executable")==identity.1).ok_or("New adapter rules require a client update")?;
                let mut structural = row.clone();
                let mut expected = original.clone();
                for key in ["Name","Hint"] {
                    if !row[key].is_null() { text(&row[key],800)?; }
                    structural.as_object_mut().ok_or("Invalid adapter")?.remove(key);
                    expected.as_object_mut().unwrap().remove(key);
                }
                if structural != expected { return Err("Remote adapter cannot change installation rules".into()); }
            }
        }
    }
    Ok(())
}
fn validate_payload(p: &Value, now: chrono::DateTime<chrono::Utc>, allow_expired: bool) -> Result<()> {
    object_keys(p, &["schemaVersion","revision","minClientVersion","maxClientVersion","issuedAt","expiresAt","config","announcements","translations","catalogs"])?;
    if p["schemaVersion"] != 1 || p["revision"].as_u64().unwrap_or(0) == 0 { return Err("Unsupported remote schema or revision".into()); }
    let client = version(env!("CARGO_PKG_VERSION"))?;
    if client < version(str(p,"minClientVersion"))? || (!p["maxClientVersion"].is_null() && client > version(str(p,"maxClientVersion"))?) { return Err("Configuration is incompatible with this client".into()); }
    let issued = timestamp(p,"issuedAt")?; let expires = timestamp(p,"expiresAt")?;
    if issued > now + chrono::Duration::minutes(5) || expires <= issued || expires-issued > chrono::Duration::days(90) || (!allow_expired && expires <= now) { return Err("Remote configuration expired or has invalid dates".into()); }
    validate_config(&p["config"])?;
    object_keys(&p["translations"], &["ru","en"])?;
    for lang in ["ru","en"] {
        let rows = p["translations"][lang].as_object().ok_or("Invalid translations")?;
        if rows.len()>5000 { return Err("Too many translations".into()); }
        for (key, value) in rows { if key.len()>1000 { return Err("Translation key too long".into()); } text(value,4000)?; }
    }
    let notices = p["announcements"].as_array().ok_or("Invalid announcements")?;
    if notices.len()>20 { return Err("Too many announcements".into()); }
    let mut ids = HashSet::new();
    for notice in notices {
        object_keys(notice, &["id","title","body","startsAt","endsAt","link"])?;
        text(&notice["id"],80)?;
        if str(notice,"id").is_empty() || !ids.insert(str(notice,"id")) { return Err("Duplicate announcement".into()); }
        localized(&notice["title"],160)?; localized(&notice["body"],2000)?;
        if timestamp(notice,"endsAt")? <= timestamp(notice,"startsAt")? { return Err("Invalid announcement schedule".into()); }
        if !notice["link"].is_null() && !["donate","support","news","offer","project"].contains(&str(notice,"link")) { return Err("Unknown announcement link".into()); }
    }
    validate_catalogs(&p["catalogs"])
}
fn verify_with_key(bytes: &[u8], key: &RsaPublicKey, now: chrono::DateTime<chrono::Utc>, allow_expired: bool) -> Result<(Value, String)> {
    if bytes.len()>MAX_ENVELOPE { return Err("Remote envelope exceeds limit".into()); }
    let envelope: Value = serde_json::from_slice(bytes).map_err(err)?;
    object_keys(&envelope, &["payload","signature"])?;
    let payload = STANDARD.decode(str(&envelope,"payload")).map_err(err)?;
    if payload.len()>MAX_PAYLOAD { return Err("Remote payload exceeds limit".into()); }
    let signature = STANDARD.decode(str(&envelope,"signature")).map_err(err)?;
    let mut signed = DOMAIN.to_vec(); signed.extend_from_slice(&payload);
    VerifyingKey::<Sha256>::new(key.clone()).verify(&signed, &rsa::pkcs1v15::Signature::try_from(signature.as_slice()).map_err(err)?).map_err(|_| "Invalid remote configuration signature")?;
    let value: Value = serde_json::from_slice(&payload).map_err(err)?;
    validate_payload(&value,now,allow_expired)?;
    Ok((value,format!("{:x}",Sha256::digest(&payload))))
}
fn verify(bytes: &[u8], allow_expired: bool) -> Result<(Value,String)> { verify_with_key(bytes,&public_key(PUBLIC_KEY)?,chrono::Utc::now(),allow_expired) }
fn initial(profile: &Path) -> State {
    let cached = (|| {
        let path = cache(profile); no_links(&path)?;
        if fs::metadata(&path).map_err(err)?.len()>MAX_ENVELOPE as u64 { return Err("Cache exceeds limit".into()); }
        verify(&fs::read(path).map_err(err)?,true)
    })().ok();
    let (payload,digest) = cached.map(|(p,h)|(Some(p),h)).unwrap_or((None,String::new()));
    State { payload,digest,checked:None,refreshing:false,error:None,source:"cache" }
}
fn usable(p: &Value) -> bool { timestamp(p,"expiresAt").is_ok_and(|t| t>chrono::Utc::now()) }
pub fn payload(profile: &Path) -> Option<Value> {
    let mut guard = states().lock().unwrap_or_else(|e| e.into_inner());
    guard.entry(profile.to_path_buf()).or_insert_with(||initial(profile)).payload.as_ref().filter(|p|usable(p)).cloned()
}
pub fn config(profile: &Path) -> Value { payload(profile).map(|p|p["config"].clone()).unwrap_or_else(||serde_json::from_str(include_str!("../../remote/config.json")).expect("embedded remote defaults")) }
pub fn enabled(profile: &Path, id: &str) -> bool { config(profile)["features"][id] != false }
pub fn seconds(profile: &Path, id: &str, default: u64) -> u64 { config(profile)["timing"][id].as_u64().unwrap_or(default) }
pub fn catalog(profile: &Path, id: &str) -> Option<Value> { payload(profile).and_then(|p|p["catalogs"].get(id).filter(|v|!v.is_null()).cloned()) }
pub fn tools(profile: &Path, base: Value) -> Value {
    let c=config(profile); let language=language(profile);
    Value::Array(base.as_array().unwrap_or(&vec![]).iter().filter(|t|c["features"][str(t,"id")]!=false).map(|t| {
        let mut t=t.clone(); let id=str(&t,"id").to_string();
        for field in ["title","description"] { if let Some(s)=c["tools"][&id][field][&language].as_str(){t[field]=json!(s);} }
        t
    }).collect())
}
pub fn link(profile: &Path, id: &str, lang: &str) -> Result<String> {
    if id=="donate" && !enabled(profile,"donate") { return Err("Voluntary support is temporarily unavailable".into()); }
    let c=config(profile); let url=str(&c["links"][id],if lang=="en"{"en"}else{"ru"});
    if !valid_link(id,url) { return Err("Unknown project link".into()); } Ok(url.to_string())
}
pub fn view(profile: &Path) -> Value {
    let p=payload(profile); let mut notices=p.as_ref().and_then(|p|p["announcements"].as_array()).cloned().unwrap_or_default();
    let now=chrono::Utc::now();
    notices.retain(|n|timestamp(n,"startsAt").is_ok_and(|t|t<=now)&&timestamp(n,"endsAt").is_ok_and(|t|t>now));
    let c=config(profile);
    if c["features"]["announcements"]==false {notices.clear();}
    let status = {
        let guard=states().lock().unwrap_or_else(|e|e.into_inner());
        let s=guard.get(profile).unwrap();
        json!({"source":if p.is_some(){s.source}else{"embedded"},"revision":p.as_ref().map(|p|p["revision"].clone()).unwrap_or(json!(0)),"lastAcceptedRevision":s.payload.as_ref().map(|p|p["revision"].clone()).unwrap_or(json!(0)),"expiresAt":p.as_ref().map(|p|p["expiresAt"].clone()),"lastError":s.error,"refreshing":s.refreshing,"endpoint":ENDPOINT})
    };
    json!({"config":c,"announcements":notices,"translations":p.as_ref().map(|p|p["translations"].clone()).unwrap_or(json!({"ru":{},"en":{}})),"status":status})
}
fn fetch() -> Result<Vec<u8>> {
    let client=reqwest::blocking::Client::builder().timeout(Duration::from_secs(10)).connect_timeout(Duration::from_secs(4)).redirect(reqwest::redirect::Policy::none()).user_agent("PlaySpace-Remote/1").build().map_err(err)?;
    let response=client.get(ENDPOINT).send().and_then(|r|r.error_for_status()).map_err(err)?;
    if response.status().is_redirection() || response.content_length().is_some_and(|n|n>MAX_ENVELOPE as u64) { return Err("Unexpected redirect or oversized remote configuration".into()); }
    let mut bytes=vec![];response.take(MAX_ENVELOPE as u64+1).read_to_end(&mut bytes).map_err(err)?;
    if bytes.len()>MAX_ENVELOPE {return Err("Remote response exceeds limit".into());} Ok(bytes)
}
fn check_revision(previous: u64, previous_digest: &str, revision: u64, digest: &str) -> Result<()> {
    if revision<previous || (revision==previous && digest!=previous_digest) {return Err("Remote revision rollback or revision reuse rejected".into());}
    Ok(())
}
fn accept(profile: &Path, bytes: &[u8]) -> Result<()> {
    let (p,digest)=verify(bytes,false)?;
    let mut guard=states().lock().unwrap_or_else(|e|e.into_inner());
    let s=guard.entry(profile.to_path_buf()).or_insert_with(||initial(profile));
    let revision=p["revision"].as_u64().unwrap();
    let floor=s.payload.as_ref().and_then(|v|v["revision"].as_u64()).unwrap_or(0);
    check_revision(floor,&s.digest,revision,&digest)?;
    atomic(&cache(profile),bytes)?;
    s.payload=Some(p);s.digest=digest;s.source="remote";s.error=None;Ok(())
}
pub fn refresh(profile: &Path, force: bool) -> Value {
    let interval=seconds(profile,"refreshSeconds",1800);
    let offline=prefs(profile).ok().is_some_and(|p|p["OfflineMode"]==true);
    let due={
        let mut guard=states().lock().unwrap_or_else(|e|e.into_inner());
        let s=guard.entry(profile.to_path_buf()).or_insert_with(||initial(profile));
        let due=!offline&&!s.refreshing&&s.checked.is_none_or(|t|t.elapsed()>=Duration::from_secs(if force{30}else{interval}));
        if due {s.checked=Some(Instant::now());s.refreshing=true;} due
    };
    if due {
        let result=fetch().and_then(|bytes|accept(profile,&bytes));
        let mut guard=states().lock().unwrap_or_else(|e|e.into_inner());
        if let Some(s)=guard.get_mut(profile){s.refreshing=false;s.error=result.err();}
    }
    view(profile)
}
pub fn handle(profile: &Path, p: &Value) -> Result<Value> {
    match str(p,"action") {
        "get"|"status"|"" => Ok(view(profile)),
        "refresh" => Ok(refresh(profile,true)),
        _ => Err("Unknown remote configuration action".into())
    }
}

pub fn verify_file(path: &Path) -> Result<Value> {
    no_links(path)?;
    if fs::metadata(path).map_err(err)?.len()>MAX_ENVELOPE as u64 {return Err("Remote envelope exceeds limit".into());}
    let (p,_) = verify(&fs::read(path).map_err(err)?,false)?;
    Ok(json!({"valid":true,"revision":p["revision"],"expiresAt":p["expiresAt"]}))
}

#[cfg(test)]
mod tests;
