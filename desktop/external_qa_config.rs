//! External QA isolates identity and excludes fixture authority. Compiler
//! optimization/debug assertions are not that isolation boundary.
use serde_json::Value;

pub fn validate(config: Option<&Value>, incompatible_mode: bool) -> Result<(), &'static str> {
    if incompatible_mode {
        return Err(
            "external extension QA cannot include staging, lab or rendering-probe authority",
        );
    }
    let config = config.ok_or("external extension QA requires its isolated configuration")?;
    if config.get("identifier").and_then(Value::as_str) != Some("app.zephium.external-qa")
        || config.get("productName").and_then(Value::as_str) != Some("Zephium Extension QA")
        || config
            .pointer("/app/windows/0/title")
            .and_then(Value::as_str)
            != Some("Zephium Extension QA")
        || config.pointer("/app/windows/0/url").and_then(Value::as_str) != Some("browser.html")
    {
        return Err("external extension QA requires its exact separate application identity");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Value {
        serde_json::json!({"identifier":"app.zephium.external-qa", "productName":"Zephium Extension QA",
            "app":{"windows":[{"title":"Zephium Extension QA","url":"browser.html"}]}})
    }
    #[test]
    fn qa_identity_is_independent_of_compiler_profile_but_never_fixture_authority() {
        assert!(validate(Some(&config()), false).is_ok());
        assert!(validate(Some(&config()), true).is_err());
        assert!(validate(None, false).is_err());
    }
    #[test]
    fn qa_cannot_alias_the_shipping_application_or_mix_identity_fields() {
        for (pointer, replacement) in [
            ("/identifier", "app.zephium"),
            ("/productName", "Zephium"),
            ("/app/windows/0/title", "Zephium"),
            ("/app/windows/0/url", "index.html"),
        ] {
            let mut value = config();
            *value.pointer_mut(pointer).unwrap() = Value::String(replacement.into());
            assert!(validate(Some(&value), false).is_err());
        }
    }
}
