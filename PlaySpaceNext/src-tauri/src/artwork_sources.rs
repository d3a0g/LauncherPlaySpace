use crate::common::*;
use serde_json::{json, Value};
use std::{fs, io::Read, path::Path, time::{Duration, Instant}};
pub fn normalized(name: &str) -> String {
    name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}
pub fn exact_id(items: &[Value], name: &str) -> Option<String> {
    let key=normalized(name);
    if key.is_empty(){return None}
    let matches: Vec<String>=items.iter().filter(|item| normalized(str(item,"name"))==key)
        .filter_map(|item| item["id"].as_str().map(String::from).or_else(||item["id"].as_u64().map(|id|id.to_string())))
        .collect();
    let unique: std::collections::HashSet<_>=matches.into_iter().collect();
    if unique.len()==1 { unique.into_iter().next() } else {None}
}
pub fn resolve(profile: &Path, name: &str, offline: bool) -> Option<String> {
    use sha2::{Digest,Sha256};
    let key=normalized(name);
    if key.is_empty()||name.len()>240{return None}
    let cache=profile.join(format!("artwork-matches/{:x}.json",Sha256::digest(key.as_bytes())));
    if let Ok(value)=read(&cache,json!({})) {
        if value["name"].as_str()==Some(key.as_str()) {
            if let Some(id)=value["id"].as_str(){if id.parse::<u32>().is_ok_and(|n|n>0){return Some(id.into())}}
        }
    }
    let base:Vec<Value>=serde_json::from_value(crate::remote::catalog(profile,"recommendations").unwrap_or_else(||serde_json::from_str(include_str!("../catalog/recommendations.json")).expect("embedded recommendations"))).ok()?;
    let mut found=exact_id(&base,name);
    if found.is_none()&&!offline {
        let result=http().ok()?.get("https://store.steampowered.com/api/storesearch/")
            .query(&[("term",name),("l","english"),("cc","us")]).timeout(Duration::from_secs(5)).send().ok()?;
        let mut bytes=vec![];
        result.error_for_status().ok()?.take(1024*1024).read_to_end(&mut bytes).ok()?;
        let value:Value=serde_json::from_slice(&bytes).ok()?;
        found=exact_id(value["items"].as_array()?,name);
    }
    if let Some(id)=&found {let _=write(&cache,&json!({"name":key,"id":id}));}
    found
}
pub fn cached_steam(id: &str, portrait: bool, hero: bool) -> Option<Vec<u8>> {
    let root=crate::discovery::steam()?.join("appcache/librarycache");
    let asset=if hero {"library_hero"} else if portrait {"library_600x900"} else {"header"};
    let mut dirs=vec![root.clone(),root.join(id)];
    if let Ok(entries)=fs::read_dir(root.join(id)) {dirs.extend(entries.flatten().take(30).filter(|e|e.file_type().is_ok_and(|f|f.is_dir()&&!f.is_symlink())).map(|e|e.path()));}
    for dir in dirs {
        for entry in fs::read_dir(dir).ok().into_iter().flatten().flatten().take(1000) {
            let name=entry.file_name().to_string_lossy().to_lowercase();
            if !(name.contains(asset)&&(entry.path().parent().is_some_and(|p|p!=root)||name.starts_with(&format!("{id}_")))) {continue}
            if entry.file_type().is_ok_and(|f|f.is_file()) && entry.metadata().is_ok_and(|m|m.len()<10*1024*1024) && no_links(&entry.path()).is_ok() {
                if let Ok(bytes)=fs::read(entry.path()){if image::load_from_memory(&bytes).is_ok(){return Some(bytes)}}
            }
        }
    }
    None
}
// Artwork requests have a shared deadline so a missing CDN asset cannot hold the queue indefinitely.
pub fn fetch(url: &str, deadline: Instant, max: u64) -> Result<Vec<u8>> {
    let remaining=deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero(){return Err("Artwork request timed out".into())}
    let response=http()?.get(url).timeout(remaining.min(Duration::from_secs(4))).send().and_then(|r|r.error_for_status()).map_err(err)?;
    if response.content_length().is_some_and(|n|n>max){return Err("Artwork too large".into())}
    let mut bytes=vec![]; response.take(max+1).read_to_end(&mut bytes).map_err(err)?;
    if bytes.len() as u64>max{return Err("Artwork too large".into())} Ok(bytes)
}
pub fn riot(deadline: Instant) -> Option<Vec<u8>> {
    let html=String::from_utf8(fetch("https://playvalorant.com/en-us/",deadline,4*1024*1024).ok()?).ok()?;
    let document=scraper::Html::parse_document(&html);
    let selector=scraper::Selector::parse("meta[property='og:image']").ok()?;
    let mut url=document.select(&selector).next()?.value().attr("content")?.to_owned();
    // Prefer the official agents artwork to the site's small favicon used in og:image.
    if let Ok(selector)=scraper::Selector::parse("script#__NEXT_DATA__") {
        if let Some(script)=document.select(&selector).next() {
            if let Ok(value)=serde_json::from_str::<Value>(&script.inner_html()) {
                if let Some(blades)=value.pointer("/props/pageProps/page/blades").and_then(Value::as_array) {
                    for blade in blades {
                        let media=&blade["item"]["media"];
                        if media["dimensions"]["width"].as_u64().unwrap_or(0)>=800 && media["dimensions"]["height"].as_u64().unwrap_or(0)>=800 {
                            if let Some(candidate)=media["url"].as_str(){url=candidate.to_owned();break}
                        }
                    }
                }
            }
        }
    }
    let uri=reqwest::Url::parse(&url).ok()?;
    if uri.scheme()!="https" || !matches!(uri.host_str()?,"cmsassets.rgpub.io"|"images.contentstack.io"|"playvalorant.com"){return None}
    fetch(&url,deadline,6*1024*1024).ok()
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn exact_titles_never_pick_sequels_or_ambiguous_games(){
        let games=vec![json!({"id":1,"name":"PEAK"}),json!({"id":2,"name":"PEAK 2"})];
        assert_eq!(exact_id(&games,"Peak™"),Some("1".into()));
        assert_eq!(exact_id(&games,"Peak demo"),None);
        assert_eq!(exact_id(&[json!({"id":1,"name":"Peak"}),json!({"id":2,"name":"Peak"})],"Peak"),None);
    }
}
