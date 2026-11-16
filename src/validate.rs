use serde_json::Value;

/// Check `args` against the tool's JSON Schema before anything runs.
///
/// Only the subset catalogs use is enforced: the top-level object shape,
/// `required`, per-property `type` and `enum`, string length bounds, numeric
/// bounds, and `additionalProperties: false`. Unknown keywords are ignored, so
/// richer schemas keep working instead of failing closed here. The page's own
/// tool still validates on call; this check exists to catch the agent's
/// mistakes early, before a browser opens.
pub fn validate_args(schema: &Value, args: &Value) -> Result<(), String> {
    let obj = args
        .as_object()
        .ok_or_else(|| "must be a JSON object".to_string())?;
    let Some(schema_obj) = schema.as_object() else {
        return Err("the catalog entry has no usable input schema".to_string());
    };
    if let Some(required) = schema_obj.get("required").and_then(Value::as_array) {
        for name in required.iter().filter_map(Value::as_str) {
            if !obj.contains_key(name) {
                return Err(format!("missing required field {name:?}"));
            }
        }
    }
    let properties = schema_obj.get("properties").and_then(Value::as_object);
    let strict = schema_obj.get("additionalProperties").and_then(Value::as_bool) == Some(false);
    match properties {
        Some(props) => {
            for (key, value) in obj {
                match props.get(key) {
                    Some(sub) => check_value(key, sub, value)?,
                    None if strict => return Err(format!("unknown field {key:?}")),
                    None => {}
                }
            }
        }
        None => {
            if strict && !obj.is_empty() {
                return Err("this tool takes no arguments".to_string());
            }
        }
    }
    Ok(())
}

fn check_value(key: &str, sub: &Value, value: &Value) -> Result<(), String> {
    if let Some(allowed) = sub.get("enum").and_then(Value::as_array)
        && !allowed.iter().any(|e| e == value)
    {
        return Err(format!("field {key:?} must be one of {}", summarize(allowed)));
    }
    let Some(kind) = sub.get("type").and_then(Value::as_str) else {
        return Ok(());
    };
    let ok = match kind {
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.is_i64() || value.is_u64(),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        "null" => value.is_null(),
        _ => true,
    };
    if !ok {
        return Err(format!("field {key:?} must be {kind}"));
    }
    if let Some(s) = value.as_str() {
        let len = s.chars().count() as u64;
        if let Some(min) = sub.get("minLength").and_then(Value::as_u64)
            && len < min
        {
            return Err(format!("field {key:?} must be at least {min} characters"));
        }
        if let Some(max) = sub.get("maxLength").and_then(Value::as_u64)
            && len > max
        {
            return Err(format!("field {key:?} must be at most {max} characters"));
        }
    }
    if let Some(n) = value.as_f64() {
        if let Some(min) = sub.get("minimum").and_then(Value::as_f64)
            && n < min
        {
            return Err(format!("field {key:?} must be at least {min}"));
        }
        if let Some(max) = sub.get("maximum").and_then(Value::as_f64)
            && n > max
        {
            return Err(format!("field {key:?} must be at most {max}"));
        }
    }
    Ok(())
}

fn summarize(allowed: &[Value]) -> String {
    let mut s = allowed
        .iter()
        .take(8)
        .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "?".to_string()))
        .collect::<Vec<_>>()
        .join(", ");
    if allowed.len() > 8 {
        s.push_str(", ...");
    }
    format!("[{s}]")
}
