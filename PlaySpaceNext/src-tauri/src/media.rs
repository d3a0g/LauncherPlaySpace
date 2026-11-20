use crate::common::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use scraper::{Html, Selector};
use serde_json::{json, Value};
use std::{collections::HashSet, fs, io::Cursor, path::Path};
pub fn handles(op: &str) -> bool {
    matches!(
        op,
        "artwork" | "recommendations" | "store_details" | "deals"
    )
}
fn valid(id: &str) -> bool {
    id.parse::<u32>().is_ok_and(|v| v > 0)
}
fn image(bytes: &[u8]) -> Result<image::DynamicImage> {
    let (w, h) = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(err)?
        .into_dimensions()
        .map_err(err)?;
    if w < 80 || h < 80 || u64::from(w) * u64::from(h) > 25_000_000 {
        return Err("Invalid image dimensions".into());
    }
    image::load_from_memory(bytes).map_err(err)
}
fn art(profile: &Path, p: &Value) -> Result<Value> {
    if !str(p, "url").is_empty() {
        let url = reqwest::Url::parse(str(p, "url")).map_err(err)?;
        if url.scheme() != "https"
            || !url.username().is_empty()
            || !url.host_str().is_some_and(|h| {
                matches!(
                    h,
                    "cdn1.epicgames.com" | "cdn2.epicgames.com" | "cdn.unrealengine.com"
                )
            })
        {
            return Err("Unsupported store artwork URL".into());
        }
        use sha2::{Digest, Sha256};
        let key = format!("{:X}", Sha256::digest(url.as_str().as_bytes()));
        let cache = profile.join(format!("epic-posters/{key}.jpg"));
        let bytes = if cache.exists() {
            fs::read(&cache).map_err(err)?
        } else {
            if prefs(profile)?["OfflineMode"] == true {
                return Ok(Value::Null);
            }
            http_bytes(url.as_str(), 6 * 1024 * 1024)?
        };
        let img = image(&bytes)?.resize(920, 500, image::imageops::FilterType::Lanczos3);
        let mut output = Cursor::new(vec![]);
        img.write_to(&mut output, image::ImageFormat::Jpeg)
            .map_err(err)?;
        let bytes = output.into_inner();
        let _ = atomic(&cache, &bytes);
        return Ok(json!({"url":format!("data:image/jpeg;base64,{}",STANDARD.encode(bytes))}));
    }
    if !str(p, "gameId").is_empty() {
        let g = game(profile, &json!({"id":p["gameId"]}))?;
        let path = crate::library::text(&g, "CustomArtworkPath");
        if !path.is_empty() {
            let path = Path::new(&path);
            no_links(path)?;
            if fs::metadata(path).map_err(err)?.len() > 10 * 1024 * 1024 {
                return Err("Custom artwork exceeds 10 MB".into());
            }
            let img = image(&fs::read(path).map_err(err)?)?.resize(
                1920,
                1080,
                image::imageops::FilterType::Lanczos3,
            );
            let mut output = Cursor::new(vec![]);
            img.write_to(&mut output, image::ImageFormat::Jpeg)
                .map_err(err)?;
            return Ok(
                json!({"url":format!("data:image/jpeg;base64,{}",STANDARD.encode(output.into_inner()))}),
            );
        }
    }
    let offline = prefs(profile)?["OfflineMode"] == true;
    let name = if !str(p,"gameId").is_empty() { crate::library::text(&game(profile,&json!({"id":p["gameId"]}))?,"Name") } else {str(p,"name").into()};
    let resolved = if valid(str(p,"id")) {str(p,"id").to_owned()} else {crate::artwork_sources::resolve(profile,&name,offline).unwrap_or_default()};
    let id = resolved.as_str();
    let riot = crate::artwork_sources::normalized(&name)=="valorant";
    if !valid(id)&&!riot { return Ok(Value::Null); }
    let cache_id=if riot {"riot-valorant-agents"} else {id};
    let portrait = p["portrait"] == true;
    let hero = p["hero"] == true;
    let folder = profile.join(if hero {
        "hero-posters"
    } else if portrait {
        "posters"
    } else {
        "wide-posters"
    });
    let cached = folder.join(format!("{cache_id}.jpg"));
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(16);
    let mut picture = fs::read(&cached)
        .ok()
        .filter(|b| b.len() < 10 * 1024 * 1024)
        .and_then(|b| image(&b).ok());
    if picture.is_none() && valid(id) {
        picture=crate::artwork_sources::cached_steam(id,portrait,hero).and_then(|b|image(&b).ok());
    }
    if picture.is_none()&&riot&&!offline {
        picture=crate::artwork_sources::riot(deadline).and_then(|b|image(&b).ok());
    }
    if picture.is_none() && !offline && valid(id) {
        let assets: Vec<&str> = if hero {
            vec!["library_hero.jpg", "header.jpg"]
        } else if portrait {
            vec![
                "library_600x900_2x.jpg",
                "library_600x900.jpg",
                "library_capsule.jpg",
                "header.jpg",
            ]
        } else {
            vec!["header.jpg", "capsule_616x353.jpg"]
        };
        'assets: for asset in assets {
            for host in [
                "shared.fastly.steamstatic.com/store_item_assets",
                "cdn.akamai.steamstatic.com",
            ] {
                if let Ok(bytes) = crate::artwork_sources::fetch(
                    &format!("https://{host}/steam/apps/{id}/{asset}"),
                    (deadline-std::time::Duration::from_secs(6)).min(std::time::Instant::now()+std::time::Duration::from_secs(2)),
                    6 * 1024 * 1024,
                ) {
                    if let Ok(img) = image(&bytes) {
                        picture = Some(img);
                        break 'assets;
                    }
                }
            }
        }
        if picture.is_none() {
            if let Ok(bytes) = crate::artwork_sources::fetch(
                &format!("https://store.steampowered.com/api/appdetails?appids={id}"),
                deadline,
                4 * 1024 * 1024,
            ) {
                if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                    for key in ["header_image", "capsule_image", "capsule_imagev5"] {
                        if let Some(url) = v[id]["data"][key].as_str() {
                            if let Ok(uri) = reqwest::Url::parse(url) {
                                if uri.scheme() == "https"
                                    && uri
                                        .host_str()
                                        .is_some_and(|h| h.ends_with(".steamstatic.com"))
                                {
                                    if let Ok(bytes) = crate::artwork_sources::fetch(url, deadline, 6 * 1024 * 1024) {
                                        if let Ok(img) = image(&bytes) {
                                            picture = Some(img);
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // Existing wide cache also works offline when a game has no poster asset.
    if picture.is_none() && portrait {
        picture = fs::read(profile.join(format!("wide-posters/{id}.jpg")))
            .ok()
            .and_then(|b| image(&b).ok());
    }
    let Some(mut img) = picture else {
        return Ok(Value::Null);
    };
    if portrait && img.width() as f64 / img.height() as f64 > 0.9 {
        let mut background = img
            .resize_to_fill(600, 900, image::imageops::FilterType::Triangle)
            .blur(22.)
            .brighten(-65);
        let foreground = img.resize(600, 600, image::imageops::FilterType::Lanczos3);
        image::imageops::overlay(
            &mut background,
            &foreground,
            ((600 - foreground.width()) / 2) as i64,
            ((900 - foreground.height()) / 2) as i64,
        );
        img = background;
    } else {
        img = img.resize(
            if hero {
                1920
            } else if portrait {
                600
            } else {
                920
            },
            if hero {
                1080
            } else if portrait {
                900
            } else {
                500
            },
            image::imageops::FilterType::Lanczos3,
        );
    }
    let mut output = Cursor::new(vec![]);
    img.write_to(&mut output, image::ImageFormat::Jpeg)
        .map_err(err)?;
    let bytes = output.into_inner();
    if !cached.exists()
        || fs::read(&cached)
            .ok()
            .and_then(|b| image(&b).ok())
            .is_none()
    {
        let _ = atomic(&cached, &bytes);
    }
    Ok(json!({"url":format!("data:image/jpeg;base64,{}",STANDARD.encode(bytes))}))
}
fn selector(value: &str) -> Selector {
    Selector::parse(value).unwrap()
}
const TAGS: [(&str, &str, &str); 14] = [
    ("1663", "Шутеры", "Shooters"),
    ("1662", "Выживание", "Survival"),
    ("1667", "Хорроры", "Horror"),
    ("1664", "Головоломки", "Puzzle"),
    ("9", "Стратегии", "Strategy"),
    ("122", "Ролевые", "RPG"),
    ("21", "Приключения", "Adventure"),
    ("599", "Симуляторы", "Simulation"),
    ("699", "Гонки", "Racing"),
    ("701", "Спорт", "Sports"),
    ("492", "Инди", "Indie"),
    ("1685", "Кооп", "Co-op"),
    ("19", "Экшен", "Action"),
    ("1695", "Песочницы", "Sandbox"),
];
fn search_rows(html: &str) -> Vec<Value> {
    let document = Html::parse_fragment(html);
    document.select(&selector("a[data-ds-appid]")).filter_map(|row|{
        let id=row.value().attr("data-ds-appid")?;if !valid(id){return None}
        let name=row.select(&selector(".title")).next()?.text().collect::<String>();
        let tags:Vec<u32>=serde_json::from_str(row.value().attr("data-ds-tagids").unwrap_or("[]")).unwrap_or_default();
        let categories:Vec<&str>=TAGS.iter().filter(|(tag,_,_)|tags.contains(&tag.parse().unwrap_or(0))).map(|(_,ru,_)|*ru).collect();
        let discount=row.select(&selector(".discount_pct")).next().map(|e|e.text().collect::<String>()).unwrap_or_default().replace(['-','%'],"").parse::<u32>().unwrap_or(0);
        let price=row.select(&selector(".discount_final_price")).next().map(|e|e.text().collect::<String>()).unwrap_or_default();
        Some(json!({"id":id,"name":name.trim(),"categories":categories,"discount":discount,"price":price,"store":"Steam","url":format!("https://store.steampowered.com/app/{id}/")}))
    }).collect()
}
fn search(params: &[(&str, String)]) -> Result<Value> {
    let bytes = http()?
        .get("https://store.steampowered.com/search/results/")
        .query(params)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(err)?
        .bytes()
        .map_err(err)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("Catalog response too large".into());
    }
    serde_json::from_slice(&bytes).map_err(err)
}
fn recommendations(profile: &Path, p: &Value) -> Result<Value> {
    let base: Vec<Value> = serde_json::from_value(crate::remote::catalog(profile,"recommendations").unwrap_or_else(||serde_json::from_str(include_str!("../catalog/recommendations.json")).expect("embedded recommendations"))).map_err(err)?;
    let file = profile.join("recommendations-live.json");
    let old = read(&file, Value::Null).unwrap_or(Value::Null);
    let due = chrono::DateTime::parse_from_rfc3339(str(&old, "UpdatedUtc"))
        .map(|t| {let elapsed=chrono::Utc::now()-t.with_timezone(&chrono::Utc);elapsed.num_seconds()<0||elapsed.num_seconds()>=crate::remote::seconds(profile,"recommendationsCacheSeconds",21600) as i64})
        .unwrap_or(true);
    let mut snapshot = old;
    if (due || p["refresh"] == true) && prefs(profile)?["OfflineMode"] != true {
        let refreshed = (|| -> Result<Value> {
            let chart: Value = serde_json::from_slice(&http_bytes(
                "https://api.steampowered.com/ISteamChartsService/GetMostPlayedGames/v1/",
                2 * 1024 * 1024,
            )?)
            .map_err(err)?;
            let search = search(&[
                ("filter", "concurrentusers".into()),
                ("count", "100".into()),
                ("start", "0".into()),
                ("infinite", "1".into()),
                ("category1", "998".into()),
                ("cc", "US".into()),
                ("l", "english".into()),
            ])?;
            let found = search_rows(str(&search, "results_html"));
            let mut games = vec![];
            for rank in chart["response"]["ranks"]
                .as_array()
                .ok_or("Missing Steam ranks")?
            {
                let id = rank["appid"].as_u64().unwrap_or(0).to_string();
                if let Some(game) = found.iter().chain(base.iter()).find(|g| str(g, "id") == id) {
                    games.push(game.clone());
                }
            }
            if games.len() < 5 {
                return Err("Steam returned an incomplete chart".into());
            }
            Ok(json!({"UpdatedUtc":now(),"Games":games}))
        })();
        if let Ok(next) = refreshed {
            write(&file, &next)?;
            snapshot = next;
        }
    }
    let mut ids = HashSet::new();
    let games: Vec<Value> = snapshot["Games"]
        .as_array()
        .into_iter()
        .flatten()
        .cloned()
        .chain(base)
        .filter_map(|mut g| {
            if g["id"].is_null() {
                g = json!({"id":g["Id"],"name":g["Name"],"categories":g["Categories"]});
            }
            if valid(str(&g, "id"))&&!str(&g,"name").trim().is_empty()&&g["categories"].as_array().is_some_and(|a|a.iter().all(Value::is_string))&&ids.insert(str(&g, "id").to_string()) {
                Some(g)
            } else {
                None
            }
        })
        .collect();
    Ok(json!(games))
}
pub fn handle(profile: &Path, op: &str, p: &Value) -> Result<Value> {
    match op {
        "artwork" => art(profile, p),
        "recommendations" => recommendations(profile, p),
        "store_details" => {
            let id = str(p, "id");
            if !valid(id) {
                return Err("Invalid Steam ID".into());
            }
            let lang = if str(p, "language") == "en" || language(profile) == "en" {
                "english"
            } else {
                "russian"
            };
            let cache = profile.join(format!("store-details/{id}-{lang}.json"));
            if let Ok(v) = read(&cache, Value::Null) {
                if !v.is_null() {
                    return Ok(v);
                }
            }
            if prefs(profile)?["OfflineMode"] == true {
                return Err("Description is not cached offline".into());
            }
            let v: Value = serde_json::from_slice(&http_bytes(
                &format!("https://store.steampowered.com/api/appdetails?appids={id}&l={lang}"),
                4 * 1024 * 1024,
            )?)
            .map_err(err)?;
            let d = &v[id]["data"];
            if !d.is_object() {
                return Err("Game description is unavailable".into());
            }
            let description = regex::Regex::new("<[^>]*>")
                .unwrap()
                .replace_all(str(d, "short_description"), "")
                .into_owned();
            let out = json!({"id":id,"name":d["name"],"description":html_escape::decode_html_entities(&description),"genres":d["genres"].as_array().into_iter().flatten().filter_map(|g|g["description"].as_str()).collect::<Vec<_>>(),"price":d["price_overview"]["final_formatted"]});
            write(&cache, &out)?;
            Ok(out)
        }
        "deals" => {
            use sha2::{Digest, Sha256};
            let key = format!("{:X}", Sha256::digest(serde_json::to_vec(p).map_err(err)?));
            let cache = profile.join(format!("deals-cache/{key}.json"));
            if prefs(profile)?["OfflineMode"] == true {
                let mut cached = read(&cache, Value::Null)?;
                if cached.is_null() {
                    return Err("Deals are not cached offline".into());
                }
                cached["cached"] = json!(true);
                return Ok(cached);
            }
            let page = p["page"].as_u64().unwrap_or(0).min(200);
            let discount = p["minimum"].as_u64().unwrap_or(10).min(100);
            let region = if str(p, "region").len() == 2 {
                str(p, "region")
            } else {
                "US"
            };
            let mut items = vec![];
            let mut total = 0;
            let mut errors = vec![];
            if str(p, "store") != "Epic Games" {
                let mut q = vec![
                    ("specials", "1".into()),
                    ("start", (page * 50).to_string()),
                    ("count", "50".into()),
                    ("infinite", "1".into()),
                    ("cc", region.into()),
                    (
                        "l",
                        if language(profile) == "en" {
                            "english".into()
                        } else {
                            "russian".into()
                        },
                    ),
                ];
                if !str(p, "genre").is_empty() {
                    q.push(("tags", str(p, "genre").into()));
                }
                if !str(p, "category").is_empty() {
                    q.push(("category1", str(p, "category").into()));
                }
                if !str(p, "query").is_empty() {
                    q.push(("term", str(p, "query").into()));
                }
                match search(&q) {
                    Ok(result) => {
                        total = result["total_count"].as_u64().unwrap_or(0);
                        items.extend(
                            search_rows(str(&result, "results_html"))
                                .into_iter()
                                .filter(|g| g["discount"].as_u64().unwrap_or(0) >= discount),
                        );
                    }
                    Err(_) => errors.push("Steam"),
                }
            }
            if page == 0 && str(p, "store") != "Steam" {
                let url=format!("https://store-site-backend-static.ak.epicgames.com/freeGamesPromotions?locale={}&country={region}&allowCountries={region}",if language(profile)=="en"{"en-US"}else{"ru"});
                match http_bytes(&url, 8 * 1024 * 1024)
                    .and_then(|b| serde_json::from_slice::<Value>(&b).map_err(err))
                {
                    Ok(result) => {
                        for item in result["data"]["Catalog"]["searchStore"]["elements"]
                            .as_array()
                            .into_iter()
                            .flatten()
                        {
                            let active = item["promotions"]["promotionalOffers"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .flat_map(|g| {
                                    g["promotionalOffers"].as_array().into_iter().flatten()
                                })
                                .any(|g| {
                                    let start =
                                        chrono::DateTime::parse_from_rfc3339(str(g, "startDate"))
                                            .ok();
                                    let end =
                                        chrono::DateTime::parse_from_rfc3339(str(g, "endDate"))
                                            .ok();
                                    g["discountSetting"]["discountPercentage"] == 0
                                        && start.is_some_and(|s| s <= chrono::Utc::now())
                                        && end.is_some_and(|e| e > chrono::Utc::now())
                                });
                            if active {
                                let slug = str(item, "productSlug");
                                items.push(json!({"id":format!("epic-{}",str(item,"id")),"name":item["title"],"categories":["Free"],"discount":100,"price":"Free","store":"Epic Games","image":item["keyImages"].as_array().into_iter().flatten().find(|i|str(i,"type")=="OfferImageWide").or_else(||item["keyImages"].as_array().and_then(|a|a.first())).map(|i|i["url"].clone()),"url":if slug.is_empty(){"https://store.epicgames.com/free-games".to_string()}else{format!("https://store.epicgames.com/p/{slug}")}}));
                            }
                        }
                    }
                    Err(_) => errors.push("Epic Games"),
                }
            }
            let result = json!({"items":items,"total":total,"page":page,"errors":errors,"updated":now(),"cached":false});
            if !items.is_empty() || errors.is_empty() {
                write(&cache, &result)?;
            } else if let Ok(mut cached) = read(&cache, Value::Null) {
                if cached.is_object() {
                    cached["cached"] = json!(true);
                    return Ok(cached);
                }
            }
            Ok(result)
        }
        _ => Err("Unknown media operation".into()),
    }
}
pub fn custom(profile: &Path, path: &Path) -> Result<String> {
    no_links(path)?;
    if fs::metadata(path).map_err(err)?.len() > 10 * 1024 * 1024 {
        return Err("Custom artwork exceeds 10 MB".into());
    }
    let img = image(&fs::read(path).map_err(err)?)?;
    let mut output = Cursor::new(vec![]);
    img.write_to(&mut output, image::ImageFormat::Jpeg)
        .map_err(err)?;
    let target = profile.join(format!("custom-artwork/{}.jpg", id()));
    atomic(&target, &output.into_inner())?;
    Ok(target.to_string_lossy().to_string())
}
