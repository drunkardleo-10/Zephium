#[path = "../foreground_probe_config.rs"]
mod contract;

#[test]
fn rendering_probe_cannot_use_real_application_identity_or_a_development_server() {
    let valid: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.work-rendering-probe.conf.json")).unwrap();
    contract::validate(&valid).unwrap();
    for (pointer, value) in [
        ("/identifier", serde_json::json!("app.zephium")),
        ("/productName", serde_json::json!("Zephium")),
        ("/app/windows/0/title", serde_json::json!("Zephium")),
        ("/build/devUrl", serde_json::json!("http://localhost:1420")),
    ] {
        let mut changed = valid.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(contract::validate(&changed).is_err());
    }
    let mut missing = valid;
    missing.as_object_mut().unwrap().remove("build");
    assert!(contract::validate(&missing).is_err());
}

#[test]
fn rendering_probe_refuses_prior_session_or_profile_data() {
    let empty = tempfile::tempdir().unwrap();
    contract::require_fresh_data_root(empty.path()).unwrap();
    contract::require_fresh_data_root(&empty.path().join("absent")).unwrap();
    std::fs::create_dir(empty.path().join("prior-session")).unwrap();
    assert!(contract::require_fresh_data_root(empty.path()).is_err());
}
