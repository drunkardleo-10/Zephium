//! Conversion between WebKit's Foundation message objects and JSON values.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{
    NSArray, NSData, NSJSONReadingOptions, NSJSONSerialization, NSJSONWritingOptions, NSNull,
};
use serde_json::Value;

pub(crate) fn from_object(object: Option<&AnyObject>) -> Value {
    let Some(object) = object else {
        return Value::Null;
    };
    // Wrapping admits top-level strings and numbers, and the validity check
    // matters: serializing an invalid object raises instead of failing.
    let wrapped = NSArray::from_slice(&[object]);
    if !unsafe { NSJSONSerialization::isValidJSONObject(&wrapped) } {
        return Value::Null;
    }
    let data = unsafe {
        NSJSONSerialization::dataWithJSONObject_options_error(
            &wrapped,
            NSJSONWritingOptions::empty(),
        )
    };
    let Ok(data) = data else {
        return Value::Null;
    };
    match serde_json::from_slice::<Value>(&data.to_vec()) {
        Ok(Value::Array(mut items)) if items.len() == 1 => items.pop().unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

pub(crate) fn to_object(value: &Value) -> Retained<AnyObject> {
    let bytes = serde_json::to_vec(value).unwrap_or_else(|_| b"null".to_vec());
    let data = NSData::with_bytes(&bytes);
    NSJSONSerialization::JSONObjectWithData_options_error(
        &data,
        NSJSONReadingOptions::FragmentsAllowed,
    )
    .unwrap_or_else(|_| null())
}

pub(crate) fn null() -> Retained<AnyObject> {
    Retained::into_super(Retained::into_super(NSNull::null()))
}
