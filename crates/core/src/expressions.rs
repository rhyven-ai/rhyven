//! Bounded, typed declarative expressions. No scripting or external side effects.
use crate::{error::ensure, Error, Result};
use serde_json::{json, Value};
use std::cmp::Ordering;

fn fail(message: &str) -> Error {
    Error::new("expression", message)
}
fn compatible(a: &str, b: &str) -> bool {
    a == b || (a == "integer" && b == "number")
}
fn numeric(t: &str) -> bool {
    matches!(t, "integer" | "number")
}
fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        _ => "number",
    }
}

pub fn check(
    e: &Value,
    input: &Value,
    fields: &Value,
    allow_field: bool,
    depth: usize,
) -> Result<String> {
    ensure(depth <= 12, "expression", "Expression depth exceeds 12")?;
    if depth == 0 {
        ensure(
            e.to_string().len() <= 16_384,
            "expression",
            "Expression exceeds 16 KiB",
        )?;
    }
    let m = e
        .as_object()
        .ok_or_else(|| fail("Expression must be an object"))?;
    for key in ["literal", "arg", "field", "runtime"] {
        if let Some(v) = m.get(key) {
            ensure(
                m.len() == 1,
                "expression",
                "Reference/literal requires exactly one key",
            )?;
            if key == "literal" {
                return Ok(kind(v).into());
            }
            let name = v
                .as_str()
                .ok_or_else(|| fail("Reference must be a string"))?;
            if key == "runtime" {
                return match name {
                    "now" => Ok("integer".into()),
                    "actor" => Ok("string".into()),
                    _ => Err(fail("Unknown runtime value")),
                };
            }
            ensure(
                key != "field" || allow_field,
                "expression",
                "Current fields are available only for updates",
            )?;
            let schema = if key == "arg" {
                &input["properties"][name]
            } else {
                &fields["properties"][name]
            };
            return schema["type"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| fail("Unknown argument or field reference"));
        }
    }
    crate::catalog::keys(e, &["op", "args"])?;
    let op = e["op"].as_str().ok_or_else(|| fail("Operator required"))?;
    let args = e["args"]
        .as_array()
        .ok_or_else(|| fail("Operator args must be an array"))?;
    let arity = match op {
        "not" | "trim" | "lower" | "upper" => 1,
        "add" | "sub" | "mul" | "div" | "mod" | "eq" | "ne" | "lt" | "le" | "gt" | "ge" | "in" => 2,
        "and" | "or" | "concat" => 0,
        _ => return Err(fail("Unknown expression operator")),
    };
    ensure(
        if arity == 0 {
            (2..=16).contains(&args.len())
        } else {
            args.len() == arity
        },
        "expression",
        "Invalid expression arity",
    )?;
    let types = args
        .iter()
        .map(|a| check(a, input, fields, allow_field, depth + 1))
        .collect::<Result<Vec<_>>>()?;
    let result = match op {
        "add" | "sub" | "mul" | "div" | "mod" => {
            ensure(
                types.iter().all(|t| numeric(t)),
                "expression",
                "Arithmetic requires numbers",
            )?;
            if op == "mod" {
                ensure(
                    types.iter().all(|t| t == "integer"),
                    "expression",
                    "mod requires integers",
                )?;
            }
            if op == "div" || types.iter().any(|t| t == "number") {
                "number"
            } else {
                "integer"
            }
        }
        "and" | "or" | "not" => {
            ensure(
                types.iter().all(|t| t == "boolean"),
                "expression",
                "Boolean operands required",
            )?;
            "boolean"
        }
        "concat" | "trim" | "lower" | "upper" => {
            ensure(
                types.iter().all(|t| t == "string"),
                "expression",
                "String operands required",
            )?;
            "string"
        }
        "in" => {
            ensure(
                types[1] == "array",
                "expression",
                "in requires an array as its second operand",
            )?;
            "boolean"
        }
        "eq" | "ne" => {
            ensure(
                compatible(&types[0], &types[1]) || compatible(&types[1], &types[0]),
                "expression",
                "Incompatible equality operands",
            )?;
            "boolean"
        }
        _ => {
            ensure(
                (numeric(&types[0]) && numeric(&types[1]))
                    || (types[0] == "string" && types[1] == "string"),
                "expression",
                "Ordering requires numbers or strings",
            )?;
            "boolean"
        }
    };
    Ok(result.into())
}

pub fn validate_action(action: &Value, fields: &Value) -> Result<()> {
    let update = action["operation"] == "update";
    let supported = update || action["operation"] == "create";
    for key in ["expressions", "condition"] {
        ensure(
            supported || action.get(key).is_none(),
            "expression",
            "Expressions/conditions require create or update",
        )?;
    }
    if let Some(map) = action.get("expressions") {
        let map = map
            .as_object()
            .ok_or_else(|| fail("expressions must map field names to expressions"))?;
        for (field, e) in map {
            let target = fields["properties"][field]["type"]
                .as_str()
                .ok_or_else(|| fail("Unknown expression target field"))?;
            ensure(
                action["set"].get(field).is_none(),
                "expression",
                "Field cannot appear in both set and expressions",
            )?;
            let t = check(e, &action["input"], fields, update, 0)?;
            ensure(
                compatible(&t, target),
                "expression",
                format!("Expression type {t} incompatible with {field}: {target}"),
            )?;
        }
    }
    if let Some(e) = action.get("condition") {
        ensure(
            check(e, &action["input"], fields, update, 0)? == "boolean",
            "expression",
            "condition must return boolean",
        )?;
    }
    Ok(())
}

