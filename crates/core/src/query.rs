//! Typed query extensions shared by direct calls, MCP and REST.
use crate::{error::ensure, expressions, schema, Result};
use serde_json::{json, Value};
use std::cmp::Ordering;

pub fn validate_object(name: &str, object: &Value) -> Result<()> {
    let props = object["schema"]["properties"].as_object().unwrap();
    if let Some(fields) = object.get("search_fields") {
        let fields = fields
            .as_array()
            .ok_or_else(|| crate::Error::new("package", "search_fields must be an array"))?;
        let mut seen = std::collections::BTreeSet::new();
        ensure(
            !fields.is_empty() && fields.len() <= 32,
            "package",
            "Declare 1..32 search fields",
        )?;
        for field in fields {
            let field = field.as_str().unwrap_or("");
            let s = &object["schema"]["properties"][field];
            ensure(
                props.contains_key(field)
                    && seen.insert(field)
                    && (s["type"] == "string"
                        || (s["type"] == "array" && s["items"]["type"] == "string")),
                "package",
                "Search fields must be distinct strings or string arrays",
            )?;
        }
    }
    if let Some(field) = object.get("supersession_field") {
        let field = field.as_str().unwrap_or("");
        let relation = &object["relationships"][field];
        ensure(
            object["immutable"] == true
                && props.get(field).is_some_and(|s| s["type"] == "string")
                && relation["object"] == name
                && relation.get("app").is_none(),
            "package",
            "Supersession requires an immutable object with a same-object local relationship",
        )?;
    }
    Ok(())
}

