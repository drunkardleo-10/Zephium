//! Read-only backend payload validation, without signing or product authority.

use std::io::Read as _;
use std::path::Path;

use zephium_extension_package::{ExtensionPublicPolicy, MAX_EXTENSION_PUBLIC_POLICY_BYTES};

pub(crate) fn check(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|_| "cannot open extension policy file")?;
    let metadata = file
        .metadata()
        .map_err(|_| "cannot inspect extension policy file")?;
    if !metadata.is_file() || metadata.len() > MAX_EXTENSION_PUBLIC_POLICY_BYTES as u64 {
        return Err("extension policy input is not a bounded regular file".to_owned());
    }
    let mut bytes = Vec::new();
    file.take((MAX_EXTENSION_PUBLIC_POLICY_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read bounded extension policy")?;
    let policy = ExtensionPublicPolicy::parse(&bytes).map_err(|error| error.to_string())?;
    Ok(serde_json::json!({
        "schema": "zephium.extension-public-policy.check.v1",
        "bytes": bytes.len(),
        "sha256": policy.sha256().iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
        "channel": policy.channel(),
        "policy_revision": policy.revision(),
        "beta_targets": policy.beta_targets().len(),
        "recommendations": policy.recommendations().len(),
        "revocations": policy.revocations().len(),
        "signature_verified": false,
        "product_authority": false,
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validator_reports_structure_without_promoting_authentication() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("policy.json");
        std::fs::write(&path, br#"{"schema_version":1,"policy_revision":1,"channel":"staging","issued_unix":100,"expires_unix":200,"beta_targets":[],"recommendations":[],"revocations":[]}"#).unwrap();
        let result: serde_json::Value = serde_json::from_str(&check(&path).unwrap()).unwrap();
        assert_eq!(result["signature_verified"], false);
        assert_eq!(result["product_authority"], false);
        assert_eq!(result["policy_revision"], 1);
        std::fs::write(&path, br#"{"schema_version":1,"schema_version":1}"#).unwrap();
        assert!(check(&path).is_err());
    }
}
