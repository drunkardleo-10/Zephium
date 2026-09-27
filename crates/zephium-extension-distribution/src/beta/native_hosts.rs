//! Publisher identity bindings for local native-app connections. This table
//! grants neither install authority nor access to an arbitrary executable.
//! Compatibility selection remains package-neutral; only host authorization
//! binds a publisher key to an inspected native application's signing identity.

use zephium_core::extensions::{
    ExtensionMacosPublisherIdentity, ExtensionPackageIdentity,
    ExtensionPublisherNativeHostRequirement,
};

struct Binding {
    publisher: [u8; 32],
    extension_id: &'static str,
    host: &'static str,
    team: &'static str,
    signing_id: &'static str,
}

// Public identities from the authenticated original CRX and the previously
// exercised, code-signature-verified publisher helper. No version/package
// approval list, host path, signing key, or executable is shipped here.
const BINDINGS: &[Binding] = &[Binding {
    publisher: [
        0x04, 0x1b, 0x53, 0xa7, 0x77, 0x32, 0x39, 0xf8, 0x57, 0x71, 0x38, 0xe9, 0xfb, 0x59, 0xd2,
        0xe0, 0x25, 0x99, 0xf3, 0x58, 0xbd, 0x24, 0x09, 0x4b, 0x9e, 0x87, 0x54, 0x02, 0xb0, 0xcc,
        0x79, 0x27,
    ],
    extension_id: "aeblfdkhhhdcdjpifhhbdiojplfjncoa",
    host: "com.1password.1password",
    team: "2BUA8C4S2C",
    signing_id: "com.1password.browser-support",
}];

fn binding(publisher: [u8; 32]) -> Option<&'static Binding> {
    BINDINGS.iter().find(|entry| entry.publisher == publisher)
}

pub(super) fn supports(publisher: [u8; 32]) -> bool {
    binding(publisher).is_some()
}

pub(super) fn policy_digest(publisher: [u8; 32]) -> Option<[u8; 32]> {
    use sha2::{Digest, Sha256};
    let entry = binding(publisher)?;
    let mut hash = Sha256::new();
    hash.update(b"zephium:local-native-host-binding:v1\0");
    hash.update(entry.publisher);
    for value in [entry.extension_id, entry.host, entry.team, entry.signing_id] {
        hash.update((value.len() as u32).to_le_bytes());
        hash.update(value.as_bytes());
    }
    Some(hash.finalize().into())
}

pub(super) fn requirement(
    package: &ExtensionPackageIdentity,
) -> Option<ExtensionPublisherNativeHostRequirement> {
    let entry = binding(package.key().bytes())?;
    ExtensionPublisherNativeHostRequirement::new(
        package.clone(),
        entry.host,
        entry.extension_id,
        ExtensionMacosPublisherIdentity::new(entry.team, entry.signing_id).ok()?,
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_complete_authenticated_publisher_keys_match() {
        assert!(supports(BINDINGS[0].publisher));
        let mut altered = BINDINGS[0].publisher;
        altered[31] ^= 1;
        assert!(!supports(altered));
        assert!(!supports([0; 32]));
        for entry in BINDINGS {
            let id: String = entry.publisher[..16]
                .iter()
                .flat_map(|byte| {
                    [
                        char::from(b'a' + (byte >> 4)),
                        char::from(b'a' + (byte & 15)),
                    ]
                })
                .collect();
            assert_eq!(id, entry.extension_id);
            assert!(ExtensionMacosPublisherIdentity::new(entry.team, entry.signing_id).is_ok());
        }
    }
}
