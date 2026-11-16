//! Reconcile library records only. Never delete game files, saves, or mod data.
use crate::{common::*, library::{self, text}};
use serde_json::{json, Value};
use std::{collections::{HashMap, HashSet}, fs, io::ErrorKind, path::{Component, Path, PathBuf, Prefix}, sync::{Mutex, LazyLock}};
use std::os::windows::fs::OpenOptionsExt;

const INTERVAL: i64 = 15;
const GRACE: i64 = 30;
#[derive(Default)]
struct Checks { last: Option<i64>, missing: HashMap<String, (String, i64)> }
static CHECKS: LazyLock<Mutex<HashMap<PathBuf, Checks>>> = LazyLock::new(||Mutex::new(HashMap::new()));
#[derive(Debug, PartialEq)]
enum Status { Installed, Missing, Unknown }

fn local_root(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute(){return None}
    let Component::Prefix(prefix)=path.components().next()? else {return None};
    let drive=match prefix.kind(){Prefix::Disk(d)|Prefix::VerbatimDisk(d)=>d,_=>return None};
    let root=PathBuf::from(format!("{}:\\",drive as char));
    #[link(name="kernel32")]
    extern "system" {fn GetDriveTypeW(root:*const u16)->u32;}
    let wide:Vec<u16>=root.to_string_lossy().encode_utf16().chain(Some(0)).collect();
    // Offline, UNC and mapped network paths are not proof of uninstallation.
    if !matches!(unsafe{GetDriveTypeW(wide.as_ptr())},2|3|6) || fs::read_dir(&root).is_err(){return None}
    Some(root)
}
fn path_status(path: &Path, directory: bool) -> Status {
    if local_root(path).is_none() || no_links(path).is_err(){return Status::Unknown}
    match fs::metadata(path) {
        Ok(m)=>if if directory {m.is_dir()} else {m.is_file()} {Status::Installed}else{Status::Unknown},
        Err(e) if e.kind()==ErrorKind::NotFound=>{
            for parent in path.ancestors().skip(1) {
                match fs::metadata(parent) {
                    Ok(m)=>return if m.is_dir()&&fs::read_dir(parent).is_ok(){Status::Missing}else{Status::Unknown},
                    Err(e) if e.kind()==ErrorKind::NotFound=>continue,
                    _=>return Status::Unknown,
                }
            }
            Status::Unknown
        }
        _=>Status::Unknown,
    }
}
fn libraries() -> Vec<PathBuf> {
    let Some(root)=crate::discovery::steam() else{return vec![]};
    let mut roots=vec![root.clone()];
    if let Ok(vdf)=fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
        let mut paths=0;
        for entry in regex::Regex::new(r#""path"\s*"([^"]+)""#).unwrap().captures_iter(&vdf){roots.push(PathBuf::from(entry[1].replace("\\\\","\\")));paths+=1;}
        if paths==0 {roots.push(PathBuf::new());}
    } else {roots.push(PathBuf::new());}
    roots.into_iter().map(|p|if p.as_os_str().is_empty(){p}else{p.join("steamapps")}).collect()
}
fn steam_folder(g: &Value) -> Option<PathBuf> {
    for key in ["InstallDirectory","ExePath"] {
        let path=PathBuf::from(text(g,key));
        if path.is_absolute(){for parent in path.ancestors(){if parent.file_name().is_some_and(|n|n.eq_ignore_ascii_case("steamapps")){return Some(parent.to_path_buf())}}}
    }
    None
}
fn status(g: &Value, registered: &[PathBuf]) -> Status {
    let exe=PathBuf::from(text(g,"ExePath"));
    let steam=text(g,"SteamAppId");
    if steam.parse::<u32>().is_ok_and(|id|id>0) {
        let derived=steam_folder(g);
        // A manually associated Steam ID does not invalidate a working local copy.
        if derived.is_none() && path_status(&exe,false)==Status::Installed{return Status::Installed}
        let mut roots=registered.to_vec();if let Some(root)=derived{roots.push(root)}
        roots.sort();roots.dedup();
        if !roots.is_empty(){
            let mut uncertain=false;
            for root in roots {
                if path_status(&root,true)!=Status::Installed {uncertain=true;continue}
                let manifest=root.join(format!("appmanifest_{steam}.acf"));
                match path_status(&manifest,false) {
                    Status::Missing=>continue,
                    Status::Unknown=>{uncertain=true;continue},
                    Status::Installed=>{}
                }
                let Ok(content)=fs::read_to_string(&manifest) else{uncertain=true;continue};
                if crate::discovery::vdf(&content,"appid").as_deref()!=Some(&steam){uncertain=true;continue}
                let Some(dir)=crate::discovery::vdf(&content,"installdir") else{uncertain=true;continue};
                let Ok(folder)=safe(&root.join("common"),&dir) else{uncertain=true;continue};
                match path_status(&folder,true) {
                    Status::Installed=>return Status::Installed,
                    // Missing/renamed EXEs while Steam updates must not remove the game.
                    Status::Missing if crate::discovery::vdf(&content,"StateFlags").as_deref()==Some("4")=>{},
                    _=>uncertain=true,
                }
            }
            return if uncertain {Status::Unknown}else{Status::Missing}
        }
    }
    if text(g,"LaunchArguments").contains("--launch-product=valorant") {
        let mut root=PathBuf::from(text(g,"InstallDirectory"));
        if root.as_os_str().is_empty(){
            let config=PathBuf::from(std::env::var_os("PROGRAMDATA").unwrap_or_default()).join("Riot Games/RiotClientInstalls.json");
            if let Ok(v)=read(&config,Value::Null){if let Some(clients)=v["associated_client"].as_object(){for (folder,client) in clients{if folder.replace('\\',"/").to_lowercase().ends_with("/valorant/live")&&client.as_str().is_some_and(|c|c.eq_ignore_ascii_case(&exe.to_string_lossy())){root=PathBuf::from(folder);break}}}}
        }
        if root.as_os_str().is_empty(){return Status::Unknown}
        let states=[path_status(&root.join("VALORANT.exe"),false),path_status(&root.join("ShooterGame/Binaries/Win64/VALORANT-Win64-Shipping.exe"),false)];
        return if states.contains(&Status::Installed){Status::Installed}else if states.iter().all(|s|*s==Status::Missing){Status::Missing}else{Status::Unknown}
    }
    path_status(&exe,false)
}
fn inactive(profile: &Path, game: &Value) -> bool {
    use sha2::{Digest,Sha256};
    let path=profile.join(format!("sessions/{:X}.lock",Sha256::digest(text(game,"LibraryId").as_bytes())));
    if !path.exists(){return true}
    fs::OpenOptions::new().read(true).write(true).share_mode(0).open(path).is_ok()
}
pub fn reconcile(profile: &Path) -> Result<usize> {reconcile_at(profile,chrono::Utc::now().timestamp())}
fn reconcile_at(profile: &Path, clock: i64) -> Result<usize> {
    let mut all=CHECKS.lock().map_err(|_|"Installation check lock failed")?;
    let check=all.entry(profile.to_path_buf()).or_default();
    if check.last.is_some_and(|last|clock>=last&&clock-last<INTERVAL){return Ok(0)}
    let Ok(_guard)=library::lock(profile) else{return Ok(0)};
    let (games,notice)=library::load(&profile.join("games.json"))?;
    if notice.is_some(){return Ok(0)} // Preserve a corrupt primary library for recovery.
    check.last=Some(clock);
    let registered=libraries();let mut removed=vec![];let mut keep=vec![];
    let ids:HashSet<_>=games.iter().map(|g|text(g,"LibraryId")).collect();
    check.missing.retain(|id,_|ids.contains(id));
    for g in games {
        let id=text(&g,"LibraryId");
        if id.is_empty()||!inactive(profile,&g)||status(&g,&registered)!=Status::Missing {
            check.missing.remove(&id);keep.push(g);continue
        }
        let fingerprint=format!("{}|{}|{}",text(&g,"ExePath"),text(&g,"InstallDirectory"),text(&g,"SteamAppId"));
        let observation=check.missing.entry(id.clone()).or_insert((fingerprint.clone(),clock));
        if observation.0!=fingerprint||clock<observation.1 {*observation=(fingerprint,clock)}
        if clock-observation.1>=GRACE {removed.push(g)}else{keep.push(g)}
    }
    if removed.is_empty(){return Ok(0)}
    let count=removed.len();
    // A durable, separate archive survives subsequent games.json.bak rotations.
    let archive=profile.join(format!("removed-games/{}.json",crate::common::id()));
    write(&archive,&json!({"RemovedUtc":now(),"Reason":"installation_missing","Games":removed}))?;
    library::save(profile,&keep)?;
    for g in &removed{check.missing.remove(&text(g,"LibraryId"));}
    audit(profile,"Library",&format!("Removed {count} uninstalled game records; archived metadata"));
    Ok(count)
}

