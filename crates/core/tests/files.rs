use agent_market_core::{catalog, recovery, Runtime};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::io::Write;
fn object(p: Value, required: Value) -> Value {
    json!({"type":"object","properties":p,"required":required,"additionalProperties":false})
}
fn package(name: &str) -> Value {
    let mut p = json!({"format":2,"name":name,"version":"0.1.0","publisher":"test","description":"Managed file fixture","guide":"Test only","hosting":{"mode":"local"},"permissions":["state.read","state.write","files.read","files.write"],"objects":{},"actions":{},"tests":[]});
    for (name, properties, required) in [
        (
            "import",
            json!({"filename":{"type":"string"},"content_base64":{"type":"string"}}),
            json!(["filename", "content_base64"]),
        ),
        (
            "inspect",
            json!({"file_id":{"type":"string"}}),
            json!(["file_id"]),
        ),
        (
            "extract",
            json!({"file_id":{"type":"string"},"offset":{"type":"integer"},"limit":{"type":"integer"},"sheet":{"type":"string"},"pointer":{"type":"string"}}),
            json!(["file_id"]),
        ),
        (
            "export",
            json!({"filename":{"type":"string"},"text":{"type":"string"},"rows":{"type":"array","items":{"type":"array","items":{"type":"string"}}}}),
            json!(["filename"]),
        ),
        (
            "delete",
            json!({"file_id":{"type":"string"}}),
            json!(["file_id"]),
        ),
    ] {
        p["actions"][name] = json!({"description":name,"operation":format!("file_{name}"),"input":object(properties,required)});
    }
    p
}
fn call(r: &Runtime, action: &str, args: Value) -> agent_market_core::Result<Value> {
    r.call(
        "rhyven_call",
        json!({"category":"test/files","function":format!("action_{action}"),"args":args}),
    )
}
fn import(r: &Runtime, name: &str, bytes: &[u8]) -> Value {
    call(
        r,
        "import",
        json!({"filename":name,"content_base64":STANDARD.encode(bytes)}),
    )
    .unwrap()
}
#[test]
fn formats_pagination_retry_permissions_and_cross_scope_isolation() {
    let dir = tempfile::tempdir().unwrap();
    let r = Runtime::collection(dir.path(), "one", "tester").unwrap();
    let p = package("test/files");
    r.install(&p, true, false).unwrap();
    let csv = import(&r, "items.csv", b"name,quantity\nbolts,0012\nnuts,2\n");
    let rows = call(
        &r,
        "extract",
        json!({"file_id":csv["id"],"offset":1,"limit":1}),
    )
    .unwrap();
    assert_eq!(rows["rows"][0]["values"], json!(["bolts", "0012"]));
    assert_eq!(rows["next_offset"], 2);
    let source = import(&r, "items.json", br#"{"items":[{"id":1},{"id":2}]}"#);
    assert_eq!(
        call(
            &r,
            "extract",
            json!({"file_id":source["id"],"pointer":"/items"})
        )
        .unwrap()["rows"][1]["pointer"],
        "/items/1"
    );
    let txt = import(&r, "notes.txt", b"first\nsecond");
    assert_eq!(
        call(&r, "extract", json!({"file_id":txt["id"],"offset":1})).unwrap()["rows"][0]["line"],
        2
    );
    let args = json!({"filename":"safe.csv","rows":[["=1+1","normal"]],"request_id":"export-once"});
    let exported = call(&r, "export", args.clone()).unwrap();
    assert_eq!(call(&r, "export", args).unwrap(), exported);
    assert_eq!(
        call(&r, "extract", json!({"file_id":exported["id"]})).unwrap()["rows"][0]["values"][0],
        "'=1+1"
    );
    let other = Runtime::collection(dir.path(), "two", "tester").unwrap();
    other.install(&p, true, false).unwrap();
    assert!(call(&other, "inspect", json!({"file_id":txt["id"]})).is_err());
    r.install(&package("test/other"), true, false).unwrap();
    assert!(r
        .call(
            "execute",
            json!({"app":"test/other","action":"inspect","args":{"file_id":txt["id"]}})
        )
        .is_err());
    let mut bad = p;
    bad["permissions"] = json!(["state.read", "state.write"]);
    assert!(catalog::validate(&bad).is_err());
}
#[test]
fn backup_restore_retention_integrity_and_invalid_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let r = Runtime::collection(dir.path(), "one", "tester").unwrap();
    r.install(&package("test/files"), true, false).unwrap();
    for (name, bytes) in [
        ("../escape.txt", b"x".as_slice()),
        ("bad.json", b"{"),
        ("bad.csv", b"a,b\nc\n"),
        ("x.txt", b"\xff"),
    ] {
        assert!(call(
            &r,
            "import",
            json!({"filename":name,"content_base64":STANDARD.encode(bytes)})
        )
        .is_err());
    }
    assert!(call(
        &r,
        "import",
        json!({"filename":"huge.txt","content_base64":STANDARD.encode(vec![b'x';524289])})
    )
    .is_err());
    let source = import(&r, "notes.txt", b"retained");
    let archive = dir.path().join("backup.rhyven");
    recovery::backup(&r, &archive).unwrap();
    let restored = tempfile::tempdir().unwrap();
    recovery::restore(restored.path(), "copy", &archive, false).unwrap();
    let copy = Runtime::collection(restored.path(), "copy", "tester").unwrap();
    assert_eq!(
        call(&copy, "inspect", json!({"file_id":source["id"]})).unwrap(),
        source
    );
    r.uninstall("test/files").unwrap();
    r.install(&package("test/files"), true, false).unwrap();
    assert_eq!(
        call(&r, "inspect", json!({"file_id":source["id"]})).unwrap(),
        source
    );
    let path = r
        .root
        .join("containers")
        .join(agent_market_core::store::hash(&json!("test/files")))
        .join("data/files")
        .join(source["sha256"].as_str().unwrap());
    std::fs::write(&path, b"tampered").unwrap();
    assert!(call(&r, "extract", json!({"file_id":source["id"]})).is_err());
    call(&r, "delete", json!({"file_id":source["id"]})).unwrap();
    assert!(call(&r, "inspect", json!({"file_id":source["id"]})).is_err());
    assert!(!path.exists());
}
fn workbook(extra: Option<(&str, &str)>) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let mut files=vec![
        ("xl/workbook.xml","<workbook xmlns:r=\"r\"><sheets><sheet name=\"Data\" r:id=\"rId1\"/></sheets></workbook>"),
        ("xl/_rels/workbook.xml.rels","<Relationships><Relationship Id=\"rId1\" Target=\"worksheets/sheet1.xml\"/></Relationships>"),
        ("xl/sharedStrings.xml","<sst><si><t>A &amp; B</t></si></sst>"),
        ("xl/worksheets/sheet1.xml","<worksheet><sheetData><row r=\"3\"><c r=\"A3\" t=\"s\"><v>0</v></c><c r=\"B3\"><f>1+1</f><v>2</v></c><c r=\"C3\"><f t=\"shared\"/></c></row></sheetData></worksheet>")];
    if let Some(extra) = extra {
        files.push(extra);
    }
    for (name, body) in files {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(body.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
#[test]
fn xlsx_preserves_locations_entities_and_unverified_formulas_and_rejects_unsafe_archive() {
    let dir = tempfile::tempdir().unwrap();
    let r = Runtime::new(dir.path(), "tester").unwrap();
    r.install(&package("test/files"), true, false).unwrap();
    let source = import(&r, "book.xlsx", &workbook(None));
    let output = call(
        &r,
        "extract",
        json!({"file_id":source["id"],"sheet":"Data"}),
    )
    .unwrap();
    assert_eq!(output["rows"][0]["row"], 3);
    assert_eq!(output["rows"][0]["cells"][0]["raw"], "A & B");
    assert_eq!(
        output["rows"][0]["cells"][1]["formula_cache_unverified"],
        true
    );
    assert_eq!(
        output["rows"][0]["cells"][2]["formula_cache_unverified"],
        true
    );
    for extra in [
        ("../escape", "no"),
        ("xl/externalLinks/a.xml", "no"),
        ("xl/vbaProject.bin", "no"),
    ] {
        assert!(call(&r,"import",json!({"filename":"unsafe.xlsx","content_base64":STANDARD.encode(workbook(Some(extra)))})).is_err());
    }
}
