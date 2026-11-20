//! Public metadata only. Subscription and downloads belong to the Steam client.
use crate::common::{self, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use scraper::{Html, Selector};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    io::{Cursor, Read},
    path::Path,
    time::{Duration, Instant},
};

const APP_ID: &str = "1167630";
const PAGE_SIZE: u64 = 30;
const MAX_HTML: u64 = 8 * 1024 * 1024;
const MAX_CACHE: u64 = 256 * 1024;
const CACHE_SECONDS: i64 = 300;

#[derive(Debug)]
struct Request {
    query: String,
    category: String,
    sort: String,
    page: u64,
    refresh: bool,
}

fn parameter(p: &Value, key: &str, max: usize) -> Result<String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(s)) if s.len() <= max && !s.chars().any(char::is_control) => {
            Ok(s.trim().to_owned())
        }
        _ => Err(format!(
            "Invalid Workshop {key} (maximum {max} UTF-8 bytes)"
        )),
    }
}

impl Request {
    fn new(app_id: &str, p: &Value) -> Result<Self> {
        if app_id != APP_ID {
            return Err("Workshop browsing currently supports only Teardown (1167630)".into());
        }
        if !p.is_object() {
            return Err("Workshop parameters must be an object".into());
        }
        let query = parameter(p, "query", 256)?;
        let mut category = parameter(p, "category", 64)?;
        let tag = parameter(p, "tag", 64)?;
        if !category.is_empty() && !tag.is_empty() && category != tag {
            return Err("Conflicting Workshop category and tag".into());
        }
        if category.is_empty() {
            category = tag;
        }
        let sort = match p.get("sort") {
            None => "trend",
            Some(Value::String(s)) if matches!(s.as_str(), "trend" | "mostrecent" | "toprated") => {
                s
            }
            _ => return Err("Invalid Workshop sort".into()),
        }
        .to_owned();
        let page = match p.get("page") {
            None => 1,
            Some(v) => v
                .as_u64()
                .filter(|n| (1..=1000).contains(n))
                .ok_or("Workshop page must be an integer from 1 to 1000")?,
        };
        let refresh = match p.get("refresh") {
            None => false,
            Some(Value::Bool(b)) => *b,
            _ => return Err("Workshop refresh must be a boolean".into()),
        };
        Ok(Self {
            query,
            category,
            sort,
            page,
            refresh,
        })
    }

    fn identity(&self) -> Value {
        json!([
            1,
            APP_ID,
            self.query,
            self.category,
            self.sort,
            self.page,
            PAGE_SIZE,
            "english"
        ])
    }

    fn key(&self) -> String {
        format!(
            "{:x}",
            Sha256::digest(self.identity().to_string().as_bytes())
        )
    }

    fn url(&self) -> reqwest::Url {
        let mut url = reqwest::Url::parse("https://steamcommunity.com/workshop/browse/").unwrap();
        {
            let mut pairs = url.query_pairs_mut();
            pairs
                .append_pair("appid", APP_ID)
                .append_pair("section", "readytouseitems")
                .append_pair("browsesort", &self.sort)
                .append_pair("p", &(self.page).to_string())
                .append_pair("numperpage", &PAGE_SIZE.to_string())
                .append_pair("l", "english")
                .append_pair("searchtext", &self.query);
            if !self.category.is_empty() {
                pairs.append_pair("requiredtags[]", &self.category);
            }
        }
        url
    }

    fn matches(&self, q: &Value) -> bool {
        let tags = q.get("required_tags").and_then(Value::as_array);
        q["appid"].as_u64() == Some(1167630)
            && q["page"].as_u64() == Some(self.page)
            && q["num_per_page"].as_u64() == Some(PAGE_SIZE)
            && q["browse_sort"].as_str() == Some(&self.sort)
            && q["search_text"].as_str() == Some(&self.query)
            && q["section"].as_str() == Some("readytouseitems")
            && if self.category.is_empty() {
                tags.is_none_or(|t| t.is_empty())
            } else {
                tags.is_some_and(|t| t.len() == 1 && t[0].as_str() == Some(&self.category))
            }
    }
}

fn text(v: &Value, max: usize) -> String {
    v.as_str()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control() || c.is_whitespace())
        .take(max)
        .collect()
}

