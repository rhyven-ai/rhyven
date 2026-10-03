use agent_market_core::{catalog, connector, tools};
use serde_json::{json, Value};
fn package() -> Value {
    json!({"format":2,"name":"test/connector","version":"0.1.0","publisher":"test","description":"Existing service wrapper","hosting":{"mode":"self-hosted","endpoint":"http://127.0.0.1:1234/mcp","domains":["127.0.0.1"],"auth":"None","privacy":"External state","account":"Existing service","billing":"External"},"permissions":["state.read","network.connect"],"objects":{},"connector":{"protocol":"mcp"},"actions":{"echo":{"description":"Echo text","input":{"type":"object","properties":{"message":{"type":"string"}},"required":["message"],"additionalProperties":false},"target":"Echo"}},"guide":"Start upstream separately","tests":[]})
}
#[test]
fn connector_contracts_and_permissions_are_closed() {
    let p = package();
    catalog::validate(&p).unwrap();
    let manifest = tools::manifest(&p);
    assert_eq!(manifest["functions"].as_array().unwrap().len(), 1);
    assert!(manifest["functions"][0]["inputSchema"]["properties"]
        .get("request_id")
        .is_none());
    for (path, value) in [
        ("/connector/protocol", json!("shell")),
        ("/actions/echo/target", json!("")),
        ("/hosting/endpoint", json!("http://example.com/mcp")),
        ("/permissions", json!(["state.read"])),
        ("/hosting/mode", json!("local")),
    ] {
        let mut invalid = p.clone();
        *invalid.pointer_mut(path).unwrap() = value;
        assert!(catalog::validate(&invalid).is_err(), "{path}");
    }
    assert!(connector::call(&p, &json!({"action":"new-upstream-tool","args":{}})).is_err());
    assert!(connector::call(&p, &json!({"action":"echo","args":{"message":4}})).is_err());
    assert!(connector::call(
        &p,
        &json!({"action":"echo","args":{"message":"x"},"request_id":"unsafe-retry"})
    )
    .is_err());
}
#[test]
fn http_cannot_escape_endpoint_or_silently_drop_arguments() {
    let mut p = package();
    p["connector"]["protocol"] = json!("http");
    p["actions"]["echo"]["target"] =
        json!({"method":"GET","path":"/items/{message}","path_args":["message"],"query_args":[]});
    catalog::validate(&p).unwrap();
    for path in [
        "//evil.test/x",
        "/../credentials",
        "/%2e%2e/x",
        "/x?secret=1",
        "/x#fragment",
        "/items/{unknown}",
    ] {
        let mut bad = p.clone();
        bad["actions"]["echo"]["target"]["path"] = json!(path);
        assert!(catalog::validate(&bad).is_err(), "{path}");
    }
    for input in ["..", "%2e%2e", "a/b", "a\\b", "a?b"] {
        assert!(connector::call(&p, &json!({"action":"echo","args":{"message":input}})).is_err());
    }
    p["actions"]["echo"]["input"]["properties"]["ignored"] = json!({"type":"string"});
    assert!(catalog::validate(&p).is_err());
}
