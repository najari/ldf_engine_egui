use super::*;
use crate::load;
use serde_json::Value;

/// 스키마에서 쓰는 JSON Schema 부분집합(type, enum, const, required, properties, items, oneOf, $ref, minimum)만 검사하는
/// 최소 검증기. 외부 의존성 없이 내보낸 JSON이 `docs/model.schema.json`을 지키는지 확인한다.
fn validate(root: &Value, schema: &Value, v: &Value, path: &str, errs: &mut Vec<String>) {
    if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
        let mut target = root;
        for part in r.trim_start_matches("#/").split('/') {
            target = target.get(part).unwrap_or(&Value::Null);
        }
        return validate(root, target, v, path, errs);
    }
    if let Some(c) = schema.get("const")
        && c != v
    {
        errs.push(format!("{path}: const {c} != {v}"));
    }
    if let Some(e) = schema.get("enum").and_then(Value::as_array)
        && !e.contains(v)
    {
        errs.push(format!("{path}: {v}은(는) {e:?}에 없다"));
    }
    if let Some(t) = schema.get("type") {
        let types: Vec<&str> = match t {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        let ok = types.iter().any(|t| match *t {
            "string" => v.is_string(),
            "integer" => v.is_i64() || v.is_u64(),
            "number" => v.is_number(),
            "boolean" => v.is_boolean(),
            "null" => v.is_null(),
            "array" => v.is_array(),
            "object" => v.is_object(),
            _ => false,
        });
        if !ok {
            errs.push(format!("{path}: 형식 {types:?}이(가) 아니다: {v}"));
            return;
        }
    }
    if let Some(min) = schema.get("minimum").and_then(Value::as_f64)
        && let Some(n) = v.as_f64()
        && n < min
    {
        errs.push(format!("{path}: {n} < {min}"));
    }
    if let Some(obj) = v.as_object() {
        for req in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(k) = req.as_str()
                && !obj.contains_key(k)
            {
                errs.push(format!("{path}: 필수 키 `{k}`가 없다"));
            }
        }
        if let Some(props) = schema.get("properties").and_then(Value::as_object) {
            for (k, sub) in props {
                if let Some(child) = obj.get(k) {
                    validate(root, sub, child, &format!("{path}.{k}"), errs);
                }
            }
        }
    }
    if let (Some(items), Some(arr)) = (schema.get("items"), v.as_array()) {
        for (i, child) in arr.iter().enumerate() {
            validate(root, items, child, &format!("{path}[{i}]"), errs);
        }
    }
    if let Some(one) = schema.get("oneOf").and_then(Value::as_array) {
        let matching = one
            .iter()
            .filter(|s| {
                let mut e = Vec::new();
                validate(root, s, v, path, &mut e);
                e.is_empty()
            })
            .count();
        if matching != 1 {
            errs.push(format!("{path}: oneOf에 {matching}개가 맞는다: {v}"));
        }
    }
}

fn schema() -> Value {
    // 저장소의 docs/model.schema.json을 그대로 읽는다. 스키마와 코드가 어긋나면 이 시험이 실패한다.
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/model.schema.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).expect("스키마가 JSON이어야 한다")
}

const SAMPLE: &str = r#"
// 머리말
LIN_description_file;
LIN_protocol_version = "2.1";
LIN_language_version = "2.1";
LIN_speed = 19.2 kbps;
Nodes { Master: M, 5 ms, 0.1 ms; Slaves: A, B; }
composite { configuration cfg { Phys { A, B; } } }
Signals { Cmd: 8, 0, M, A, B; Pos: 16, 0, A, M; Blob: 16, {1, 2}, B, M; Orphan: 4, 0, Ghost, M; }
Diagnostic_signals { MasterReqB0: 8, 0; }
Frames { FCmd: 1, M, 2 { Cmd, 0; } FA: 2, A, 2 { Pos, 0; } FB: 3, B, 2 { Blob, 0; } }
Sporadic_frames { SP: FCmd; }
Event_triggered_frames { ETF: Coll, 10, FA, FB; }
Diagnostic_frames { MasterReq: 0x3c { MasterReqB0, 0; } }
Node_attributes {
  A { LIN_protocol = "2.1"; configured_NAD = 1; product_id = 1, 2, 3; response_error = Pos; wakeup_time = 50 ms;
      configurable_frames { FCmd; FA; ETF; } }
}
Schedule_tables {
  Main { FCmd delay 10 ms; FA delay 10 ms; AssignNAD { A } delay 10 ms; AssignFrameIdRange { A, 0, 0xC1 } delay 10 ms;
         ConditionalChangeNAD { 1, 2, 3, 4, 5, 6 } delay 10 ms; DataDump { A, 1, 2, 3, 4, 5 } delay 10 ms;
         SaveConfiguration { A } delay 10 ms; AssignFrameId { A, FA } delay 10 ms; FreeFormat { 1, 2, 3, 4, 5, 6, 7, 8 } delay 10 ms; }
  Coll { FA delay 10 ms; FB delay 10 ms; }
}
Signal_encoding_types { Enc { logical_value, 0, "off"; physical_value, 0, 255, 1, 0, "u"; bcd_value; } Asc { ascii_value; } }
Signal_representation { Enc: Cmd; Asc: Blob; }
Vendor_extension { x = 1; }
"#;