fn numeric_id(v: &Value) -> Option<&str> {
    v.as_str().filter(|s| {
        !s.is_empty()
            && s.len() <= 20
            && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<u64>().is_ok_and(|n| n > 0)
    })
}

fn image_url(v: &Value) -> String {
    let raw = v.as_str().unwrap_or("");
    if raw.len() > 2048 {
        return String::new();
    }
    reqwest::Url::parse(raw)
        .ok()
        .filter(|u| {
            u.scheme() == "https"
                && u.username().is_empty()
                && u.password().is_none()
                && u.port().is_none()
                && u.host_str().is_some_and(|h| {
                    [
                        "steamusercontent.com",
                        "steamstatic.com",
                        "steamuserimages-a.akamaihd.net",
                    ]
                    .iter()
                    .any(|domain| h == *domain || h.ends_with(&format!(".{domain}")))
                })
        })
        .map(|u| u.to_string())
        .unwrap_or_default()
}

// Steam's current browse page nests dehydrated query JSON inside the SSR JSON.
fn parse_page(html: &str, request: &Request) -> Result<Value> {
    if html.len() as u64 > MAX_HTML {
        return Err("Workshop page exceeds the size limit".into());
    }
    let document = Html::parse_document(html);
    let selector = Selector::parse("script#valve-ssr-data[type='application/json']").unwrap();
    let script = document
        .select(&selector)
        .next()
        .ok_or("Steam Workshop page format is unavailable")?;
    let root: Value =
        serde_json::from_str(&script.text().collect::<String>()).map_err(common::err)?;
    let nested = &root["renderContext"]["queryData"];
    let decoded;
    let data = if let Some(s) = nested.as_str() {
        decoded = serde_json::from_str::<Value>(s).map_err(common::err)?;
        &decoded
    } else {
        nested
    };
    let queries = data["queries"]
        .as_array()
        .filter(|q| q.len() <= 2048)
        .ok_or("Steam Workshop query data is unavailable")?;
    let browse = queries
        .iter()
        .find(|q| q["queryKey"][0] == "workshop_browse" && request.matches(&q["queryKey"][1]))
        .ok_or("Steam returned a different Workshop query")?;
    let result = &browse["state"]["data"];
    if browse["state"]["status"] != "success"
        || result["eresult"].as_u64() != Some(1)
        || result["current_page"].as_u64() != Some(request.page)
    {
        return Err("Steam Workshop browse request failed".into());
    }
    let results = result["results"]
        .as_array()
        .filter(|r| r.len() <= PAGE_SIZE as usize)
        .ok_or("Invalid Steam Workshop results")?;
    let total = result["total_count"]
        .as_u64()
        .ok_or("Missing Steam Workshop result count")?;
    let pages = result["total_pages"]
        .as_u64()
        .ok_or("Missing Steam Workshop pagination")?;
    let creators = result["creator_player_link_details"].as_array();
    let mut ids = HashSet::new();
    let mut items = Vec::new();
    for item in results {
        let id = numeric_id(&item["publishedfileid"]).ok_or("Invalid Workshop item id")?;
        if item["consumer_appid"].as_u64() != Some(1167630) {
            return Err("Unexpected Workshop item app id".into());
        }
        let name = text(&item["title"], 512);
        if name.trim().is_empty() {
            return Err("Missing Workshop item title".into());
        }
        if !ids.insert(id) {
            continue;
        }
        let creator = numeric_id(&item["creator"]).unwrap_or("");
        let public = creators
            .and_then(|list| {
                list.iter()
                    .find(|c| c["public_data"]["steamid"].as_str() == Some(creator))
            })
            .map(|c| &c["public_data"])
            .or_else(|| {
                queries
                    .iter()
                    .find(|q| {
                        q["queryKey"][0] == "PlayerLinkDetails"
                            && q["queryKey"][1].as_str() == Some(creator)
                    })
                    .map(|q| &q["state"]["data"]["public_data"])
            });
        let owner = public
            .map(|v| text(&v["persona_name"], 128))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| creator.to_owned());
        items.push(json!({
            "id":id, "name":name, "owner":owner,
            "description":text(&item["short_description"], 4096),
            "image":image_url(&item["preview_url"]),
            "url":format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}")
        }));
    }
    Ok(
        json!({"items":items,"page":request.page,"hasMore":request.page < pages && !results.is_empty(),
        "source":"steam-workshop","cached":false,"total":total}),
    )
}