fn int(v: &Value) -> Result<i64> {
    v.as_i64()
        .ok_or_else(|| fail("Integer arithmetic requires signed 64-bit values"))
}
fn float(v: &Value) -> Result<f64> {
    if v.is_i64() || v.is_u64() {
        ensure(
            v.as_f64().unwrap().abs() <= 9_007_199_254_740_991.0,
            "expression",
            "Integer too large for safe floating-point conversion",
        )?;
    }
    v.as_f64().ok_or_else(|| fail("Numeric value required"))
}
pub fn compare(a: &Value, b: &Value) -> Result<Ordering> {
    if (a.is_i64() || a.is_u64()) && (b.is_i64() || b.is_u64()) {
        let wide = |v: &Value| {
            v.as_i64()
                .map(i128::from)
                .unwrap_or_else(|| i128::from(v.as_u64().unwrap()))
        };
        return Ok(wide(a).cmp(&wide(b)));
    }
    if a.is_number() && b.is_number() {
        return float(a)?
            .partial_cmp(&float(b)?)
            .ok_or_else(|| fail("Non-finite comparison"));
    }
    if let (Some(a), Some(b)) = (a.as_str(), b.as_str()) {
        return Ok(a.cmp(b));
    }
    if let (Some(a), Some(b)) = (a.as_bool(), b.as_bool()) {
        return Ok(a.cmp(&b));
    }
    Err(fail("Values cannot be ordered"))
}
pub fn equal(a: &Value, b: &Value) -> Result<bool> {
    if a.is_number() && b.is_number() {
        Ok(compare(a, b)? == Ordering::Equal)
    } else {
        Ok(a == b)
    }
}

pub fn eval(
    e: &Value,
    input: &Value,
    current: &Value,
    actor: &str,
    now: u64,
    depth: usize,
) -> Result<Value> {
    ensure(depth <= 12, "expression", "Expression depth exceeds 12")?;
    if let Some(v) = e.get("literal") {
        return Ok(v.clone());
    }
    for (key, source) in [("arg", input), ("field", current)] {
        if let Some(name) = e[key].as_str() {
            return source
                .get(name)
                .cloned()
                .ok_or_else(|| fail("Referenced value is absent"));
        }
    }
    if let Some(name) = e["runtime"].as_str() {
        return match name {
            "now" => Ok(json!(now)),
            "actor" => Ok(json!(actor)),
            _ => Err(fail("Unknown runtime value")),
        };
    }
    let op = e["op"].as_str().ok_or_else(|| fail("Operator required"))?;
    let operands = e["args"]
        .as_array()
        .ok_or_else(|| fail("Operator operands required"))?;
    let mut values = Vec::new();
    for arg in operands {
        let v = eval(arg, input, current, actor, now, depth + 1)?;
        if op == "and" && v == false {
            return Ok(json!(false));
        }
        if op == "or" && v == true {
            return Ok(json!(true));
        }
        values.push(v);
    }
    let a = values
        .first()
        .ok_or_else(|| fail("Operator operands required"))?;
    let b = values.get(1).unwrap_or(&Value::Null);
    let result = match op {
        "and" => json!(true),
        "or" => json!(false),
        "not" => json!(!a.as_bool().ok_or_else(|| fail("Boolean required"))?),
        "eq" => json!(equal(a, b)?),
        "ne" => json!(!equal(a, b)?),
        "lt" => json!(compare(a, b)? == Ordering::Less),
        "le" => json!(compare(a, b)? != Ordering::Greater),
        "gt" => json!(compare(a, b)? == Ordering::Greater),
        "ge" => json!(compare(a, b)? != Ordering::Less),
        "in" => {
            let mut found = false;
            for v in b.as_array().ok_or_else(|| fail("Array required"))? {
                if equal(a, v)? {
                    found = true;
                    break;
                }
            }
            json!(found)
        }
        "concat" => json!(values
            .iter()
            .map(|v| v.as_str().ok_or_else(|| fail("String required")))
            .collect::<Result<Vec<_>>>()?
            .join("")),
        "trim" => json!(a.as_str().ok_or_else(|| fail("String required"))?.trim()),
        "lower" => json!(a
            .as_str()
            .ok_or_else(|| fail("String required"))?
            .to_lowercase()),
        "upper" => json!(a
            .as_str()
            .ok_or_else(|| fail("String required"))?
            .to_uppercase()),
        "add" | "sub" | "mul" | "mod"
            if (a.is_i64() || a.is_u64()) && (b.is_i64() || b.is_u64()) =>
        {
            let (a, b) = (int(a)?, int(b)?);
            let n = match op {
                "add" => a.checked_add(b),
                "sub" => a.checked_sub(b),
                "mul" => a.checked_mul(b),
                _ => a.checked_rem(b),
            }
            .ok_or_else(|| fail("Integer overflow or division by zero"))?;
            json!(n)
        }
        "add" | "sub" | "mul" | "div" => {
            let (a, b) = (float(a)?, float(b)?);
            ensure(op != "div" || b != 0.0, "expression", "Division by zero")?;
            let n = match op {
                "add" => a + b,
                "sub" => a - b,
                "mul" => a * b,
                _ => a / b,
            };
            ensure(n.is_finite(), "expression", "Non-finite arithmetic result")?;
            json!(n)
        }
        _ => return Err(fail("Invalid operator or operand types")),
    };
    ensure(
        result.to_string().len() <= 100_000,
        "expression",
        "Expression result exceeds 100 KB",
    )?;
    Ok(result)
}
