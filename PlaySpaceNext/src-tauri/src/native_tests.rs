use crate::{common::*, library, media, mods, saves, workspace};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn area() -> (PathBuf, PathBuf, Value) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/native-tests")
        .join(id());
    let profile = root.join("profile");
    let game_folder = root.join("game");
    fs::create_dir_all(&profile).unwrap();
    fs::create_dir_all(&game_folder).unwrap();
    let exe = game_folder.join("fixture.exe");
    fs::write(&exe, b"not executable, file-operation fixture only").unwrap();
    let game = json!({"Name":"Fixture","ExePath":exe,"InstallDirectory":game_folder,"SteamAppId":"1091500","LibraryId":id(),"PlayedSeconds":0,"futureField":{"keep":true}});
    library::save(&profile, &[game.clone()]).unwrap();
    (root, profile, game)
}
#[test]
fn paths_reject_traversal_links_ads_and_windows_devices() {
    let (root, _, _) = area();
    for name in [
        "../escape",
        "C:/escape",
        "/absolute",
        "a/../../escape",
        "a:stream",
        "NUL.txt",
        "CON",
        "x//y",
        "x./y",
        "x/.. /file",
    ] {
        assert!(safe(&root, name).is_err(), "{name}");
    }
    assert!(safe(&root, "BepInEx/plugins/author-mod/file.dll").is_ok());
}
#[test]
fn preferences_and_collections_preserve_data() {
    let (_, profile, g) = area();
    write(
        &profile.join("preferences.json"),
        &json!({"unknown":"keep","Language":"ru"}),
    )
    .unwrap();
    workspace::handle(
        &profile,
        &json!({"area":"settings","action":"save","value":{"Language":"en","OfflineMode":true}}),
    )
    .unwrap();
    assert_eq!(prefs(&profile).unwrap()["unknown"], "keep");
    assert_eq!(language(&profile), "en");
    assert!(workspace::handle(
        &profile,
        &json!({"area":"settings","action":"save","value":{"Language":"invalid"}})
    )
    .is_err());
    workspace::handle(
        &profile,
        &json!({"area":"collections","action":"create","name":"Co-op"}),
    )
    .unwrap();
    workspace::handle(&profile,&json!({"area":"collections","action":"set","name":"Co-op","id":g["LibraryId"],"included":true})).unwrap();
    let result = workspace::handle(
        &profile,
        &json!({"area":"collections","action":"rename","original":"Co-op","name":"Friends"}),
    )
    .unwrap();
    assert_eq!(
        result["Members"][g["LibraryId"].as_str().unwrap()][0],
        "Friends"
    );
    assert_eq!(
        library::load(&profile.join("games.json")).unwrap().0[0]["futureField"]["keep"],
        true
    );
}
#[test]
fn backups_verify_before_restore_and_keep_safety_snapshot() {
    let (root, profile, g) = area();
    let folder = root.join("saves");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("slot.sav"), b"original").unwrap();
    let params = json!({"id":g["LibraryId"],"action":"configure","directory":folder,"notes":"test","automatic":true});
    saves::handle(&profile, &params).unwrap();
    let point = saves::handle(&profile, &json!({"id":g["LibraryId"],"action":"create"})).unwrap();
    fs::write(folder.join("slot.sav"), b"changed").unwrap();
    let preview = saves::handle(
        &profile,
        &json!({"id":g["LibraryId"],"action":"preview","snapshot":point["Id"]}),
    )
    .unwrap();
    assert_eq!(preview[0]["action"], "replace");
    let result = saves::handle(
        &profile,
        &json!({"id":g["LibraryId"],"action":"restore","snapshot":point["Id"],"confirmed":true}),
    )
    .unwrap();
    assert_eq!(result["restored"], 1);
    assert_eq!(fs::read(folder.join("slot.sav")).unwrap(), b"original");
    let care = &library::load(&profile.join("games.json")).unwrap().0[0]["Care"];
    let file = profile
        .join("game-backups")
        .join(str(care, "Id"))
        .join(str(&point, "Id"))
        .join("files/slot.sav");
    fs::write(file, b"corrupt").unwrap();
    assert!(saves::handle(
        &profile,
        &json!({"id":g["LibraryId"],"action":"restore","snapshot":point["Id"],"confirmed":true})
    )
    .is_err());
    assert_eq!(fs::read(folder.join("slot.sav")).unwrap(), b"original");
}
#[test]
fn automatic_snapshots_skip_identical_data_and_keep_protected_versions() {
    let (root, profile, game) = area();
    let saves_folder = root.join("save-data");
    fs::create_dir(&saves_folder).unwrap();
    let file = saves_folder.join("slot.sav");
    fs::write(&file,b"one").unwrap();
    let id = &game["LibraryId"];
    saves::handle(&profile,&json!({"id":id,"action":"configure","directory":saves_folder,"afterPlaying":true,"retention":1})).unwrap();
    let game = library::load(&profile.join("games.json")).unwrap().0.remove(0);
    let list = || saves::handle(&profile,&json!({"id":id,"action":"get"})).unwrap()["snapshots"].as_array().unwrap().clone();
    saves::after_playing(&profile,&game).unwrap();
    saves::after_playing(&profile,&game).unwrap();
    let first = list();
    assert_eq!(first.len(),1);
    saves::handle(&profile,&json!({"id":id,"action":"protect","snapshot":first[0]["Id"],"protected":true})).unwrap();
    for bytes in [b"two",b"new"] {
        fs::write(&file,bytes).unwrap();
        saves::after_playing(&profile,&game).unwrap();
    }
    let remaining=list();
    assert_eq!(remaining.len(),2);
    assert!(remaining.iter().any(|point|point["Id"]==first[0]["Id"] && point["Protected"]==true));
}