#[test]
fn the_export_follows_the_published_schema() {
    let m = load(SAMPLE).model;
    let json = to_json(&m);
    let mut errs = Vec::new();
    let s = schema();
    validate(&s, &s, &json, "$", &mut errs);
    assert!(errs.is_empty(), "{errs:#?}\n{json:#}");
}

#[test]
fn objects_refer_to_each_other_by_name() {
    let m = load(SAMPLE).model;
    let j = to_json(&m);
    assert_eq!(j["header"]["protocol_version"], "2.1");
    assert_eq!(j["header"]["speed_bps"], 19200);
    assert_eq!(j["header"]["leading_comment"], "// 머리말");
    let fa = j["frames"]
        .as_array()
        .and_then(|f| f.iter().find(|f| f["name"] == "FA"))
        .expect("FA");
    assert_eq!(fa["publisher"], "A");
    assert_eq!(fa["signals"][0]["signal"], "Pos");
    let etf = j["frames"]
        .as_array()
        .and_then(|f| f.iter().find(|f| f["name"] == "ETF"))
        .expect("ETF");
    assert_eq!(etf["collision_table"], "Coll");
    assert_eq!(etf["associated"], serde_json::json!(["FA", "FB"]));
    let blob = j["signals"]
        .as_array()
        .and_then(|s| s.iter().find(|s| s["name"] == "Blob"))
        .expect("Blob");
    assert_eq!(blob["init"], serde_json::json!({ "bytes": [1, 2] }));
    assert_eq!(blob["encodings"], serde_json::json!(["Asc"]));
    // 정의되지 않은 발행자도 이름 그대로 남는다
    let orphan = j["signals"]
        .as_array()
        .and_then(|s| s.iter().find(|s| s["name"] == "Orphan"))
        .expect("Orphan");
    assert_eq!(orphan["publisher"], "Ghost");
    // 모델이 해석하지 않은 것은 원문으로 나간다
    assert_eq!(j["unknown_sections"][0]["name"], "Vendor_extension");
    assert_eq!(j["node_attributes"][0]["extra"][0]["key"], "wakeup_time");
    // 노드 구성은 모델로 나간다(원문 보관 섹션에 없다)
    assert_eq!(
        j["composition"],
        serde_json::json!({
            "keyword": "composite",
            "configurations": [{ "name": "cfg", "composites": [{ "name": "Phys", "logical": ["A", "B"] }] }],
        })
    );
    assert_eq!(j["unknown_sections"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        to_json(&LdfModel::default())["composition"],
        serde_json::Value::Null
    );
}

#[test]
fn an_empty_model_exports_a_valid_document() {
    let json = to_json(&LdfModel::default());
    let mut errs = Vec::new();
    let s = schema();
    validate(&s, &s, &json, "$", &mut errs);
    assert!(errs.is_empty(), "{errs:#?}");
    assert_eq!(json["nodes"], serde_json::json!([]));
}

#[test]
fn the_validator_actually_rejects_bad_documents() {
    let s = schema();
    let mut json = to_json(&load(SAMPLE).model);
    json["nodes"][0]["role"] = serde_json::json!("king");
    json["signals"][0]["size_bits"] = serde_json::json!(-1);
    json.as_object_mut().expect("object").remove("frames");
    let mut errs = Vec::new();
    validate(&s, &s, &json, "$", &mut errs);
    assert!(errs.iter().any(|e| e.contains("king")), "{errs:?}");
    assert!(
        errs.iter()
            .any(|e| e.contains("size_bits") || e.contains("< 0")),
        "{errs:?}"
    );
    assert!(errs.iter().any(|e| e.contains("frames")), "{errs:?}");
}
