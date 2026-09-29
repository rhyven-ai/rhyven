//! Supported JSON Schema types, constraints and value validation.
use crate::{error::ensure, Error, Result};
use serde_json::{json, Value};

pub fn name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

pub fn check(s: &Value, depth: usize) -> Result<()> {
    let object = s
        .as_object()
        .ok_or_else(|| Error::new("invalid_schema", "Schema must be an object"))?;
    ensure(depth <= 8, "invalid_schema", "Maximum schema depth is 8")?;
    let allowed = [
        "type",
        "properties",
        "required",
        "additionalProperties",
        "items",
        "enum",
        "default",
        "minLength",
        "maxLength",
        "minimum",
        "maximum",
        "maxItems",
        "description",
        "format",
        "anyOf",
    ];
    for key in object.keys() {
        ensure(
            allowed.contains(&key.as_str()),
            "unsupported_schema",
            format!("Unsupported keyword {key}"),
        )?;
    }
    if let Some(branches) = s.get("anyOf") {
        ensure(
            object.keys().all(|k| k == "anyOf" || k == "description"),
            "invalid_schema",
            "anyOf cannot mix with other constraints",
        )?;
        let branches = branches
            .as_array()
            .ok_or_else(|| Error::new("invalid_schema", "anyOf requires an array"))?;
        ensure(
            !branches.is_empty() && branches.len() <= 16,
            "invalid_schema",
            "anyOf requires 1..16 alternatives",
        )?;
        for branch in branches {
            check(branch, depth + 1)?;
        }
        return Ok(());
    }
    if let Some(types) = s["type"].as_array() {
        ensure(
            !types.is_empty() && types.len() <= 7,
            "invalid_schema",
            "type requires 1..7 alternatives",
        )?;
        let mut seen = std::collections::BTreeSet::new();
        for typ in types {
            ensure(
                typ.is_string() && seen.insert(typ.as_str().unwrap()),
                "invalid_schema",
                "Types must be unique strings",
            )?;
            let mut branch = s.clone();
            branch["type"] = typ.clone();
            check(&branch, depth + 1)?;
        }
        return Ok(());
    }
    let typ = s["type"].as_str().unwrap_or("");
    if let Some(format) = s.get("format") {
        ensure(
            typ == "string" && format == "date",
            "invalid_schema",
            "Only string format=date is supported",
        )?;
    }
    ensure(
        [
            "object", "array", "string", "integer", "number", "boolean", "null",
        ]
        .contains(&typ),
        "invalid_schema",
        "Unsupported or missing type",
    )?;
    if typ == "object" {
        ensure(
            s["additionalProperties"] == false,
            "invalid_schema",
            "Objects require additionalProperties=false",
        )?;
        let props = s["properties"]
            .as_object()
            .ok_or_else(|| Error::new("invalid_schema", "Object requires properties"))?;
        let required = s.get("required").cloned().unwrap_or(json!([]));
        let required = required
            .as_array()
            .ok_or_else(|| Error::new("invalid_schema", "required must be an array"))?;
        let mut seen = std::collections::BTreeSet::new();
        for key in required {
            let key = key
                .as_str()
                .ok_or_else(|| Error::new("invalid_schema", "required must contain strings"))?;
            ensure(
                props.contains_key(key) && seen.insert(key),
                "invalid_schema",
                "Invalid/duplicate required field",
            )?;
        }
        for (key, sub) in props {
            ensure(name(key), "invalid_schema", "Invalid property name")?;
            check(sub, depth + 1)?;
        }
    }
    if typ == "array" {
        check(&s["items"], depth + 1)?;
    }
    for key in ["minLength", "maxLength", "maxItems"] {
        if let Some(v) = s.get(key) {
            ensure(
                v.as_u64().is_some(),
                "invalid_schema",
                format!("{key} must be a nonnegative integer"),
            )?;
        }
    }
    for key in ["minimum", "maximum"] {
        if let Some(v) = s.get(key) {
            ensure(
                v.is_number(),
                "invalid_schema",
                format!("{key} must be numeric"),
            )?;
        }
    }
    if let Some(v) = s.get("enum") {
        ensure(
            v.as_array().is_some_and(|a| !a.is_empty()),
            "invalid_schema",
            "enum must be a nonempty array",
        )?;
    }
    if let Some(v) = s.get("default") {
        validate(v.clone(), s)?;
    }
    Ok(())
}