#[cfg(test)] mod tests {
    use super::*;
    fn fixture()->(PathBuf,Value){
        let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/installation-tests").join(crate::common::id());fs::create_dir_all(&root).unwrap();
        let g=json!({"LibraryId":crate::common::id(),"Name":"Local game","ExePath":root.join("game/game.exe"),"PlayedSeconds":123,"future":{"keep":true}});
        library::save(&root,&[g.clone()]).unwrap();(root,g)
    }
    #[test] fn missing_records_are_archived_after_a_grace_period(){
        let (p,g)=fixture();assert_eq!(reconcile_at(&p,100).unwrap(),0);assert_eq!(reconcile_at(&p,115).unwrap(),0);assert_eq!(reconcile_at(&p,130).unwrap(),1);
        assert!(library::load(&p.join("games.json")).unwrap().0.is_empty());
        let archive=fs::read_dir(p.join("removed-games")).unwrap().next().unwrap().unwrap().path();
        let v=read(&archive,Value::Null).unwrap();assert_eq!(v["Games"][0],g);assert!(p.join("games.json.bak").exists());
    }
    #[test] fn reappearing_files_reset_the_grace_period(){
        let (p,g)=fixture();reconcile_at(&p,100).unwrap();let exe=PathBuf::from(text(&g,"ExePath"));fs::create_dir_all(exe.parent().unwrap()).unwrap();fs::write(&exe,b"fixture").unwrap();reconcile_at(&p,130).unwrap();
        fs::remove_file(exe).unwrap();assert_eq!(reconcile_at(&p,160).unwrap(),0);assert_eq!(reconcile_at(&p,190).unwrap(),1);
    }
    #[test] fn network_and_relative_paths_are_never_proof_of_uninstallation(){
        assert_eq!(path_status(Path::new(r"\\unavailable\games\game.exe"),false),Status::Unknown);assert_eq!(path_status(Path::new("relative/game.exe"),false),Status::Unknown);
    }
    #[test] fn steam_manifest_protects_a_game_with_a_stale_executable_path(){
        let (p,mut g)=fixture();let apps=p.join("steamapps");fs::create_dir_all(apps.join("common/TestGame")).unwrap();fs::write(apps.join("appmanifest_999999999.acf"),"\"appid\" \"999999999\" \"installdir\" \"TestGame\" \"StateFlags\" \"4\"").unwrap();g["SteamAppId"]=json!("999999999");g["ExePath"]=json!(apps.join("common/TestGame/old.exe"));assert_eq!(status(&g,&[]),Status::Installed);
        fs::remove_file(apps.join("appmanifest_999999999.acf")).unwrap();assert_eq!(status(&g,&[]),Status::Missing);
    }
    #[test] fn active_sessions_and_locked_libraries_are_preserved(){
        let (p,g)=fixture();let guard=library::lock(&p).unwrap();assert_eq!(reconcile_at(&p,100).unwrap(),0);drop(guard);
        use sha2::{Digest,Sha256};let path=p.join(format!("sessions/{:X}.lock",Sha256::digest(text(&g,"LibraryId").as_bytes())));fs::create_dir_all(path.parent().unwrap()).unwrap();let _session=fs::OpenOptions::new().create(true).write(true).share_mode(0).open(path).unwrap();reconcile_at(&p,100).unwrap();assert_eq!(reconcile_at(&p,140).unwrap(),0);assert_eq!(library::load(&p.join("games.json")).unwrap().0.len(),1);
    }
    #[test] fn steam_updates_and_unknown_libraries_do_not_remove_games(){
        let (p,mut g)=fixture();let apps=p.join("steamapps");fs::create_dir_all(&apps).unwrap();g["SteamAppId"]=json!("999999999");g["ExePath"]=json!(apps.join("common/Updating/old.exe"));
        fs::write(apps.join("appmanifest_999999999.acf"),"\"appid\" \"999999999\" \"installdir\" \"Updating\" \"StateFlags\" \"1026\"").unwrap();assert_eq!(status(&g,&[]),Status::Unknown);
        fs::remove_file(apps.join("appmanifest_999999999.acf")).unwrap();assert_eq!(status(&g,&[PathBuf::new()]),Status::Unknown);
    }
    #[test] fn corrupt_primary_library_is_not_rewritten_by_cleanup(){
        let (p,_)=fixture();library::save(&p,&library::load(&p.join("games.json")).unwrap().0).unwrap();fs::write(p.join("games.json"),b"corrupt-primary").unwrap();reconcile_at(&p,100).unwrap();assert_eq!(reconcile_at(&p,200).unwrap(),0);assert_eq!(fs::read(p.join("games.json")).unwrap(),b"corrupt-primary");
    }
}
