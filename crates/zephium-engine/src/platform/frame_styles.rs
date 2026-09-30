//! Narrow immutable stylesheet delivery shared by native frame adapters.
use sha2::{Digest, Sha256};
use std::rc::Rc;
use std::sync::Arc;

pub(crate) struct FrameStyleData {
    pub generation: u64,
    pub subscription: Arc<str>,
    pub personal: Arc<str>,
}
pub(crate) type FrameStyleLookup = Rc<dyn Fn(&str) -> Option<FrameStyleData>>;
pub(crate) const MAX_STYLE_FRAMES: usize = 64;
pub(crate) const INSPECT_STYLE_DOCUMENT:&str="(()=>{const a=globalThis.__zephium_content_style_v1__;return a&&a.version===1?a.inspectEncoded():null})()";

pub(crate) fn style_script(data: &FrameStyleData, token: &str, url: &str) -> Option<String> {
    if token.len() != 32
        || !token.bytes().all(|b| b.is_ascii_hexdigit())
        || url.len() > 32768
        || data.subscription.len() > 1024 * 1024
        || data.personal.len() > 1024 * 1024
    {
        return None;
    }
    let hash = |value: &str| -> String {
        use std::fmt::Write;
        let mut out = String::with_capacity(64);
        for b in Sha256::digest(value.as_bytes()) {
            let _ = write!(out, "{b:02x}");
        }
        out
    };
    let args = serde_json::json!([
        token,
        url,
        format!("{:016x}", data.generation),
        hash(&data.subscription),
        data.subscription.as_ref(),
        hash(&data.personal),
        data.personal.as_ref()
    ]);
    Some(format!("((p)=>{{const a=globalThis.__zephium_content_style_v1__;if(!a)return false;const s=a.apply('subscription',p[0],p[1],p[2],p[3],p[4]);const u=a.apply('personal',p[0],p[1],p[2],p[5],p[6]);return s===true&&u===true;}})({args})"))
}
pub(crate) fn document_identity(encoded: &str) -> Option<(String, String)> {
    if encoded.len() > 36 * 1024 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(encoded).ok()?;
    let object = value.as_object()?;
    if object.len() != 2 {
        return None;
    }
    let token = object.get("token")?.as_str()?;
    let url = object.get("url")?.as_str()?;
    if token.len() != 32 || !token.bytes().all(|b| b.is_ascii_hexdigit()) || url.len() > 32768 {
        return None;
    }
    Some((token.into(), url.into()))
}