pub fn properties(object: &Value) -> Value {
    let mut fields = serde_json::Map::new();
    let mut sortable = vec!["$created_at", "$updated_at"];
    for (name, s) in object["schema"]["properties"].as_object().unwrap() {
        let mut scalar = s.clone();
        scalar.as_object_mut().unwrap().remove("default");
        let mut ops =
            json!({"eq":scalar,"ne":scalar,"in":{"type":"array","items":scalar,"maxItems":100}});
        if matches!(s["type"].as_str(), Some("string" | "number" | "integer")) {
            for op in ["lt", "le", "gt", "ge"] {
                ops[op] = scalar.clone();
            }
        }
        if s["type"] == "string" {
            ops["contains"] = json!({"type":"string","maxLength":1000});
        }
        if s["type"] == "array" {
            ops["has"] = s["items"].clone();
        }
        if matches!(
            s["type"].as_str(),
            Some("string" | "number" | "integer" | "boolean")
        ) {
            sortable.push(name.as_str());
        }
        fields.insert(
            name.clone(),
            json!({"type":"object","properties":ops,"required":[],"additionalProperties":false}),
        );
    }
    let mut result = json!({"where":{"type":"object","properties":fields,"required":[],"additionalProperties":false}});
    if !sortable.is_empty() {
        result["order_by"] = json!({"type":"array","maxItems":3,"items":{"type":"object","properties":{"field":{"type":"string","enum":sortable},"direction":{"type":"string","enum":["asc","desc"]}},"required":["field"],"additionalProperties":false}});
    }
    let timestamp_ops = json!({"type":"object","properties":{
        "eq":{"type":"integer","minimum":0},"lt":{"type":"integer","minimum":0},
        "le":{"type":"integer","minimum":0},"gt":{"type":"integer","minimum":0},
        "ge":{"type":"integer","minimum":0}},"additionalProperties":false});
    result["metadata"] = json!({"type":"object","properties":{"created_at":timestamp_ops,"updated_at":timestamp_ops},"additionalProperties":false});
    if object.get("search_fields").is_some() {
        result["search"] = json!({"type":"string","minLength":1,"maxLength":1000,"description":"Case-insensitive literal keywords; all words must occur across declared search fields"});
    }
    if object.get("supersession_field").is_some() {
        result["current_only"] = json!({"type":"boolean","description":"Exclude any record superseded by another record, before applying other filters"});
    }
    result
}
pub fn validate(args: &Value, object: &Value) -> Result<()> {
    let props = properties(object);
    for key in ["where", "order_by", "metadata", "search", "current_only"] {
        if let Some(v) = args.get(key) {
            ensure(
                props.get(key).is_some(),
                "validation",
                format!("Query option {key} is not declared for this object"),
            )?;
            schema::validate(v.clone(), &props[key])?;
        }
    }
    if let Some(search) = args["search"].as_str() {
        let n = search.split_whitespace().count();
        ensure(
            n > 0 && n <= 32,
            "validation",
            "Search requires 1..32 keywords",
        )?;
    }
    Ok(())
}
pub fn matches(data: &Value, where_: &Value) -> Result<bool> {
    for (field, conditions) in where_.as_object().into_iter().flatten() {
        let Some(actual) = data.get(field) else {
            return Ok(false);
        };
        for (op, expected) in conditions.as_object().unwrap() {
            let ok = match op.as_str() {
                "eq" => expressions::equal(actual, expected)?,
                "ne" => !expressions::equal(actual, expected)?,
                "lt" => expressions::compare(actual, expected)? == Ordering::Less,
                "le" => expressions::compare(actual, expected)? != Ordering::Greater,
                "gt" => expressions::compare(actual, expected)? == Ordering::Greater,
                "ge" => expressions::compare(actual, expected)? != Ordering::Less,
                "contains" => actual
                    .as_str()
                    .unwrap()
                    .contains(expected.as_str().unwrap()),
                "has" => actual.as_array().unwrap().contains(expected),
                "in" => {
                    let mut found = false;
                    for v in expected.as_array().unwrap() {
                        if expressions::equal(actual, v)? {
                            found = true;
                            break;
                        }
                    }
                    found
                }
                _ => unreachable!(),
            };
            if !ok {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

pub fn search_matches(data: &Value, text: &Value, object: &Value) -> bool {
    let Some(text) = text.as_str() else {
        return true;
    };
    let fields: Vec<String> = object["search_fields"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|f| {
            let value = &data[f.as_str().unwrap()];
            if let Some(s) = value.as_str() {
                vec![s.to_lowercase()]
            } else {
                value
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_lowercase)
                    .collect()
            }
        })
        .collect();
    text.to_lowercase()
        .split_whitespace()
        .all(|word| fields.iter().any(|s| s.contains(word)))
}

fn sort_key<'a>(record: &'a Value, field: &str) -> Option<&'a Value> {
    if let Some(field) = field.strip_prefix('$') {
        record.get(field)
    } else {
        record["data"].get(field)
    }
}
pub fn sort(records: &mut [Value], order: &Value) -> Result<()> {
    // Preflight every numeric sort key before entering the infallible comparator.
    // Mixed numeric domains that cannot be compared without precision loss fail closed.
    for item in order.as_array().into_iter().flatten() {
        let field = item["field"].as_str().unwrap();
        let values: Vec<_> = records.iter().filter_map(|r| sort_key(r, field)).collect();
        if values.iter().any(|v| v.is_f64()) {
            for v in &values {
                if v.is_number() {
                    expressions::compare(v, &json!(0.5))?;
                }
            }
        }
    }
    records.sort_by(|a, b| {
        for item in order.as_array().into_iter().flatten() {
            let field = item["field"].as_str().unwrap();
            let cmp = match (sort_key(a, field), sort_key(b, field)) {
                (None, None) => Ordering::Equal,
                (None, _) => return Ordering::Greater,
                (_, None) => return Ordering::Less,
                (Some(a), Some(b)) => {
                    expressions::compare(a, b).expect("Validated scalar sort keys")
                }
            };
            let cmp = if item["direction"] == "desc" {
                cmp.reverse()
            } else {
                cmp
            };
            if cmp != Ordering::Equal {
                return cmp;
            }
        }
        a["id"].as_str().cmp(&b["id"].as_str())
    });
    Ok(())
}
