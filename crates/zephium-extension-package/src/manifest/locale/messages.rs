//! Closed parsing for authenticated Chrome locale-message documents.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::ExtensionDefaultLocaleResolutionError;
use crate::manifest::metadata::valid_localized_message_name;
use crate::{parse_bounded_json, BoundedJsonLimits};

#[derive(Debug)]
pub(super) struct ParsedMessage {
    pub(super) message: String,
}

pub(super) fn parse_messages(
    bytes: &[u8],
    needed: &BTreeSet<&str>,
) -> Result<BTreeMap<String, ParsedMessage>, ExtensionDefaultLocaleResolutionError> {
    let bounded = parse_bounded_json(bytes, BoundedJsonLimits::extension_locale_messages())
        .map_err(ExtensionDefaultLocaleResolutionError::Json)?;
    let root = match bounded.into_value() {
        Value::Object(root) => root,
        _ => return Err(ExtensionDefaultLocaleResolutionError::RootNotObject),
    };
    let mut seen = BTreeSet::new();
    let mut selected = BTreeMap::new();
    for (source_key, value) in root {
        if !valid_localized_message_name(&source_key, true) {
            return Err(ExtensionDefaultLocaleResolutionError::InvalidMessageKey(
                source_key.into_boxed_str(),
            ));
        }
        let lookup_key = source_key.to_ascii_lowercase();
        if !seen.insert(lookup_key.clone()) {
            return Err(ExtensionDefaultLocaleResolutionError::AmbiguousMessageKey(
                lookup_key.into_boxed_str(),
            ));
        }
        let message = parse_message_entry(&source_key, value)?;
        if needed.contains(lookup_key.as_str()) {
            selected.insert(lookup_key, message);
        }
    }
    for key in needed {
        if !selected.contains_key(*key) {
            return Err(ExtensionDefaultLocaleResolutionError::MissingMessage(
                (*key).into(),
            ));
        }
    }
    Ok(selected)
}

fn parse_message_entry(
    key: &str,
    value: Value,
) -> Result<ParsedMessage, ExtensionDefaultLocaleResolutionError> {
    let mut object = match value {
        Value::Object(object) => object,
        _ => return Err(invalid_entry(key)),
    };
    let message = object
        .remove("message")
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| invalid_entry(key))?;
    if object
        .remove("description")
        .is_some_and(|value| !value.is_string())
    {
        return Err(invalid_entry(key));
    }
    let placeholders = object
        .remove("placeholders")
        .map(|value| parse_placeholders(key, value))
        .transpose()?
        .unwrap_or_default();
    // Chromium ignores other message-entry metadata. It remains bounded and
    // duplicate-key checked by the JSON parser, but cannot affect display.
    if !valid_message_template(&message, &placeholders) {
        return Err(invalid_entry(key));
    }
    Ok(ParsedMessage { message })
}

fn parse_placeholders(
    message_key: &str,
    value: Value,
) -> Result<BTreeSet<String>, ExtensionDefaultLocaleResolutionError> {
    let placeholders = match value {
        Value::Object(placeholders) => placeholders,
        _ => return Err(invalid_placeholder(message_key)),
    };
    let mut normalized = BTreeSet::new();
    for (source_name, value) in placeholders {
        if !valid_localized_message_name(&source_name, false) {
            return Err(invalid_placeholder(message_key));
        }
        let name = source_name.to_ascii_lowercase();
        if !normalized.insert(name) {
            return Err(invalid_placeholder(message_key));
        }
        let mut descriptor = match value {
            Value::Object(descriptor) => descriptor,
            _ => return Err(invalid_placeholder(message_key)),
        };
        let content = descriptor
            .remove("content")
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or_else(|| invalid_placeholder(message_key))?;
        if descriptor
            .remove("example")
            .is_some_and(|value| !value.is_string())
            || !descriptor.is_empty()
            || !valid_placeholder_content(&content)
        {
            return Err(invalid_placeholder(message_key));
        }
    }
    Ok(normalized)
}

fn valid_message_template(message: &str, placeholders: &BTreeSet<String>) -> bool {
    let bytes = message.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'$' {
            cursor += 1;
            continue;
        }
        let Some(next) = bytes.get(cursor + 1).copied() else {
            return false;
        };
        if next == b'$' || matches!(next, b'1'..=b'9') {
            cursor += 2;
            continue;
        }
        let Some(relative_end) = bytes[cursor + 1..].iter().position(|byte| *byte == b'$') else {
            return false;
        };
        let end = cursor + 1 + relative_end;
        let Ok(name) = std::str::from_utf8(&bytes[cursor + 1..end]) else {
            return false;
        };
        if !valid_localized_message_name(name, false)
            || !placeholders.contains(&name.to_ascii_lowercase())
        {
            return false;
        }
        cursor = end + 1;
    }
    true
}

fn valid_placeholder_content(content: &str) -> bool {
    let bytes = content.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'$' {
            cursor += 1;
            continue;
        }
        match bytes.get(cursor + 1).copied() {
            Some(b'$' | b'1'..=b'9') => cursor += 2,
            _ => return false,
        }
    }
    true
}

fn invalid_entry(key: &str) -> ExtensionDefaultLocaleResolutionError {
    ExtensionDefaultLocaleResolutionError::InvalidMessageEntry(key.into())
}

fn invalid_placeholder(key: &str) -> ExtensionDefaultLocaleResolutionError {
    ExtensionDefaultLocaleResolutionError::InvalidPlaceholder(key.into())
}