fn cached_page(envelope: &Value, request: &Request) -> Option<Value> {
    if envelope["request"] != request.identity() {
        return None;
    }
    let page = &envelope["data"];
    let items = page["items"].as_array()?;
    chrono::DateTime::parse_from_rfc3339(page["updatedAt"].as_str()?).ok()?;
    if items.len() > PAGE_SIZE as usize
        || page["source"] != "steam-workshop"
        || page["page"].as_u64() != Some(request.page)
        || !page["hasMore"].is_boolean()
        || !page["cached"].is_boolean()
        || !page["total"].is_u64()
    {
        return None;
    }
    let mut ids = HashSet::new();
    for item in items {
        let id = numeric_id(&item["id"])?;
        if !ids.insert(id)
            || item["url"].as_str()?
                != format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}")
        {
            return None;
        }
        for (key, max) in [("name", 512), ("owner", 128), ("description", 4096)] {
            if item[key].as_str()?.chars().count() > max {
                return None;
            }
        }
        let image = item["image"].as_str()?;
        if !image.is_empty() && image_url(&item["image"]) != image {
            return None;
        }
    }
    let mut page = page.clone();
    page["cached"] = json!(true);
    Some(page)
}

fn fresh_for(page: &Value, seconds: i64) -> bool {
    page["updatedAt"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .is_some_and(|t| {
            let age = chrono::Utc::now().signed_duration_since(t).num_seconds();
            (0..seconds).contains(&age)
        })
}

const MAX_PREVIEW_DOWNLOAD: u64 = 2 * 1024 * 1024;
const MAX_PREVIEW_CACHE: u64 = 128 * 1024;

fn thumbnail(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(common::err)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(common::err)?
        .thumbnail(320, 180)
        .to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 75)
        .encode_image(&image)
        .map_err(common::err)?;
    if out.len() as u64 > MAX_PREVIEW_CACHE {
        return Err("Workshop thumbnail exceeds size limit".into());
    }
    Ok(out)
}

fn preview(
    profile: &Path,
    url: &str,
    client: Option<&reqwest::blocking::Client>,
    deadline: Instant,
) -> String {
    if url.is_empty() || image_url(&json!(url)).is_empty() {
        return String::new();
    }
    let key = format!("{:x}", Sha256::digest(url.as_bytes()));
    let path = profile
        .join("cache/steam-workshop-previews-v1")
        .join(format!("{key}.jpg"));
    let cached = (|| {
        common::no_links(&path)?;
        let file = std::fs::File::open(&path).map_err(common::err)?;
        let mut bytes = Vec::new();
        file.take(MAX_PREVIEW_CACHE + 1)
            .read_to_end(&mut bytes)
            .map_err(common::err)?;
        if bytes.len() as u64 > MAX_PREVIEW_CACHE {
            return Err("Oversized thumbnail".into());
        }
        thumbnail(&bytes)
    })();
    let bytes: Result<Vec<u8>> = cached.or_else(|_: String| {
        let client = client.ok_or("Workshop preview unavailable offline")?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("Workshop preview deadline")?;
        let response = client
            .get(url)
            .timeout(remaining.min(Duration::from_secs(2)))
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(common::err)?;
        if image_url(&json!(response.url().as_str())).is_empty()
            || response
                .content_length()
                .is_some_and(|n| n > MAX_PREVIEW_DOWNLOAD)
        {
            return Err("Unsupported Workshop preview response".into());
        }
        let mut bytes = Vec::new();
        response
            .take(MAX_PREVIEW_DOWNLOAD + 1)
            .read_to_end(&mut bytes)
            .map_err(common::err)?;
        if bytes.len() as u64 > MAX_PREVIEW_DOWNLOAD {
            return Err("Oversized Workshop preview".into());
        }
        let bytes = thumbnail(&bytes)?;
        let _ = common::atomic(&path, &bytes);
        Ok(bytes)
    });
    bytes
        .map(|b| format!("data:image/jpeg;base64,{}", STANDARD.encode(b)))
        .unwrap_or_default()
}

