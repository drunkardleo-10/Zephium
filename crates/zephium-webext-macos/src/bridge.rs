//! Native half of the compatibility layer: `runtime.sendNativeMessage` to
//! [`APPLICATION`] with `{ api, ... }` lands here.

use std::rc::Rc;

use block2::DynBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::NSError;
use objc2_web_kit::WKWebExtensionContext;
use serde_json::Value;

use crate::runtime::Shared;
use crate::{error, json, LogLevel};

pub(crate) const APPLICATION: &str = "app.zephium.webext";

pub(crate) fn handle(
    shared: &Rc<Shared>,
    context: &WKWebExtensionContext,
    message: &AnyObject,
    reply: &DynBlock<dyn Fn(*mut AnyObject, *mut NSError)>,
) {
    let message = json::from_object(Some(message));
    let result = match message.get("api").and_then(Value::as_str) {
        Some("log") => {
            let level = match message.get("level").and_then(Value::as_str) {
                Some("error") => LogLevel::Error,
                Some("warning") => LogLevel::Warning,
                _ => LogLevel::Info,
            };
            let text = message
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let text: String = text.chars().take(2000).collect();
            shared.log(context, level, &text);
            Ok(Value::Null)
        }
        Some("notify") => {
            let title = message
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default();
            shared.log(context, LogLevel::Info, &format!("notification: {title}"));
            Ok(Value::Null)
        }
        Some(api) => Err(format!("{api} is not supported")),
        None => Err("missing api".to_owned()),
    };
    match result {
        Ok(value) => {
            let object = json::to_object(&value);
            reply.call((Retained::as_ptr(&object).cast_mut(), std::ptr::null_mut()));
        }
        Err(message) => {
            let error = error(&message);
            reply.call((std::ptr::null_mut(), Retained::as_ptr(&error).cast_mut()));
        }
    }
}
