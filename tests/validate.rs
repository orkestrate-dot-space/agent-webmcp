use agent_webmcp::validate::validate_args;
use serde_json::json;

fn schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "count": { "type": "integer", "minimum": 1 },
            "mode": { "type": "string", "enum": ["a", "b"] },
            "code": { "type": "string", "minLength": 3, "maxLength": 3 }
        },
        "required": ["name"],
        "additionalProperties": false
    })
}

#[test]
fn accepts_a_valid_object() {
    validate_args(&schema(), &json!({ "name": "Ada", "count": 2, "mode": "a", "code": "SFO" })).unwrap();
    validate_args(&schema(), &json!({ "name": "Ada" })).unwrap();
}

#[test]
fn rejects_a_non_object() {
    let err = validate_args(&schema(), &json!(["name"])).unwrap_err();
    assert!(err.contains("JSON object"), "{err}");
}

#[test]
fn rejects_a_missing_required_field() {
    let err = validate_args(&schema(), &json!({ "count": 1 })).unwrap_err();
    assert!(err.contains("missing required field"), "{err}");
    assert!(err.contains("name"), "{err}");
}

#[test]
fn rejects_a_wrong_type() {
    let err = validate_args(&schema(), &json!({ "name": 3 })).unwrap_err();
    assert!(err.contains("must be string"), "{err}");
}

#[test]
fn rejects_a_float_where_an_integer_is_wanted() {
    let err = validate_args(&schema(), &json!({ "name": "x", "count": 1.5 })).unwrap_err();
    assert!(err.contains("must be integer"), "{err}");
}

#[test]
fn rejects_an_unknown_field_when_strict() {
    let err = validate_args(&schema(), &json!({ "name": "x", "nme": "typo" })).unwrap_err();
    assert!(err.contains("unknown field"), "{err}");
}

#[test]
fn rejects_a_value_outside_the_enum() {
    let err = validate_args(&schema(), &json!({ "name": "x", "mode": "c" })).unwrap_err();
    assert!(err.contains("must be one of"), "{err}");
    assert!(err.contains("\"a\""), "{err}");
}

#[test]
fn rejects_out_of_range_values() {
    let err = validate_args(&schema(), &json!({ "name": "x", "count": 0 })).unwrap_err();
    assert!(err.contains("at least 1"), "{err}");
    let err = validate_args(&schema(), &json!({ "name": "x", "code": "TOOLONG" })).unwrap_err();
    assert!(err.contains("at most 3"), "{err}");
}

#[test]
fn allows_extra_fields_when_not_strict() {
    let loose = json!({ "type": "object", "properties": { "name": { "type": "string" } } });
    validate_args(&loose, &json!({ "name": "x", "whatever": 1 })).unwrap();
}

#[test]
fn ignores_keywords_it_does_not_understand() {
    let exotic = json!({
        "type": "object",
        "properties": { "x": { "type": "string", "pattern": "^[A-Z]+$", "format": "hostname" } },
        "required": ["x"]
    });
    validate_args(&exotic, &json!({ "x": "anything at all" })).unwrap();
}