// Metadata caches retain source URLs; only data URIs (or empty placeholders)
// cross IPC, keeping the existing img-src CSP. Six workers, 30 previews, 8s budget.
fn with_previews(profile: &Path, mut page: Value, offline: bool) -> Value {
    let urls: Vec<String> = page["items"]
        .as_array()
        .into_iter()
        .flatten()
        .take(PAGE_SIZE as usize)
        .map(|item| item["image"].as_str().unwrap_or("").to_owned())
        .collect();
    let deadline = Instant::now() + Duration::from_secs(8);
    // Redirect targets must pass the same Steam CDN whitelist before fetching.
    let client = if offline {
        None
    } else {
        reqwest::blocking::Client::builder()
            .user_agent("PlaySpaceNext/0.2")
            .timeout(Duration::from_secs(2))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 3
                    || image_url(&json!(attempt.url().as_str())).is_empty()
                {
                    attempt.error("Unsupported Workshop preview redirect")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .ok()
    };
    let images = std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for chunk in urls.chunks(5) {
            let client = client.as_ref();
            workers.push(scope.spawn(move || {
                chunk
                    .iter()
                    .map(|url| preview(profile, url, client, deadline))
                    .collect::<Vec<_>>()
            }));
        }
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap_or_default())
            .collect::<Vec<_>>()
    });
    if let Some(items) = page["items"].as_array_mut() {
        for (index, item) in items.iter_mut().enumerate() {
            item["image"] = json!(images.get(index).cloned().unwrap_or_default());
        }
    }
    page
}

