use agent_market_core::{catalog, expressions, Runtime};
use serde_json::{json, Value};
fn object(properties: Value, required: Value) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn package() -> Value {
    let fields = object(
        json!({"quantity":{"type":"integer","minimum":0,"default":0},"before":{"type":"integer","default":0},"name":{"type":"string"},"state":{"type":"string","enum":["open","closed"]},"actor":{"type":"string"},"at":{"type":"integer"},"rank":{"type":"integer"}}),
        json!(["quantity", "name", "state", "actor", "at"]),
    );
    json!({"format":2,"name":"test/stock","version":"0.1.0","publisher":"test","description":"Declarative transaction fixture","guide":"Test only","hosting":{"mode":"local"},"permissions":["state.read","state.write"],"objects":{"item":{"schema":fields,"protected_fields":["quantity"]}},"actions":{
        "receive":{"description":"Create item","operation":"create","object":"item","input":object(json!({"quantity":{"type":"integer","minimum":0},"name":{"type":"string"}}),json!(["quantity","name"])),"set":{"quantity":{"$arg":"quantity"},"state":"open"},"expressions":{"name":{"op":"lower","args":[{"op":"trim","args":[{"arg":"name"}]}]},"actor":{"runtime":"actor"},"at":{"runtime":"now"}}},
        "withdraw":{"description":"Atomic decrement","operation":"update","object":"item","input":object(json!({"id":{"type":"string"},"expected_revision":{"type":"integer","minimum":1},"amount":{"type":"integer","minimum":1}}),json!(["id","expected_revision","amount"])),"condition":{"op":"and","args":[{"op":"ge","args":[{"field":"quantity"},{"arg":"amount"}]},{"op":"in","args":[{"field":"state"},{"literal":["open"]}]}]},"expressions":{"before":{"field":"quantity"},"quantity":{"op":"sub","args":[{"field":"quantity"},{"arg":"amount"}]},"actor":{"runtime":"actor"},"at":{"runtime":"now"}}}
    },"tests":[]})
}
fn receive(r: &Runtime, name: &str, n: i64) -> Value {
    r.call(
        "execute",
        json!({"app":"test/stock","action":"receive","args":{"name":name,"quantity":n}}),
    )
    .unwrap()
}
#[test]
fn transactional_expressions_guards_retry_and_fresh_discovery() {
    let d = tempfile::tempdir().unwrap();
    let r = Runtime::new(d.path(), "agent-one").unwrap();
    let p = package();
    catalog::validate(&p).unwrap();
    r.install(&p, true, false).unwrap();
    let row = receive(&r, "  Bolts  ", 10);
    assert_eq!(row["data"]["name"], "bolts");
    assert_eq!(row["data"]["actor"], "agent-one");
    assert!(row["data"]["at"].as_u64().unwrap() > 0);
    let args = json!({"category":"test/stock","function":"action_withdraw","args":{"id":row["id"],"expected_revision":1,"amount":3,"request_id":"once"}});
    let changed = r.call("rhyven_call", args.clone()).unwrap();
    assert_eq!(changed["data"]["quantity"], 7);
    assert_eq!(changed["data"]["before"], 10);
    assert_eq!(r.call("rhyven_call", args).unwrap(), changed);
    let snapshot = r.snapshot().unwrap();
    assert_eq!(r.call("execute",json!({"app":"test/stock","action":"withdraw","args":{"id":row["id"],"expected_revision":2,"amount":8}})).unwrap_err().code,"guard_failed");
    assert_eq!(r.snapshot().unwrap(), snapshot);
    assert_eq!(r.call("execute",json!({"app":"test/stock","action":"withdraw","args":{"id":row["id"],"expected_revision":1,"amount":1}})).unwrap_err().code,"revision_conflict");
    let desc = r
        .call("rhyven_describe", json!({"category":"test/stock"}))
        .unwrap();
    let query = desc["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "object_item_query")
        .unwrap();
    assert!(
        query["inputSchema"]["properties"]["where"]["properties"]["quantity"]["properties"]
            .get("ge")
            .is_some()
    );
}
#[test]
fn queries_are_typed_sorted_before_pagination_and_exact_for_large_integers() {
    let d = tempfile::tempdir().unwrap();
    let r = Runtime::new(d.path(), "agent").unwrap();
    r.install(&package(), true, false).unwrap();
    receive(&r, "Bolts", 10);
    receive(&r, "Nuts", 20);
    receive(&r, "Washers", 30);
    let q = json!({"category":"test/stock","function":"object_item_query","args":{"where":{"quantity":{"ge":10,"lt":30},"state":{"in":["open"]}},"order_by":[{"field":"quantity","direction":"desc"}],"limit":1,"offset":1}});
    let result = r.call("rhyven_call", q).unwrap();
    assert_eq!(result["total"], 2);
    assert_eq!(result["items"][0]["data"]["quantity"], 10);
    let query = |extra: Value| {
        let mut args = json!({"app":"test/stock","object":"item"});
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        r.call("query", args)
    };
    assert_eq!(
        query(json!({"where":{"name":{"contains":"olt"}}})).unwrap()["total"],
        1
    );
    assert_eq!(
        query(json!({"where":{"rank":{"ne":0}}})).unwrap()["total"],
        0
    );
    assert!(query(json!({"where":{"quantity":{"ge":"wrong"}}})).is_err());
    assert!(query(json!({"where":{"quantity":{"regex":5}}})).is_err());
    assert!(query(json!({"order_by":[{"field":"unknown"}]})).is_err());
    receive(&r, "large-a", 9_007_199_254_740_992);
    receive(&r, "large-b", 9_007_199_254_740_993);
    let result=query(json!({"where":{"quantity":{"gt":9_007_199_254_740_992_i64}},"order_by":[{"field":"quantity"}]})).unwrap();
    assert_eq!(result["total"], 1);
    assert_eq!(result["items"][0]["data"]["name"], "large-b");
}
#[test]
fn invalid_programs_and_runtime_numeric_failures_leave_state_intact() {
    for expr in [
        json!({"op":"shell","args":[]}),
        json!({"op":"add","args":[{"literal":"x"},{"literal":1}]}),
        json!({"field":"absent"}),
        json!({"arg":"absent"}),
        json!({"literal":1,"field":"quantity"}),
    ] {
        let mut p = package();
        p["actions"]["withdraw"]["expressions"]["quantity"] = expr;
        assert!(catalog::validate(&p).is_err());
    }
    let mut p = package();
    p["actions"]["receive"]["expressions"]["quantity"] = json!({"field":"quantity"});
    assert!(catalog::validate(&p).is_err());
    let d = tempfile::tempdir().unwrap();
    let r = Runtime::new(d.path(), "agent").unwrap();
    let mut p = package();
    p["actions"]["withdraw"]["expressions"]["quantity"] =
        json!({"op":"add","args":[{"field":"quantity"},{"literal":i64::MAX}]});
    r.install(&p, true, false).unwrap();
    let row = receive(&r, "x", 1);
    let snapshot = r.snapshot().unwrap();
    assert_eq!(r.call("execute",json!({"app":"test/stock","action":"withdraw","args":{"id":row["id"],"expected_revision":1,"amount":1}})).unwrap_err().code,"expression");
    assert_eq!(r.snapshot().unwrap(), snapshot);
}
#[test]
fn numeric_and_boolean_semantics_and_bounds() {
    let run = |e: Value| {
        expressions::check(
            &e,
            &object(json!({}), json!([])),
            &object(json!({}), json!([])),
            false,
            0,
        )?;
        expressions::eval(&e, &json!({}), &json!({}), "agent", 123, 0)
    };
    assert_eq!(
        run(json!({"op":"div","args":[{"literal":5},{"literal":2}]})).unwrap(),
        json!(2.5)
    );
    assert_eq!(
        run(json!({"op":"mod","args":[{"literal":5},{"literal":2}]})).unwrap(),
        json!(1)
    );
    for op in ["div", "mod"] {
        assert!(run(json!({"op":op,"args":[{"literal":5},{"literal":0}]})).is_err());
    }
    assert!(run(json!({"op":"mul","args":[{"literal":1e308},{"literal":10}]})).is_err());
    assert!(run(
        json!({"op":"add","args":[{"literal":9_007_199_254_740_993_i64},{"literal":0.5}]})
    )
    .is_err());
    assert_eq!(run(json!({"op":"or","args":[{"literal":true},{"op":"gt","args":[{"op":"div","args":[{"literal":1},{"literal":0}]},{"literal":0}]}]})).unwrap(),true);
    assert_eq!(run(json!({"op":"concat","args":[{"literal":"item-"},{"op":"upper","args":[{"literal":"abc"}]}]})).unwrap(),"item-ABC");
    let mut deep = json!({"literal":true});
    for _ in 0..14 {
        deep = json!({"op":"not","args":[deep]});
    }
    assert!(run(deep).is_err());
}

#[test]
fn concurrent_actions_cannot_overdraw_and_integer_schema_bounds_are_exact() {
    let d = tempfile::tempdir().unwrap();
    let r = Runtime::new(d.path(), "agent").unwrap();
    r.install(&package(), true, false).unwrap();
    let row = receive(&r, "stock", 10);
    let args = json!({"app":"test/stock","action":"withdraw","args":{"id":row["id"],"expected_revision":1,"amount":7}});
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let path = d.path().to_path_buf();
            let args = args.clone();
            std::thread::spawn(move || Runtime::new(path, "worker").unwrap().call("execute", args))
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        r.call(
            "get",
            json!({"app":"test/stock","object":"item","id":row["id"]})
        )
        .unwrap()["data"]["quantity"],
        3
    );
    assert!(agent_market_core::schema::validate(
        json!(9_007_199_254_740_993_i64),
        &json!({"type":"integer","maximum":9_007_199_254_740_992_i64})
    )
    .is_err());
}
