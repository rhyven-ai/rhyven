use agent_market_core::schema;
use serde_json::json;

#[test]
fn typed_action_alternatives_reject_mixed_or_forged_fields() {
    let schema = json!({"anyOf":[
        {"type":"null"},
        {"type":"object","properties":{"type":{"type":"string","enum":["run"]},"program":{"type":"string"}},"required":["type","program"],"additionalProperties":false},
        {"type":"object","properties":{"type":{"type":"string","enum":["change_directory"]},"path":{"type":"string"}},"required":["type","path"],"additionalProperties":false}
    ]});
    schema::check(&schema, 0).unwrap();
    for valid in [
        json!(null),
        json!({"type":"run","program":"printf"}),
        json!({"type":"change_directory","path":"/tmp"}),
    ] {
        assert_eq!(schema::validate(valid.clone(), &schema).unwrap(), valid);
    }
    for invalid in [
        json!({"type":"run"}),
        json!({"type":"run","path":"/tmp"}),
        json!({"type":"run","program":"printf","approved":true}),
    ] {
        assert!(schema::validate(invalid, &schema).is_err());
    }
}

#[test]
fn nullable_values_and_schema_bounds_are_checked() {
    let schema = json!({"type":["string","null"],"maxLength":3});
    schema::check(&schema, 0).unwrap();
    assert!(schema::validate(json!(null), &schema).is_ok());
    assert!(schema::validate(json!("yes"), &schema).is_ok());
    assert!(schema::validate(json!("long"), &schema).is_err());
    assert!(schema::validate(json!(3), &schema).is_err());
    for invalid in [
        json!({"anyOf":[]}),
        json!({"anyOf":[{"type":"string"}],"enum":["x"]}),
        json!({"type":["string","string"]}),
        json!({"type":[null]}),
    ] {
        assert!(schema::check(&invalid, 0).is_err());
    }
    let mut deep = json!({"type":"string"});
    for _ in 0..10 {
        deep = json!({"anyOf":[deep]});
    }
    assert!(schema::check(&deep, 0).is_err());
}