/// Blocking metadata fetch; call from the IPC blocking worker. `category` (or
/// `tag`) is one required Steam tag. Page is one-based, 1..=1000, 30 items/page.
/// Offline and failed refreshes may return stale cache; `updatedAt` is RFC3339.
pub fn catalog(profile: &Path, app_id: &str, p: &Value) -> Result<Value> {
    let request = Request::new(app_id, p)?;
    let path = profile
        .join("cache/steam-workshop-v1")
        .join(format!("{}.json", request.key()));
    let cached = if common::no_links(&path).is_ok()
        && std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= MAX_CACHE)
    {
        common::read(&path, Value::Null)
            .ok()
            .and_then(|v| cached_page(&v, &request))
    } else {
        None
    };
    let offline = common::prefs(profile)?["OfflineMode"]
        .as_bool()
        .unwrap_or(false);
    if offline {
        return cached
            .map(|page| with_previews(profile, page, true))
            .ok_or("No cached Workshop results for this query/page in offline mode".into());
    }
    if !request.refresh {
        if let Some(page) = cached.as_ref().filter(|v| fresh_for(v,crate::remote::seconds(profile,"workshopCacheSeconds",CACHE_SECONDS as u64) as i64)) {
            return Ok(with_previews(profile, page.clone(), false));
        }
    }
    let live: Result<Value> = (|| {
        let bytes = common::http_bytes(request.url().as_str(), MAX_HTML)?;
        let html = std::str::from_utf8(&bytes).map_err(common::err)?;
        let mut page = parse_page(html, &request)?;
        page["updatedAt"] = json!(common::now());
        let bytes = serde_json::to_vec(&json!({"request":request.identity(),"data":page}))
            .map_err(common::err)?;
        if bytes.len() as u64 <= MAX_CACHE {
            // Browsing remains usable when the profile cache is unwritable.
            let _ = common::atomic(&path, &bytes);
        }
        Ok(page)
    })();
    match live {
        Ok(page) => Ok(with_previews(profile, page, false)),
        Err(error) => cached
            .map(|page| with_previews(profile, page, true))
            .ok_or_else(|| format!("Steam Workshop unavailable: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(r: &Request, items: Value, total: u64, pages: u64) -> String {
        let q = json!({"queries":[{"queryKey":["workshop_browse",{
            "appid":1167630,"page":r.page,"num_per_page":30,"browse_sort":r.sort,
            "search_text":r.query,"section":"readytouseitems",
            "required_tags":if r.category.is_empty(){vec![]}else{vec![r.category.clone()]}
        },1],"state":{"status":"success","data":{"eresult":1,"current_page":r.page,
            "total_count":total,"total_pages":pages,"results":items,
            "creator_player_link_details":[{"public_data":{"steamid":"76561198000000000","persona_name":"Alice"}}]}}}]});
        format!(
            "<html><script type='application/json' id='valve-ssr-data'>{}</script></html>",
            json!({"renderContext":{"queryData":q.to_string()}})
        )
    }

    fn item() -> Value {
        json!({"publishedfileid":"1234567890","consumer_appid":1167630,
            "title":"A & B <map>","creator":"76561198000000000","short_description":"Public summary",
            "preview_url":"https://images.steamusercontent.com/ugc/123/abc/"})
    }

    #[test]
    fn validates_parameters_and_whitelist() {
        assert!(Request::new("../1167630", &json!({})).is_err());
        for p in [
            json!([]),
            json!({"page":-1}),
            json!({"page":0}),
            json!({"page":1001}),
            json!({"page":0.5}),
            json!({"sort":"popular"}),
            json!({"refresh":"true"}),
            json!({"query":"x".repeat(257)}),
            json!({"tag":"x".repeat(65)}),
            json!({"query":"a\nb"}),
            json!({"query":123}),
            json!({"tag":"Map","category":"Vehicle"}),
        ] {
            assert!(Request::new(APP_ID, &p).is_err(), "{p}");
        }
        assert_eq!(
            Request::new(APP_ID, &json!({"page":999})).unwrap().page,
            999
        );
    }

    #[test]
    fn url_encodes_input_and_uses_one_based_pages() {
        let r = Request::new(
            APP_ID,
            &json!({"query":"a&appid=1 / мир", "tag":"Map & Vehicle", "page":2}),
        )
        .unwrap();
        let pairs: Vec<_> = r
            .url()
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert_eq!(pairs.iter().filter(|(k, _)| k == "appid").count(), 1);
        assert!(pairs.contains(&("appid".into(), APP_ID.into())));
        assert!(pairs.contains(&("p".into(), "2".into())));
        assert!(pairs.contains(&("searchtext".into(), r.query.clone())));
        assert!(pairs.contains(&("requiredtags[]".into(), r.category.clone())));
    }

    #[test]
    fn cache_identity_includes_filters_but_not_refresh() {
        let base = Request::new(APP_ID, &json!({})).unwrap().key();
        assert_eq!(
            base,
            Request::new(APP_ID, &json!({"refresh":true}))
                .unwrap()
                .key()
        );
        for p in [
            json!({"query":"map"}),
            json!({"page":2}),
            json!({"sort":"mostrecent"}),
            json!({"tag":"Map"}),
        ] {
            let key = Request::new(APP_ID, &p).unwrap().key();
            assert_ne!(base, key);
            assert_eq!(key.len(), 64);
            assert!(key.bytes().all(|b| b.is_ascii_hexdigit()));
        }
        assert_eq!(
            Request::new(APP_ID, &json!({"tag":" Map "})).unwrap().key(),
            Request::new(APP_ID, &json!({"category":"Map"}))
                .unwrap()
                .key()
        );
    }

    #[test]
    fn parses_nested_ssr_summaries_and_pagination() {
        let r = Request::new(
            APP_ID,
            &json!({"query":"car","category":"Vehicle","page":2,"sort":"toprated"}),
        )
        .unwrap();
        let page = parse_page(&fixture(&r, json!([item(), item()]), 61, 3), &r).unwrap();
        assert_eq!(page["items"].as_array().unwrap().len(), 1);
        assert_eq!(page["items"][0]["owner"], "Alice");
        assert_eq!(page["items"][0]["name"], "A & B <map>");
        assert_eq!(page["items"][0]["description"], "Public summary");
        assert_eq!(page["page"], 2);
        assert_eq!(page["total"], 61);
        assert_eq!(page["hasMore"], true);
        assert_eq!(
            parse_page(&fixture(&r, json!([item()]), 31, 2), &r).unwrap()["hasMore"],
            false
        );
        assert!(parse_page(
            &fixture(&r, json!([item()]), 61, 3),
            &Request::new(APP_ID, &json!({})).unwrap()
        )
        .is_err());
    }

    #[test]
    fn empty_results_are_valid_but_unknown_or_corrupt_pages_are_not() {
        let r = Request::new(APP_ID, &json!({})).unwrap();
        let empty = parse_page(&fixture(&r, json!([]), 0, 0), &r).unwrap();
        assert_eq!(empty["items"], json!([]));
        assert_eq!(empty["hasMore"], false);
        for html in [
            "<html>Sign in</html>",
            "<script id='valve-ssr-data' type='application/json'>{bad}</script>",
        ] {
            assert!(parse_page(html, &r).is_err());
        }
        let mut bad = item();
        bad["consumer_appid"] = json!(480);
        assert!(parse_page(&fixture(&r, json!([bad]), 1, 1), &r).is_err());
        let mut bad = item();
        bad["publishedfileid"] = json!("1&url=evil");
        assert!(parse_page(&fixture(&r, json!([bad]), 1, 1), &r).is_err());
        assert!(parse_page(&"x".repeat(MAX_HTML as usize + 1), &r).is_err());
    }

    #[test]
    fn cached_pages_require_matching_identity_and_safe_links() {
        let r = Request::new(APP_ID, &json!({})).unwrap();
        let mut page = parse_page(&fixture(&r, json!([item()]), 1, 1), &r).unwrap();
        page["updatedAt"] = json!("2020-01-01T00:00:00Z");
        let envelope = json!({"request":r.identity(),"data":page});
        assert_eq!(cached_page(&envelope, &r).unwrap()["cached"], true);
        assert!(!fresh_for(&page,CACHE_SECONDS));
        assert!(cached_page(
            &envelope,
            &Request::new(APP_ID, &json!({"query":"different"})).unwrap()
        )
        .is_none());
        let mut bad = envelope.clone();
        bad["data"]["items"][0]["url"] = json!("file:///C:/test");
        assert!(cached_page(&bad, &r).is_none());
        let mut bad = envelope;
        bad["data"]["updatedAt"] = json!("invalid");
        assert!(cached_page(&bad, &r).is_none());
        page["updatedAt"] = json!(common::now());
        assert!(fresh_for(&page,CACHE_SECONDS));
    }

    #[test]
    fn images_allow_only_https_steam_hosts() {
        for url in [
            "javascript:alert(1)",
            "file:///C:/test",
            "http://images.steamusercontent.com/a",
            "https://steamusercontent.com.evil.test/a",
            "https://evilsteamusercontent.com/a",
            "https://user@images.steamusercontent.com/a",
            "https://images.steamusercontent.com:444/a",
        ] {
            assert!(image_url(&json!(url)).is_empty(), "{url}");
        }
        assert!(!image_url(&item()["preview_url"]).is_empty());
    }

    #[test]
    fn thumbnails_are_small_jpegs_and_reject_invalid_bytes() {
        let image = image::DynamicImage::new_rgb8(640, 480);
        let mut png = Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let bytes = thumbnail(png.get_ref()).unwrap();
        assert!(bytes.len() as u64 <= MAX_PREVIEW_CACHE);
        assert_eq!(
            image::guess_format(&bytes).unwrap(),
            image::ImageFormat::Jpeg
        );
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert!(decoded.width() <= 320 && decoded.height() <= 180);
        assert!(thumbnail(b"not an image").is_err());
    }

    #[test]
    #[ignore = "uses live public Steam endpoints; no profile or cache writes"]
    fn live_public_workshop_smoke() {
        for params in [
            json!({}),
            json!({"sort":"mostrecent"}),
            json!({"sort":"toprated","page":2,"query":"car","category":"Vehicle"}),
        ] {
            let request = Request::new(APP_ID, &params).unwrap();
            let bytes = common::http_bytes(request.url().as_str(), MAX_HTML).unwrap();
            let page = parse_page(std::str::from_utf8(&bytes).unwrap(), &request).unwrap();
            let items = page["items"].as_array().unwrap();
            assert!(!items.is_empty());
            assert!(items.len() <= PAGE_SIZE as usize);
            let url = items[0]["image"].as_str().unwrap();
            let preview = common::http_bytes(url, MAX_PREVIEW_DOWNLOAD).unwrap();
            let jpeg = thumbnail(&preview).unwrap();
            println!(
                "sort={} page={} items={} total={} first={} thumbnail={} bytes",
                request.sort,
                request.page,
                items.len(),
                page["total"],
                items[0]["id"],
                jpeg.len()
            );
        }
    }
}
