//! Tolerant parse-and-heal pipeline for LLM-authored docgen payloads.
//!
//! Mirrors the spirit of `agent_core`'s canonicalize/repair path: repair malformed
//! JSON, unwrap stringified structured fields, normalize aliases, coerce scalar types,
//! and apply safe defaults before strict schema validation runs.

use serde_json::{Map, Value};

use crate::json_repair::repair_json;
use crate::serde_coerce;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HealReport {
    pub repairs: Vec<String>,
}

impl HealReport {
    fn push(&mut self, repair: &str) {
        if !self.repairs.iter().any(|existing| existing == repair) {
            self.repairs.push(repair.to_string());
        }
    }
}

pub fn heal_start_artifact_input(raw: &str) -> Result<(Value, HealReport), String> {
    let mut report = HealReport::default();
    let trimmed = raw.trim();
    let mut value = repair_json(trimmed).map_err(|error| error.to_string())?;
    if serde_json::from_str::<Value>(trimmed).is_err() {
        report.push("repaired invalid JSON argument payload");
    }
    extract_start_artifact_from_document_shape(&mut value, &mut report);
    normalize_start_aliases(&mut value, &mut report);
    let value = retain_only_keys(value, &["title", "author"]);
    Ok((value, report))
}

pub fn heal_append_blocks_input(raw: &str) -> Result<(Value, HealReport), String> {
    let mut report = HealReport::default();
    let trimmed = raw.trim();
    let mut value = repair_json(trimmed).map_err(|error| error.to_string())?;
    if serde_json::from_str::<Value>(trimmed).is_err() {
        report.push("repaired invalid JSON argument payload");
    }
    normalize_append_aliases(&mut value, &mut report);
    unwrap_stringified_blocks_field(&mut value, &mut report);
    wrap_singleton_block(&mut value, &mut report);
    apply_append_defaults(&mut value, &mut report);
    coerce_append_top_level_types(&mut value, &mut report);
    serde_coerce::normalize_append_blocks_args(&mut value);
    let value = retain_only_keys(value, &["artifactId", "blocks", "isComplete"]);
    Ok((value, report))
}

pub fn heal_document_input(raw: &str) -> Result<(Value, HealReport), String> {
    let mut report = HealReport::default();
    let trimmed = raw.trim();
    let mut value = repair_json(trimmed).map_err(|error| error.to_string())?;
    if serde_json::from_str::<Value>(trimmed).is_err() {
        report.push("repaired invalid JSON argument payload");
    }
    unwrap_stringified_blocks_field_on_document(&mut value, &mut report);
    serde_coerce::normalize_document(&mut value);
    Ok((value, report))
}

fn extract_start_artifact_from_document_shape(value: &mut Value, report: &mut HealReport) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if object.contains_key("title") {
        return;
    }
    let metadata = object.get("metadata").cloned();
    let Some(metadata_object) = metadata.as_ref().and_then(Value::as_object) else {
        return;
    };
    if let Some(title) = metadata_object.get("title").cloned() {
        object.insert("title".to_string(), title);
        report.push("extracted title from metadata.title");
    }
    if !object.contains_key("author") {
        if let Some(author) = metadata_object.get("author").cloned() {
            object.insert("author".to_string(), author);
            report.push("extracted author from metadata.author");
        }
    }
}

fn normalize_start_aliases(value: &mut Value, report: &mut HealReport) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    move_alias(object, "document_title", "title", report);
}

fn normalize_append_aliases(value: &mut Value, report: &mut HealReport) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    move_alias(object, "artifact_id", "artifactId", report);
    move_alias(object, "is_complete", "isComplete", report);
    move_alias(object, "complete", "isComplete", report);
}

fn move_alias(
    object: &mut Map<String, Value>,
    alias: &str,
    canonical: &str,
    report: &mut HealReport,
) {
    let Some(value) = object.remove(alias) else {
        return;
    };
    object.entry(canonical.to_string()).or_insert(value);
    report.push(&format!("normalized parameter alias {alias} to {canonical}"));
}

fn unwrap_stringified_blocks_field(value: &mut Value, report: &mut HealReport) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let Some(blocks) = object.get_mut("blocks") else {
        return;
    };
    if unwrap_stringified_array(blocks) {
        report.push("parsed stringified blocks array");
    }
}

fn unwrap_stringified_blocks_field_on_document(value: &mut Value, report: &mut HealReport) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let Some(blocks) = object.get_mut("blocks") else {
        return;
    };
    if unwrap_stringified_array(blocks) {
        report.push("parsed stringified document.blocks array");
    }
}