#[test]
fn backup_configuration_rejects_profile_and_executables() {
    let (root, profile, g) = area();
    assert!(saves::handle(
        &profile,
        &json!({"id":g["LibraryId"],"action":"configure","directory":profile})
    )
    .is_err());
    let folder = root.join("saves");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("payload.dll"), b"dll").unwrap();
    saves::handle(
        &profile,
        &json!({"id":g["LibraryId"],"action":"configure","directory":folder}),
    )
    .unwrap();
    assert!(saves::handle(&profile, &json!({"id":g["LibraryId"],"action":"create"})).is_err());
}
fn zip(path: &std::path::Path, name: &str, bytes: &[u8]) {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    zip.start_file(name, zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(bytes).unwrap();
    zip.finish().unwrap();
}
#[test]
fn native_zip_mods_do_not_overwrite_and_modified_files_are_preserved() {
    let (root, profile, g) = area();
    let archive = root.join("mod.zip");
    zip(&archive, "example.archive", b"archive contents");
    let p = json!({"id":g["LibraryId"],"action":"import","path":archive,"kind":"native","confirmed":true});
    let result = mods::handle(&profile, &p).unwrap();
    assert_eq!(result["installed"], 1);
    let root = PathBuf::from(g["InstallDirectory"].as_str().unwrap()).join("archive/pc/mod");
    let installed = root.join("example.archive");
    assert_eq!(fs::read(&installed).unwrap(), b"archive contents");
    let state = mods::handle(
        &profile,
        &json!({"id":g["LibraryId"],"action":"get","kind":"native"}),
    )
    .unwrap();
    let package = state["installed"]["Packages"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap();
    fs::write(&installed, b"user-edited").unwrap();
    assert!(mods::handle(
        &profile,
        &json!({"id":g["LibraryId"],"action":"uninstall","package":package,"kind":"native"})
    )
    .is_err());
    assert_eq!(fs::read(&installed).unwrap(), b"user-edited");
    assert!(mods::handle(&profile, &p).is_err());
    fs::write(&installed, b"archive contents").unwrap();
    mods::handle(
        &profile,
        &json!({"id":g["LibraryId"],"action":"uninstall","package":package,"kind":"native"}),
    )
    .unwrap();
    assert!(!installed.exists());
}
#[test]
fn zip_slip_is_rejected_without_writes() {
    let (root, profile, g) = area();
    let archive = root.join("bad.zip");
    zip(&archive, "../escape.archive", b"payload");
    assert!(mods::handle(&profile,&json!({"id":g["LibraryId"],"action":"import","path":archive,"kind":"native","confirmed":true})).is_err());
    assert!(!root.join("escape.archive").exists());
}
#[test]
fn image_fallback_works_from_offline_wide_cache() {
    let (_, profile, _) = area();
    write(
        &profile.join("preferences.json"),
        &json!({"OfflineMode":true}),
    )
    .unwrap();
    let image = image::DynamicImage::new_rgb8(460, 215);
    let path = profile.join("wide-posters/620.jpg");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    image.save(&path).unwrap();
    let result = media::handle(&profile, "artwork", &json!({"id":"620","portrait":true})).unwrap();
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(result["url"].as_str().unwrap().split(',').nth(1).unwrap())
        .unwrap();
    let img = image::load_from_memory(&bytes).unwrap();
    assert_eq!((img.width(), img.height()), (600, 900));
    assert!(
        media::handle(&profile, "artwork", &json!({"id":"../escape"}))
            .unwrap()
            .is_null()
    );
}
#[test]
fn local_game_cover_resolves_exact_title_without_mutating_library() {
    let (_, profile, mut g) = area();
    g["Name"]=json!("Portal 2");g["SteamAppId"]=Value::Null;
    library::save(&profile,&[g.clone()]).unwrap();
    let before=fs::read(profile.join("games.json")).unwrap();
    write(&profile.join("preferences.json"),&json!({"OfflineMode":true})).unwrap();
    let path=profile.join("wide-posters/620.jpg");fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::DynamicImage::new_rgb8(460,215).save(path).unwrap();
    let result=media::handle(&profile,"artwork",&json!({"gameId":g["LibraryId"],"portrait":true})).unwrap();
    assert!(result["url"].as_str().is_some_and(|s|s.starts_with("data:image/jpeg;base64,")));
    assert_eq!(fs::read(profile.join("games.json")).unwrap(),before);
}
#[test]
fn unsupported_mod_game_has_a_read_only_empty_state() {
    let (_,profile,mut g)=area();
    g["SteamAppId"]=json!("999999999");g["ExePath"]=json!("C:\\Unmatched\\unknown.exe");
    library::save(&profile,&[g.clone()]).unwrap();
    let data=mods::handle(&profile,&json!({"id":g["LibraryId"],"action":"get"})).unwrap();
    assert_eq!(data["unsupported"],true);
    assert!(data["available"]["thunderstore"].is_array());
    assert!(mods::handle(&profile,&json!({"id":g["LibraryId"],"action":"install"})).is_err());
}
#[test]
fn duplicates_keep_metadata_and_collection_membership() {
    let (_, profile, g) = area();
    let mut second = g.clone();
    second["LibraryId"] = json!(id());
    second["IsPinned"] = json!(true);
    second["PlayedSeconds"] = json!(120.);
    library::save(&profile, &[g.clone(), second.clone()]).unwrap();
    write(
        &profile.join("collections.json"),
        &json!({"Names":["Friends"],"Members":{str(&second,"LibraryId"):["Friends"]}}),
    )
    .unwrap();
    workspace::handle(
        &profile,
        &json!({"area":"duplicates","action":"merge","id":g["LibraryId"]}),
    )
    .unwrap();
    let games = library::load(&profile.join("games.json")).unwrap().0;
    assert_eq!(games.len(), 1);
    assert_eq!(games[0]["PlayedSeconds"], 120.);
    assert_eq!(games[0]["futureField"]["keep"], true);
    assert_eq!(
        read(&profile.join("collections.json"), Value::Null).unwrap()["Members"]
            [str(&g, "LibraryId")][0],
        "Friends"
    );
    workspace::handle(&profile,&json!({"area":"duplicates","action":"undo_merge"})).unwrap();
    let restored = library::load(&profile.join("games.json")).unwrap().0;
    assert_eq!(restored.len(),2);
    assert!(restored.contains(&second));
    assert_eq!(restored[0]["PlayedSeconds"],g["PlayedSeconds"]);
}

#[test]
fn portable_build_resolves_exact_versions_and_requires_matching_review() {
    let (root, profile, mut game) = area();
    game["SteamAppId"] = json!("1966720");
    library::save(&profile,&[game.clone()]).unwrap();
    write(&profile.join("preferences.json"),&json!({"OfflineMode":true})).unwrap();
    write(&profile.join("mod-catalog/lethal-company.json"),&json!([
        {"owner":"Tester","name":"Sample","versions":[{"version_number":"1.0.0","dependencies":["BepInEx-BepInExPack-5.4.22"]}]},
        {"owner":"BepInEx","name":"BepInExPack","versions":[{"version_number":"5.4.22","dependencies":[]}]}
    ])).unwrap();
    let manifest=root.join("build.json");
    write(&manifest,&json!({"schema":1,"community":"lethal-company","packages":[{"id":"Tester-Sample","version":"1.0.0"}]})).unwrap();
    let plan=mods::handle(&profile,&json!({"id":game["LibraryId"],"action":"build_plan","path":manifest})).unwrap();
    assert_eq!(plan[0]["id"],"BepInEx-BepInExPack");
    assert_eq!(plan[1]["version"],"1.0.0");
    let error=mods::handle(&profile,&json!({"id":game["LibraryId"],"action":"build_install","path":manifest,"confirmed":true,"reviewed":[]})).unwrap_err();
    assert!(error.contains("Review its plan"));
    assert!(!root.join("game/.playspace-mods/installed.json").exists());
}

#[test]
fn profiles_switch_owned_files_without_touching_game_files() {
    let (root, profile, g) = area();
    let archive = root.join("profile-mod.zip");
    zip(&archive, "profile.archive", b"profile data");
    mods::handle(&profile,&json!({"id":g["LibraryId"],"kind":"native","action":"import","path":archive,"confirmed":true})).unwrap();
    let saved = mods::handle(
        &profile,
        &json!({"id":g["LibraryId"],"kind":"native","action":"profile_save","name":"My profile"}),
    )
    .unwrap();
    let state = mods::handle(
        &profile,
        &json!({"id":g["LibraryId"],"kind":"native","action":"get"}),
    )
    .unwrap();
    let package = state["installed"]["Packages"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap();
    mods::handle(
        &profile,
        &json!({"id":g["LibraryId"],"kind":"native","action":"uninstall","package":package}),
    )
    .unwrap();
    let point=mods::handle(&profile,&json!({"id":g["LibraryId"],"kind":"native","action":"profile_activate","profile":saved["id"]})).unwrap();
    assert_eq!(point["changes"].as_array().unwrap().len(), 1);
    mods::handle(&profile,&json!({"id":g["LibraryId"],"kind":"native","action":"profile_activate","profile":saved["id"],"confirmed":true})).unwrap();
    let path = PathBuf::from(g["InstallDirectory"].as_str().unwrap())
        .join("archive/pc/mod/profile.archive");
    assert_eq!(fs::read(path).unwrap(), b"profile data");
    assert_eq!(
        fs::read(g["ExePath"].as_str().unwrap()).unwrap(),
        b"not executable, file-operation fixture only"
    );
}

#[test]
fn transfer_verifies_copy_and_retains_original_and_metadata() {
    let (root, profile, g) = area();
    let destination = root.join("destination");
    fs::create_dir(&destination).unwrap();
    let result = crate::transfer::handle(
        &profile,
        &json!({"id":g["LibraryId"],"action":"copy","destination":destination,"confirmed":true}),
    )
    .unwrap();
    assert_eq!(result["sourceRetained"], true);
    let updated = library::load(&profile.join("games.json"))
        .unwrap()
        .0
        .remove(0);
    assert_eq!(updated["futureField"]["keep"], true);
    assert_eq!(
        hash(Path::new(g["ExePath"].as_str().unwrap())).unwrap(),
        hash(Path::new(updated["ExePath"].as_str().unwrap())).unwrap()
    );
    assert!(crate::transfer::handle(
        &profile,
        &json!({"id":updated["LibraryId"],"action":"preview","destination":destination})
    )
    .is_err());
}

#[test]
fn unreal_graphics_keep_unknown_settings_and_restore_original() {
    let (root, profile, mut g) = area();
    g["SteamAppId"] = json!("1623730");
    library::save(&profile, &[g.clone()]).unwrap();
    let path = root.join("GameUserSettings.ini");
    let original=b"[/Script/Engine.GameUserSettings]\nResolutionSizeX=1280\nUserCustomSetting=KeepMe\n\n[Other]\nFoo=Bar\n";
    fs::write(&path, original).unwrap();
    crate::graphics::handle(&profile,&json!({"id":g["LibraryId"],"action":"save","value":{"ConfigFile":path,"ResolutionWidth":1920,"ResolutionHeight":1080,"TargetFps":90,"RenderScale":85,"Quality":"Высокие","WindowMode":"Безрамочный"}})).unwrap();
    crate::graphics::handle(&profile, &json!({"id":g["LibraryId"],"action":"apply"})).unwrap();
    let changed = fs::read_to_string(&path).unwrap();
    assert!(changed.contains("ResolutionSizeX=1920"));
    assert!(changed.contains("UserCustomSetting=KeepMe"));
    assert!(changed.contains("Foo=Bar"));
    assert!(changed.contains("FrameRateLimit=90"));
    crate::graphics::handle(&profile, &json!({"id":g["LibraryId"],"action":"restore"})).unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn cyberpunk_native_integer_options_remain_integers() {
    let (root, profile, g) = area();
    let path = root.join("UserSettings.json");
    let input = json!({"data":[{"group_name":"/graphics","options":[{"name":"TestSetting","type":"int","value":1,"min_value":0,"max_value":3}]}],"unrelated":{"preserve":true}});
    write(&path, &input).unwrap();
    crate::graphics::handle(&profile,&json!({"id":g["LibraryId"],"action":"save","value":{"ConfigFile":path,"NativeOptions":{"/graphics|TestSetting":"2"}}})).unwrap();
    crate::graphics::handle(&profile, &json!({"id":g["LibraryId"],"action":"apply"})).unwrap();
    let changed = read(&path, Value::Null).unwrap();
    assert_eq!(changed["data"][0]["options"][0]["value"], 2);
    assert!(changed["data"][0]["options"][0]["value"].is_i64());
    assert_eq!(changed["unrelated"]["preserve"], true);
}

#[test]
fn invalid_admin_grants_never_unlock_review() {
    let (root, profile, _) = area();
    let grant = root.join("grant.json");
    fs::write(&grant, b"{\"Payload\":\"e30=\",\"Signature\":\"AA==\"}").unwrap();
    assert!(crate::admin::handle(&profile, &json!({"action":"import","path":grant})).is_err());
    assert!(!profile.join("admin-access.json").exists());
    assert!(crate::admin::handle(&profile, &json!({"action":"review","path":grant})).is_err());
}

#[test]
fn invalid_pe_and_wav_files_are_rejected_without_execution() {
    let (root, _, _) = area();
    let path = root.join("fake.dll");
    fs::write(&path, b"MZfake").unwrap();
    assert!(pe64(&path, true).is_err());
    let wav = root.join("fake.wav");
    fs::write(&wav, b"RIFFno-pcm-data").unwrap();
    assert!(crate::sounds::validate(&wav).is_err());
}