pub fn validate(mut value: Value, s: &Value) -> Result<Value> {
    if let Some(branches) = s["anyOf"].as_array() {
        return branches
            .iter()
            .find_map(|branch| validate(value.clone(), branch).ok())
            .ok_or_else(|| {
                Error::new("validation", "Value does not match any allowed alternative")
            });
    }
    if let Some(types) = s["type"].as_array() {
        return types
            .iter()
            .find_map(|typ| {
                let mut branch = s.clone();
                branch["type"] = typ.clone();
                validate(value.clone(), &branch).ok()
            })
            .ok_or_else(|| Error::new("validation", "Value does not match any allowed type"));
    }
    let typ = s["type"].as_str().unwrap_or("");
    let valid = match typ {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    };
    ensure(valid, "validation", format!("Expected {typ}"))?;
    match typ {
        "object" => {
            let props = s["properties"]
                .as_object()
                .ok_or_else(|| Error::new("invalid_schema", "Missing properties"))?;
            let data = value.as_object_mut().unwrap();
            for key in data.keys() {
                ensure(
                    props.contains_key(key),
                    "validation",
                    format!("Unknown field {key}"),
                )?;
            }
            for (key, sub) in props {
                if !data.contains_key(key) {
                    if let Some(default) = sub.get("default") {
                        data.insert(key.clone(), default.clone());
                    }
                }
                if let Some(v) = data.get_mut(key) {
                    *v = validate(v.clone(), sub)
                        .map_err(|e| Error::new(&e.code, format!("{key}: {}", e.message)))?;
                }
            }
            for key in s["required"].as_array().into_iter().flatten() {
                let key = key.as_str().unwrap_or("");
                ensure(
                    data.contains_key(key),
                    "validation",
                    format!("Missing required field {key}"),
                )?;
            }
        }
        "array" => {
            let data = value.as_array_mut().unwrap();
            ensure(
                data.len() as u64 <= s["maxItems"].as_u64().unwrap_or(1000),
                "validation",
                "Too many array items",
            )?;
            for v in data {
                *v = validate(v.clone(), &s["items"])?;
            }
        }
        "string" => {
            if s["format"] == "date" {
                ensure(
                    calendar_date(value.as_str().unwrap()),
                    "validation",
                    "Expected a valid YYYY-MM-DD calendar date",
                )?;
            }
            let len = value.as_str().unwrap().chars().count() as u64;
            ensure(
                len >= s["minLength"].as_u64().unwrap_or(0)
                    && len <= s["maxLength"].as_u64().unwrap_or(100000),
                "validation",
                "Invalid string length",
            )?;
        }
        "number" | "integer" => {
            if let Some(minimum) = s.get("minimum") {
                ensure(
                    crate::expressions::compare(&value, minimum)? != std::cmp::Ordering::Less,
                    "validation",
                    "Number below minimum",
                )?;
            }
            if let Some(maximum) = s.get("maximum") {
                ensure(
                    crate::expressions::compare(&value, maximum)? != std::cmp::Ordering::Greater,
                    "validation",
                    "Number above maximum",
                )?;
            }
        }
        _ => {}
    }
    if let Some(values) = s["enum"].as_array() {
        ensure(
            values.contains(&value),
            "validation",
            format!("Value must be one of {}", s["enum"]),
        )?;
    }
    Ok(value)
}

fn calendar_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || b.iter()
            .enumerate()
            .any(|(i, b)| i != 4 && i != 7 && !b.is_ascii_digit())
    {
        return false;
    }
    let year: u32 = s[..4].parse().unwrap();
    let month: u32 = s[5..7].parse().unwrap();
    let day: u32 = s[8..].parse().unwrap();
    let days = match month {
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => {
            29
        }
        2 => 28,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return false,
    };
    year > 0 && day > 0 && day <= days
}
