use agent_market_core::ranker;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};

fn server(response: Value) -> (String, std::thread::JoinHandle<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 4096];
        let (offset, length) = loop {
            let n = socket.read(&mut chunk).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&chunk[..n]);
            if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&bytes[..i]).to_ascii_lowercase();
                let length = header
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .unwrap()
                    .trim()
                    .parse::<usize>()
                    .unwrap();
                break (i + 4, length);
            }
        };
        while bytes.len() < offset + length {
            let n = socket.read(&mut chunk).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&chunk[..n]);
        }
        let body = serde_json::from_slice(&bytes[offset..offset + length]).unwrap();
        let response = response.to_string();
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response.len(),
            response
        )
        .unwrap();
        body
    });
    (url, handle)
}
fn candidates() -> Vec<Value> {
    vec![
        json!({"description":"Save note","covers":["write"]}),
        json!({"description":"Store document","covers":["write"]}),
    ]
}
fn plan() -> Value {
    json!({"steps":[{"id":"write","need":"Store document"}]})
}
#[test]
fn generic_laya_and_jev_wire_adapters_only_reorder_existing_candidates() {
    for provider in ["generic", "laya", "jev"] {
        let response = if provider == "generic" {
            json!({"order":["c1","c0"]})
        } else {
            json!({"answers":{"s0":{"choice":"c1"}}})
        };
        let (url, handle) = server(response);
        let mut c = candidates();
        let result = ranker::rerank(
            &json!({"enabled":true,"provider":provider,"endpoint":url}),
            &plan(),
            &mut c,
            Duration::from_secs(3),
        )
        .unwrap();
        assert_eq!(result["status"], "applied");
        assert_eq!(c[0]["description"], "Store document");
        let request = handle.join().unwrap();
        if provider != "generic" {
            assert_eq!(
                request["questions"]["s0"]["criteria"]["none"],
                "No suitable candidate"
            );
            assert!(request["state"].is_string());
        }
    }
}
#[test]
fn unknown_choices_and_remote_access_fail_without_mutating_shortlist() {
    let (url, handle) = server(json!({"order":["c999"]}));
    let mut c = candidates();
    let original = c.clone();
    assert!(ranker::rerank(
        &json!({"enabled":true,"provider":"generic","endpoint":url}),
        &plan(),
        &mut c,
        Duration::from_secs(2)
    )
    .is_err());
    handle.join().unwrap();
    assert_eq!(c, original);
    assert!(ranker::rerank(
        &json!({"enabled":true,"provider":"generic","endpoint":"https://example.com/rank"}),
        &plan(),
        &mut c,
        Duration::from_secs(2)
    )
    .is_err());
    assert_eq!(c, original);
    assert_eq!(
        ranker::rerank(&Value::Null, &plan(), &mut c, Duration::from_secs(2)).unwrap()["status"],
        "disabled"
    );
}
