//! Compile-time identity for the ordinary-controller development witness.
pub fn validate(config: &serde_json::Value) -> std::io::Result<()> {
    if config["identifier"] != "app.zephium.work-navigation-probe"
        || config["productName"] != "Zephium Work Navigation Probe"
        || config
            .pointer("/app/windows/0/title")
            .and_then(serde_json::Value::as_str)
            != Some("Zephium Work Navigation Probe")
        || config.pointer("/build/devUrl") != Some(&serde_json::Value::Null)
    {
        return Err(std::io::Error::other(
            "navigation qualification requires its exact isolated bundled identity",
        ));
    }
    Ok(())
}
