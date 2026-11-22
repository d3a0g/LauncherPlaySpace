use crate::{common::*,library};
use serde_json::{json,Value};
use std::{fs,path::Path};

pub fn handle(profile:&Path,p:&Value)->Result<Value>{
    match str(p,"action") {
        "scan_folder"=>{
            let root=Path::new(str(p,"path")); specific_directory(root)?;
            no_links(root)?;
            if !root.is_dir(){return Err("Select an existing games folder".into())}
            let mut files=Vec::new(); let mut pending=vec![(root.to_path_buf(),0)]; let mut visited=0;
            while let Some((dir,depth))=pending.pop(){
                if depth>8 {continue}
                for entry in fs::read_dir(dir).map_err(err)? {
                    let entry=entry.map_err(err)?; visited+=1;
                    if visited>50000{return Err("Folder is too large; select a smaller games folder".into())}
                    let ty=entry.file_type().map_err(err)?; if ty.is_symlink(){continue}
                    let path=entry.path();
                    if ty.is_dir(){pending.push((path,depth+1));continue}
                    let name=entry.file_name().to_string_lossy().to_lowercase();
                    if !name.ends_with(".exe") || ["unins","setup","crash","redist","unitycrash","report","launcher","helper"].iter().any(|s|name.contains(s)){continue}
                    let stem=path.file_stem().unwrap_or_default().to_string_lossy();
                    let unity=path.with_file_name(format!("{stem}_Data")).is_dir();
                    let unreal=path.to_string_lossy().replace('\\',"/").to_lowercase().contains("/binaries/win64/");
                    files.push(json!({"path":path,"name":stem,"confidence":if unity||unreal{"game"}else{"review"}}));
                    if files.len()>500{return Err("Too many executables; select a smaller folder".into())}
                }
            }
            Ok(json!(files))
        },
        "bulk"=>{
            let ids=p["ids"].as_array().ok_or("Select games")?;
            if ids.is_empty(){return Err("Select games".into())}
            let _lock=library::lock(profile)?;
            let mut games=library::load(&profile.join("games.json"))?.0;
            write(&profile.join("library-tools-undo.json"),&json!(games))?;
            let mut affected=0;
            for g in &mut games {if ids.iter().any(|id|id.as_str()==Some(&library::text(g,"LibraryId"))){
                match str(p,"field") {
                    "IsPinned"=>library::set(g,"IsPinned",json!(p["value"]==true)),
                    "PlayStatus"=>{let s=str(p,"value");if !["planned","playing","completed","paused",""].contains(&s){return Err("Invalid status".into())}library::set(g,"PlayStatus",json!(s));},
                    _=>return Err("Unsupported bulk action".into())
                }affected+=1;
            }}
            library::save(profile,&games)?; Ok(json!({"affected":affected}))
        },
        "undo"=>{
            let _lock=library::lock(profile)?;
            let before:Vec<Value>=serde_json::from_value(read(&profile.join("library-tools-undo.json"),Value::Null)?).map_err(err)?;
            let mut current=library::load(&profile.join("games.json"))?.0;
            for game in &mut current {if let Some(old)=before.iter().find(|g|library::text(g,"LibraryId")==library::text(game,"LibraryId")) {
                for key in ["IsPinned","PlayStatus"] {library::set(game,key,old[key].clone());}
            }}
            library::save(profile,&current)?;fs::remove_file(profile.join("library-tools-undo.json")).map_err(err)?;
            Ok(json!({"done":true}))
        },
        "restore"=>{
            let _lock=library::lock(profile)?;
            let token=str(p,"archive"); uuid::Uuid::parse_str(token).map_err(err)?;
            let file=profile.join(format!("removed-games/{token}.json"));
            let archived=read(&file,Value::Null)?;
            let entries:Vec<Value>=serde_json::from_value(if archived.is_array(){archived}else{archived["Games"].clone()}).map_err(err)?;
            let mut games=library::load(&profile.join("games.json"))?.0;
            for g in entries {if !g.is_object()||library::text(&g,"Name").is_empty()||library::text(&g,"ExePath").is_empty(){return Err("Invalid archived game".into())}
                if !games.iter().any(|x|library::same(x,&g)){games.push(g)}
            }
            library::save(profile,&games)?; fs::remove_file(file).map_err(err)?;Ok(json!({"done":true}))
        },
        _=>{
            let mut archives=vec![];
            if let Ok(entries)=fs::read_dir(profile.join("removed-games")){for entry in entries.flatten(){
                let path=entry.path();let token=path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                if uuid::Uuid::parse_str(&token).is_err(){continue}
                if let Ok(value)=read(&path,Value::Null){let games=if value.is_array(){value}else{value["Games"].clone()};archives.push(json!({"id":token,"games":games}));}
            }}
            Ok(json!({"archives":archives}))
        }
    }
}

#[cfg(test)] mod tests {
 use super::*;
 #[test] fn bulk_undo_preserves_playtime_and_restores_archived_records(){
  let p=std::env::temp_dir().join(format!("playspace-library-tools-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&p).unwrap();
  let id=uuid::Uuid::new_v4().simple().to_string();let g=json!({"LibraryId":id,"Name":"Example","ExePath":"C:/Games/example.exe","PlayedSeconds":30,"IsPinned":false});
  library::save(&p,&[g.clone()]).unwrap();
  handle(&p,&json!({"action":"bulk","ids":[id],"field":"IsPinned","value":true})).unwrap();
  let mut games=library::load(&p.join("games.json")).unwrap().0;games[0]["PlayedSeconds"]=json!(99);library::save(&p,&games).unwrap();
  handle(&p,&json!({"action":"undo"})).unwrap();let games=library::load(&p.join("games.json")).unwrap().0;
  assert_eq!(games[0]["PlayedSeconds"],99);assert_eq!(games[0]["IsPinned"],false);
  library::handle(&p,&p.join("absent"),"remove_game",&json!({"id":id})).unwrap();
  let archives=handle(&p,&json!({})).unwrap();let token=&archives["archives"][0]["id"];
  handle(&p,&json!({"action":"restore","archive":token})).unwrap();
  assert_eq!(library::load(&p.join("games.json")).unwrap().0.len(),1);
  assert!(handle(&p,&json!({"action":"restore","archive":"../outside"})).is_err());
 }
}