fn unwrap_stringified_array(value: &mut Value) -> bool {
    let Value::String(raw) = value else {
        return false;
    };
    let trimmed = raw.trim();
    if let Ok(parsed) = repair_json(trimmed) {
        if parsed.is_array() {
            *value = parsed;
            return true;
        }
    }
    false
}

fn wrap_singleton_block(value: &mut Value, report: &mut HealReport) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let Some(blocks) = object.get("blocks") else {
        return;
    };
    if blocks.is_object() {
        object.insert("blocks".to_string(), Value::Array(vec![blocks.clone()]));
        report.push("wrapped singleton block object into blocks array");
    }
}

fn apply_append_defaults(value: &mut Value, report: &mut HealReport) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if !object.contains_key("isComplete") {
        object.insert("isComplete".to_string(), Value::Bool(false));
        report.push("applied default isComplete:false");
    }
}

fn coerce_append_top_level_types(value: &mut Value, report: &mut HealReport) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if let Some(is_complete) = object.get_mut("isComplete") {
        if let Some(parsed) = coerce_bool_value(is_complete) {
            if is_complete.as_bool() != Some(parsed) {
                report.push("coerced isComplete to boolean");
            }
            *is_complete = Value::Bool(parsed);
        }
    }
}

fn coerce_bool_value(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(parsed) => Some(*parsed),
        Value::String(text) => match text.trim().to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        Value::Number(number) if number.as_i64() == Some(0) => Some(false),
        Value::Number(number) if number.as_i64() == Some(1) => Some(true),
        _ => None,
    }
}

fn retain_only_keys(value: Value, allowed: &[&str]) -> Value {
    let Some(mut object) = value.as_object().cloned() else {
        return value;
    };
    object.retain(|key, _| allowed.contains(&key.as_str()));
    Value::Object(object)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heals_stringified_blocks_array() {
        let raw = r#"{"artifact_id":"art-1","blocks":"[{\"type\":\"paragraph\",\"text\":\"Hello\"}]","is_complete":"false"}"#;
        let (value, report) = heal_append_blocks_input(raw).expect("heal append_blocks");
        assert!(value["blocks"].is_array());
        assert_eq!(value["artifactId"], "art-1");
        assert_eq!(value["isComplete"], false);
        assert!(report.repairs.iter().any(|r| r.contains("artifact_id")));
        assert!(report
            .repairs
            .iter()
            .any(|r| r.contains("parsed stringified blocks")));
    }

    #[test]
    fn heals_singleton_block_object() {
        let raw =
            r#"{"artifactId":"art-1","blocks":{"type":"paragraph","text":"solo"},"isComplete":false}"#;
        let (value, report) = heal_append_blocks_input(raw).expect("heal singleton block");
        assert!(value["blocks"].is_array());
        assert_eq!(value["blocks"].as_array().unwrap().len(), 1);
        assert!(report
            .repairs
            .iter()
            .any(|r| r.contains("singleton block")));
    }

    #[test]
    fn heals_markdown_fenced_append_blocks_json() {
        let raw = "```json\n{\"artifactId\":\"art-1\",\"blocks\":[],\"isComplete\":false}\n```";
        let (value, _) = heal_append_blocks_input(raw).expect("heal fenced json");
        assert_eq!(value["artifactId"], "art-1");
    }

    #[test]
    fn heals_trailing_comma_json() {
        let raw = r#"{"artifactId":"art-1","blocks":[{"type":"paragraph","text":"Hi",},],"isComplete":false,}"#;
        let (value, report) = heal_append_blocks_input(raw).expect("heal trailing commas");
        assert_eq!(value["blocks"].as_array().unwrap().len(), 1);
        assert!(report
            .repairs
            .iter()
            .any(|r| r.contains("repaired invalid JSON")));
    }

    #[test]
    fn heals_start_artifact_from_document_shape() {
        let raw = r#"{"version":"1.0","metadata":{"title":"Report","author":"Sean"},"blocks":[]}"#;
        let (value, report) = heal_start_artifact_input(raw).expect("heal start_artifact");
        assert_eq!(value["title"], "Report");
        assert_eq!(value["author"], "Sean");
        assert!(!value.as_object().unwrap().contains_key("version"));
        assert!(report.repairs.iter().any(|r| r.contains("metadata.title")));
    }

    #[test]
    fn heals_string_heading_level_inside_blocks() {
        let raw = r#"{"artifactId":"art-1","blocks":[{"type":"heading","level":"2","text":"Section"}]}"#;
        let (value, _) = heal_append_blocks_input(raw).expect("heal string level");
        assert_eq!(value["blocks"][0]["level"], 2);
    }
}