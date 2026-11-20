use super::*;
use rsa::{pkcs1v15::SigningKey, signature::{Signer,SignatureEncoding}, RsaPrivateKey};
fn key() -> &'static RsaPrivateKey {
    static KEY: OnceLock<RsaPrivateKey> = OnceLock::new();
    KEY.get_or_init(|| RsaPrivateKey::new(&mut rsa::rand_core::OsRng,1024).unwrap())
}
fn fixture() -> Value {
    let now=chrono::Utc::now();
    json!({"schemaVersion":1,"revision":1,"minClientVersion":"2.0.0","maxClientVersion":null,"issuedAt":(now-chrono::Duration::minutes(1)).to_rfc3339(),"expiresAt":(now+chrono::Duration::days(1)).to_rfc3339(),"config":serde_json::from_str::<Value>(include_str!("../../../remote/config.json")).unwrap(),"announcements":[],"translations":{"ru":{},"en":{}},"catalogs":{"recommendations":null,"modAdapters":null}})
}
fn envelope(p: &Value) -> Vec<u8> {
    let bytes=serde_json::to_vec(p).unwrap();let mut signed=DOMAIN.to_vec();signed.extend_from_slice(&bytes);
    let sig=SigningKey::<Sha256>::new(key().clone()).sign(&signed);
    serde_json::to_vec(&json!({"payload":STANDARD.encode(bytes),"signature":STANDARD.encode(sig.to_bytes())})).unwrap()
}
fn check(p: &Value) -> Result<(Value,String)> { verify_with_key(&envelope(p),&key().to_public_key(),chrono::Utc::now(),false) }
#[test] fn signed_bundle_verifies_and_modified_payload_fails() {
    let p=fixture();assert!(check(&p).is_ok());
    let mut e:Value=serde_json::from_slice(&envelope(&p)).unwrap();let mut altered=p.clone();altered["revision"]=json!(2);e["payload"]=json!(STANDARD.encode(serde_json::to_vec(&altered).unwrap()));
    assert!(verify_with_key(&serde_json::to_vec(&e).unwrap(),&key().to_public_key(),chrono::Utc::now(),false).is_err());
}
#[test] fn another_signing_key_is_rejected() {
    let other=RsaPrivateKey::new(&mut rsa::rand_core::OsRng,1024).unwrap();
    assert!(verify_with_key(&envelope(&fixture()),&other.to_public_key(),chrono::Utc::now(),false).is_err());
}
#[test] fn expired_and_future_bundles_are_rejected() {
    let mut p=fixture();p["issuedAt"]=json!((chrono::Utc::now()-chrono::Duration::days(3)).to_rfc3339());p["expiresAt"]=json!((chrono::Utc::now()-chrono::Duration::days(1)).to_rfc3339());assert!(check(&p).is_err());
    assert!(verify_with_key(&envelope(&p),&key().to_public_key(),chrono::Utc::now(),true).is_ok());
    p["issuedAt"]=json!((chrono::Utc::now()+chrono::Duration::hours(1)).to_rfc3339());p["expiresAt"]=json!((chrono::Utc::now()+chrono::Duration::days(2)).to_rfc3339());assert!(check(&p).is_err());
}
#[test] fn schema_and_client_compatibility_are_enforced() {
    for (field,value) in [("schemaVersion",json!(2)),("minClientVersion",json!("99.0.0")),("maxClientVersion",json!("1.0.0")),("revision",json!(0))] {let mut p=fixture();p[field]=value;assert!(check(&p).is_err(),"{field}");}
}
#[test] fn unsafe_project_links_and_fields_are_rejected() {
    for url in ["http://auth.robokassa.ru/merchant/Invoice/a","https://auth.robokassa.ru.attacker.test/merchant/Invoice/a","https://auth.robokassa.ru@evil.test/merchant/Invoice/a","https://auth.robokassa.ru:444/merchant/Invoice/a","file:///C:/Windows/a","javascript:alert(1)","https://auth.robokassa.ru/merchant/Invoice/a?redirect=bad"] {
        let mut p=fixture();p["config"]["links"]["donate"]["ru"]=json!(url);assert!(check(&p).is_err(),"{url}");
    }
    let mut p=fixture();p["config"]["command"]=json!("powershell.exe");assert!(check(&p).is_err());
    p=fixture();p["config"]["features"]["admin"]=json!(true);assert!(check(&p).is_err());
}
#[test] fn bounded_timing_and_colours_are_enforced() {
    for (field,value) in [("refreshSeconds",0),("workshopCacheSeconds",100000)] {let mut p=fixture();p["config"]["timing"][field]=json!(value);assert!(check(&p).is_err());}
    let mut p=fixture();p["config"]["branding"]["accent"]=json!("red; background:url(evil)");assert!(check(&p).is_err());
}
#[test] fn recommendations_reject_duplicate_ids_and_code_fields() {
    let mut p=fixture();let row=json!({"id":"620","name":"Portal 2","categories":["Кооп"]});p["catalogs"]["recommendations"]=json!([row.clone()]);assert!(check(&p).is_ok());
    p["catalogs"]["recommendations"]=json!([row.clone(),row]);assert!(check(&p).is_err());
    p["catalogs"]["recommendations"]=json!([{"id":"620","name":"Portal 2","categories":[],"command":"bad"}]);assert!(check(&p).is_err());
}
#[test] fn mod_labels_can_change_but_install_paths_cannot() {
    let mut p=fixture();let base:Value=serde_json::from_str(include_str!("../../catalog/mod-adapters.json")).unwrap();p["catalogs"]["modAdapters"]=base;
    p["catalogs"]["modAdapters"]["native"][0]["Name"]=json!("New label");assert!(check(&p).is_ok());
    p["catalogs"]["modAdapters"]["native"][0]["Folder"]=json!("../Startup");assert!(check(&p).is_err());
}
#[test] fn announcements_are_scheduled_and_no_arbitrary_urls_allowed() {
    let mut p=fixture();p["announcements"]=json!([{"id":"news","title":{"ru":"Новости","en":"News"},"body":{"ru":"Текст","en":"Text"},"startsAt":p["issuedAt"],"endsAt":p["expiresAt"],"link":"news"}]);assert!(check(&p).is_ok());
    p["announcements"][0]["link"]=json!("https://evil.test");assert!(check(&p).is_err());
}
#[test] fn embedded_fallback_and_offline_mode_do_not_need_network() {
    let root=std::env::temp_dir().join(format!("playspace-remote-{}",id()));
    write(&root.join("preferences.json"),&json!({"OfflineMode":true})).unwrap();
    let initial=view(&root);assert_eq!(initial["status"]["source"],"embedded");assert!(link(&root,"donate","en").unwrap().contains("KL10bltZtkCT1pfk7R0EVQ"));
    assert_eq!(refresh(&root,true)["status"]["source"],"embedded");
    assert!(!cache(&root).exists());let _=fs::remove_dir_all(root);
}
#[test] fn corrupt_cache_is_not_used() {
    let root=std::env::temp_dir().join(format!("playspace-remote-{}",id()));atomic(&cache(&root),b"{bad json").unwrap();
    assert_eq!(view(&root)["status"]["source"],"embedded");let _=fs::remove_dir_all(root);
}
#[test] fn revision_rollback_and_reuse_are_rejected() {
    assert!(check_revision(5,"abc",4,"def").is_err());
    assert!(check_revision(5,"abc",5,"def").is_err());
    assert!(check_revision(5,"abc",5,"abc").is_ok());
    assert!(check_revision(5,"abc",6,"def").is_ok());
}
