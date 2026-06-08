use serde::de::{self, Deserializer, Error};
use serde::Deserialize;
use serde_json::Value;

pub fn deserialize_u32_lenient<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    match Value::deserialize(deserializer)? {
        Value::Number(number) => number
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| Error::custom("expected u32")),
        Value::String(text) => text.trim().parse::<u32>().map_err(Error::custom),
        _ => Err(Error::custom("expected u32 or numeric string")),
    }
}

pub fn deserialize_option_u32_lenient<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<Value>::deserialize(deserializer)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| Error::custom("expected u32")),
        Some(Value::String(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
                Ok(None)
            } else {
                trimmed.parse::<u32>().map(Some).map_err(Error::custom)
            }
        }
        Some(_) => Err(Error::custom("expected u32 or numeric string")),
    }
}

pub fn deserialize_vec_u32_lenient<'de, D>(deserializer: D) -> Result<Vec<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    let values = Vec::<Value>::deserialize(deserializer)?;
    values
        .into_iter()
        .map(|value| match value {
            Value::Number(number) => number
                .as_u64()
                .and_then(|parsed| u32::try_from(parsed).ok())
                .ok_or_else(|| Error::custom("expected u32 in array")),
            Value::String(text) => text.trim().parse::<u32>().map_err(Error::custom),
            _ => Err(Error::custom("expected u32 or numeric string in array")),
        })
        .collect()
}

pub fn deserialize_option_f32_lenient<'de, D>(deserializer: D) -> Result<Option<f32>, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<Value>::deserialize(deserializer)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_f64()
            .map(|value| value as f32)
            .map(Some)
            .ok_or_else(|| Error::custom("expected f32")),
        Some(Value::String(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
                Ok(None)
            } else {
                trimmed.parse::<f32>().map(Some).map_err(Error::custom)
            }
        }
        Some(_) => Err(Error::custom("expected f32 or numeric string")),
    }
}

pub fn deserialize_bool_lenient<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    bool_from_value(Value::deserialize(deserializer)?).map_err(Error::custom)
}

pub fn deserialize_option_bool_lenient<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<Value>::deserialize(deserializer)? {
        None | Some(Value::Null) => Ok(None),
        Some(value) => bool_from_value(value).map(Some).map_err(Error::custom),
    }
}

pub fn normalize_append_blocks_args(value: &mut Value) {
    if let Some(object) = value.as_object_mut() {
        if let Some(blocks) = object.get_mut("blocks") {
            normalize_blocks_array(blocks);
        }
    }
}

pub fn normalize_document(value: &mut Value) {
    if let Some(object) = value.as_object_mut() {
        if let Some(blocks) = object.get_mut("blocks") {
            normalize_blocks_array(blocks);
        }
    }
}

fn normalize_blocks_array(blocks: &mut Value) {
    let Some(items) = blocks.as_array_mut() else {
        return;
    };
    for block in items {
        normalize_block_value(block);
    }
}

fn normalize_block_value(block: &mut Value) {
    let Some(object) = block.as_object_mut() else {
        return;
    };
    let block_type = object
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_string);
    match block_type.as_deref() {
        Some("heading") => coerce_field_to_u32(object, "level"),
        Some("list") => {
            coerce_field_to_bool(object, "ordered");
            coerce_optional_field_to_u32(object, "start");
            if let Some(items) = object.get_mut("items").and_then(Value::as_array_mut) {
                for item in items {
                    normalize_list_item(item);
                }
            }
        }
        Some("code") => {
            coerce_field_to_bool(object, "show_line_numbers");
            coerce_array_field_to_u32(object, "highlight_lines");
        }
        Some("math") => coerce_field_to_bool(object, "display"),
        Some("image") => {
            coerce_optional_field_to_f32(object, "width_pt");
            coerce_optional_field_to_f32(object, "height_pt");
        }
        Some("toc") => coerce_optional_field_to_u32(object, "max_depth"),
        _ => {}
    }
}

fn normalize_list_item(item: &mut Value) {
    let Some(object) = item.as_object_mut() else {
        return;
    };
    coerce_optional_field_to_bool(object, "checked");
    if let Some(children) = object.get_mut("children").and_then(Value::as_array_mut) {
        for child in children {
            normalize_list_item(child);
        }
    }
}

fn coerce_field_to_u32(object: &mut serde_json::Map<String, Value>, field: &str) {
    let Some(value) = object.get(field) else {
        return;
    };
    if let Some(parsed) = parse_u32_value(value) {
        object.insert(field.to_string(), Value::Number(parsed.into()));
    }
}

fn coerce_optional_field_to_u32(object: &mut serde_json::Map<String, Value>, field: &str) {
    let Some(value) = object.get(field) else {
        return;
    };
    if value.is_null() {
        return;
    }
    if let Some(parsed) = parse_u32_value(value) {
        object.insert(field.to_string(), Value::Number(parsed.into()));
    }
}

fn coerce_optional_field_to_f32(object: &mut serde_json::Map<String, Value>, field: &str) {
    let Some(value) = object.get(field) else {
        return;
    };
    if value.is_null() {
        return;
    }
    if let Some(parsed) = parse_f32_value(value) {
        if let Some(number) = serde_json::Number::from_f64(parsed as f64) {
            object.insert(field.to_string(), Value::Number(number));
        }
    }
}

fn coerce_field_to_bool(object: &mut serde_json::Map<String, Value>, field: &str) {
    let Some(value) = object.get(field) else {
        return;
    };
    if let Ok(parsed) = bool_from_value(value.clone()) {
        object.insert(field.to_string(), Value::Bool(parsed));
    }
}

fn coerce_optional_field_to_bool(object: &mut serde_json::Map<String, Value>, field: &str) {
    let Some(value) = object.get(field) else {
        return;
    };
    if value.is_null() {
        return;
    }
    if let Ok(parsed) = bool_from_value(value.clone()) {
        object.insert(field.to_string(), Value::Bool(parsed));
    }
}

fn coerce_array_field_to_u32(object: &mut serde_json::Map<String, Value>, field: &str) {
    let Some(Value::Array(items)) = object.get_mut(field) else {
        return;
    };
    for item in items {
        if let Some(parsed) = parse_u32_value(item) {
            *item = Value::Number(parsed.into());
        }
    }
}

fn parse_u32_value(value: &Value) -> Option<u32> {
    match value {
        Value::Number(number) => number.as_u64().and_then(|parsed| u32::try_from(parsed).ok()),
        Value::String(text) => text.trim().parse::<u32>().ok(),
        _ => None,
    }
}

fn parse_f32_value(value: &Value) -> Option<f32> {
    match value {
        Value::Number(number) => number.as_f64().map(|parsed| parsed as f32),
        Value::String(text) => text.trim().parse::<f32>().ok(),
        _ => None,
    }
}

fn bool_from_value(value: Value) -> Result<bool, de::value::Error> {
    match value {
        Value::Bool(parsed) => Ok(parsed),
        Value::String(text) => match text.trim().to_ascii_lowercase().as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(de::Error::custom("expected bool or boolean string")),
        },
        Value::Number(number) if number.as_i64() == Some(0) => Ok(false),
        Value::Number(number) if number.as_i64() == Some(1) => Ok(true),
        _ => Err(de::Error::custom("expected bool")),
    }
}