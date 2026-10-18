use super::*;
use ldf_model::{load, semantic_diff, semantically_equal};

/// 대부분의 구문을 포함한 샘플(직접 작성). 깨진 참조와 다중 인코딩, J2602 마스터도 있다.
const SAMPLE: &str = r#"
// 샘플 클러스터
LIN_description_file;
LIN_protocol_version = "2.1";
LIN_language_version = "2.1";
LIN_speed = 19.2 kbps;
Channel_name = "Body";

Nodes {
  Master: Gateway, 5 ms, 0.1 ms, 48 bits, 40 %;
  Slaves: Door, Lamp;
}
Node_composition { configuration Cfg { Cluster: Door, Lamp; } }
Signals {
  DoorState: 2, 0, Door, Gateway;
  LampCmd: 8, 0xFFFFFFFFFFFFFFFF, Gateway, Lamp, Door;
  LampPos: 32, {1, 2, 3, 4}, Lamp, Gateway;
  Fault: 1, 0, Lamp, Gateway;
  Orphan: 4, 0, Ghost, Gateway;
}
Diagnostic_signals { MasterReqB0: 8, 0; SlaveRespB0: 8, 0; }
Frames {
  DoorFrame: 0x10, Door, 1 { DoorState, 0; }
  LampFrame: 0x11, Gateway, 8 { LampCmd, 0; LampPos, 8; }
  LampStatus: 0x12, Lamp, 2 { Fault, 0; Missing, 8; }
  Empty: 0x13, Door, 1 { }
}
Sporadic_frames { Sporadic1: LampFrame, DoorFrame; }
Event_triggered_frames { Etf1: Collision, 0x20, DoorFrame, LampStatus; Etf2: 0x21, DoorFrame; }
Diagnostic_frames { MasterReq: 0x3c { MasterReqB0, 0; } SlaveResp: 0x3d { SlaveRespB0, 0; } }
Node_attributes {
  Lamp {
    LIN_protocol = "2.1"; configured_NAD = 0x20; initial_NAD = 0x21;
    product_id = 0x1a, 0x2b, 3; response_error = Fault; fault_state_signals = Fault;
    P2_min = 50 ms; ST_min = 0 ms; N_As_timeout = 1000 ms; N_Cr_timeout = 1000 ms;
    wakeup_time = 50 ms;
    configurable_frames { LampFrame; LampStatus = 0x1234; }
  }
}
Schedule_tables {
  Normal { LampFrame delay 10 ms; Etf1 delay 20 ms; MasterReq delay 10.5 ms; Nope delay 5 ms; }
  Collision { DoorFrame delay 10 ms; LampStatus delay 10 ms; }
  Config {
    AssignNAD { Lamp } delay 10 ms;
    ConditionalChangeNAD { 0x7f, 0x01, 0x02, 0x03, 0x00, 0x04 } delay 10 ms;
    DataDump { Lamp, 1, 2, 3, 4, 5 } delay 10 ms;
    SaveConfiguration { Lamp } delay 10 ms;
    AssignFrameIdRange { Lamp, 0, 0x10, 0x11 } delay 10 ms;
    AssignFrameId { Lamp, LampFrame } delay 10 ms;
    FreeFormat { 1, 2, 3, 4, 5, 6, 7, 8 } delay 10 ms;
  }
}
Signal_encoding_types {
  LampEnc { logical_value, 0, "off"; logical_value, 1, "on"; physical_value, 0, 100, 0.5, -2, "percent"; bcd_value; ascii_value; }
  Second { logical_value, 5, "five"; }
}
Signal_representation { LampEnc: LampCmd, DoorState; Second: LampCmd, Ghost; }
Vendor_extension { x = 1; }
"#;

fn rewrite(src: &str) -> (LdfModelAlias, String) {
    let m = load(src).model;
    let w = write(&m, &WriteOptions::default());
    (m, w.text)
}

type LdfModelAlias = ldf_model::LdfModel;

#[test]
fn the_sample_survives_a_write_read_round_trip() {
    let (m, text) = rewrite(SAMPLE);
    let again = load(&text);
    assert!(
        again
            .diagnostics
            .iter()
            .all(|d| d.severity != ldf_model::Severity::Error),
        "{:?}\n{text}",
        again.diagnostics
    );
    if let Some(d) = semantic_diff(&m, &again.model) {
        panic!("왕복 후 달라졌다: {d}\n--- 출력 ---\n{text}");
    }
}

#[test]
fn output_is_a_fixpoint_after_the_first_rewrite() {
    let (_, first) = rewrite(SAMPLE);
    let (_, second) = rewrite(&first);
    assert_eq!(first, second);
}

#[test]
fn section_order_and_values_follow_the_original() {
    let (_, text) = rewrite(SAMPLE);
    let pos = |needle: &str| {
        text.find(needle)
            .unwrap_or_else(|| panic!("`{needle}` 없음\n{text}"))
    };
    assert!(pos("LIN_speed = 19.2 kbps;") < pos("Nodes {"));
    assert!(pos("Nodes {") < pos("Node_composition {"));
    assert!(pos("Signals {") < pos("Frames {"));
    assert!(pos("Schedule_tables {") < pos("Vendor_extension {"));
    assert!(text.contains("Master: Gateway, 5 ms, 0.1 ms, 48 bits, 40 %;"));
    assert!(text.contains("LampPos: 32, {1, 2, 3, 4}, Lamp, Gateway;"));
    assert!(text.contains("DoorFrame: 16, Door, 1 {"));
    assert!(text.contains("MasterReq: 0x3C {"));
    assert!(text.contains("configured_NAD = 0x20;"));
    assert!(text.contains("MasterReq delay 10.5 ms;"));
    assert!(text.contains("physical_value, 0, 100, 0.5, -2, \"percent\";"));
    // 모르는 섹션과 해석하지 않은 속성은 원문 그대로
    assert!(text.contains("Vendor_extension { x = 1; }"));
    assert!(text.contains("wakeup_time = 50 ms;"));
    // 깨진 참조도 이름 그대로 남는다
    assert!(text.contains("Orphan: 4, 0, Ghost, Gateway;"));
    assert!(text.contains("Missing, 8;"));
}

#[test]
fn multiple_encodings_keep_their_order_per_signal() {
    let (m, text) = rewrite(SAMPLE);
    let again = load(&text).model;
    let order = |m: &ldf_model::LdfModel| -> Vec<String> {
        let s = m
            .signals
            .iter()
            .find(|(_, s)| s.name == "LampCmd")
            .map(|(_, s)| s)
            .expect("sig");
        s.encodings.iter().map(|r| format!("{r:?}")).collect()
    };
    assert_eq!(order(&m), order(&again));
    assert_eq!(order(&m).len(), 2);
}

#[test]
fn downgrading_to_2_0_drops_newer_items_and_warns() {
    let m = load(SAMPLE).model;
    let w = write(
        &m,
        &WriteOptions {
            target: Target::V2_0,
            ..WriteOptions::default()
        },
    );
    assert!(w.text.contains("LIN_protocol_version = \"2.0\";"));
    assert!(!w.text.contains("Channel_name"));
    assert!(!w.text.contains("initial_NAD"));
    assert!(!w.text.contains("Node_composition"));
    let all = w.warnings.join("\n");
    assert!(all.contains("Channel_name"), "{all}");
    assert!(all.contains("initial_NAD"), "{all}");
    assert!(all.contains("Node_composition"), "{all}");
    // 다시 읽어도 나머지는 같다
    let again = load(&w.text).model;
    assert_eq!(again.signals.len(), m.signals.len());
    assert_eq!(again.header.protocol_version.as_deref(), Some("2.0"));
}

#[test]
fn keeping_the_version_produces_no_warnings() {
    let m = load(SAMPLE).model;
    assert!(write(&m, &WriteOptions::default()).warnings.is_empty());
}

#[test]
fn iso17987_only_header_items_are_dropped_for_2x_targets() {
    let src = "LIN_description_file;\nLIN_protocol_version = \"ISO17987:2015\";\nLIN_language_version = \"ISO17987:2015\";\n\
               LDF_file_revision = \"1.2\";\nLIN_speed = 19.2 kbps;\nLIN_sig_byte_order_big_endian;\n";
    let m = load(src).model;
    let kept = write(&m, &WriteOptions::default());
    assert!(kept.text.contains("LDF_file_revision = \"1.2\";"));
    assert!(kept.text.contains("LIN_sig_byte_order_big_endian;"));
    assert!(semantically_equal(&m, &load(&kept.text).model));
    let w = write(
        &m,
        &WriteOptions {
            target: Target::V2_2,
            ..WriteOptions::default()
        },
    );
    assert!(!w.text.contains("LDF_file_revision"));
    assert!(!w.text.contains("big_endian"));
    assert_eq!(w.warnings.len(), 3, "{:?}", w.warnings); // 버전 변경 + 두 항목
}

#[test]
fn crlf_output_uses_crlf_everywhere() {
    let m = load(SAMPLE).model;
    let w = write(
        &m,
        &WriteOptions {
            eol: Eol::CrLf,
            ..WriteOptions::default()
        },
    );
    assert!(w.text.contains("\r\n"));
    assert!(!w.text.replace("\r\n", "").contains('\n'));
    assert!(semantically_equal(&m, &load(&w.text).model));
}

#[test]
fn empty_and_minimal_models_write_without_panicking() {
    let w = write(&ldf_model::LdfModel::default(), &WriteOptions::default());
    assert!(w.text.is_empty());
    let m = load("LIN_description_file;").model;
    assert_eq!(
        write(&m, &WriteOptions::default()).text,
        "LIN_description_file;\n"
    );
}

#[test]
fn empty_sections_that_existed_in_the_source_are_kept() {
    let m = load("Nodes { }\nSignals { }").model;
    let text = write(&m, &WriteOptions::default()).text;
    assert!(text.contains("Nodes {\n}"));
    assert!(text.contains("Signals {\n}"));
}

#[test]
fn quotes_inside_strings_cannot_break_the_output() {
    let mut m = load("LIN_description_file;\nChannel_name = \"x\";").model;
    m.header.channel_name = Some("a\"b".to_owned());
    let text = write(&m, &WriteOptions::default()).text;
    let again = load(&text).model;
    assert_eq!(again.header.channel_name.as_deref(), Some("a'b"));
}

#[test]
fn broken_inputs_never_panic_the_writer() {
    for src in [
        "",
        "{",
        "Nodes {",
        "Signals { : ; , , }",
        "Frames { F: 1, M { , ; } }",
        "@@ \"x",
        &"{".repeat(5000),
    ] {
        let m = load(src).model;
        let _ = write(&m, &WriteOptions::default());
    }
}

#[test]
fn node_composition_round_trips_in_both_forms_and_keeps_its_place() {
    for (kw, body) in [
        (
            "composite",
            "configuration c1 { Phys { A, B; } Other { C; } }",
        ),
        (
            "Node_composition",
            "configuration c1 { Phys: A, B; Other: C; }",
        ),
    ] {
        let src = format!(
            "LIN_description_file;\nLIN_protocol_version = \"2.1\";\nNodes {{ Master: M, 1 ms, 0 ms; Slaves: A, B, C; }}\n{kw} {{ {body} }}\nSignals {{ S: 8, 0, M, A; }}\n"
        );
        let m = load(&src).model;
        assert!(m.composition.is_some(), "{kw}");
        let w = write(&m, &WriteOptions::default()).text;
        assert!(semantically_equal(&m, &load(&w).model), "{kw}\n{w}");
        // 순서: Nodes 다음, Signals 앞
        let pos = |s: &str| w.find(s).unwrap_or(usize::MAX);
        assert!(pos("Nodes") < pos(kw) && pos(kw) < pos("Signals"), "{w}");
        // 같은 모양으로 다시 쓴다
        assert_eq!(w.contains("Phys:"), kw == "Node_composition", "{w}");
    }
}

#[test]
fn node_composition_is_dropped_for_2_0_but_composite_is_kept() {
    let src = |kw: &str| {
        format!(
            "LIN_description_file;\nLIN_protocol_version = \"2.1\";\nNodes {{ Master: M, 1 ms, 0 ms; Slaves: A; }}\n{kw} {{ configuration c {{ P {{ A; }} }} }}\n"
        )
    };
    let w = write(
        &load(&src("Node_composition")).model,
        &WriteOptions {
            target: Target::V2_0,
            ..Default::default()
        },
    );
    assert!(!w.text.contains("Node_composition"));
    assert!(
        w.warnings.iter().any(|x| x.contains("Node_composition")),
        "{:?}",
        w.warnings
    );
    let w = write(
        &load(&src("composite")).model,
        &WriteOptions {
            target: Target::V2_0,
            ..Default::default()
        },
    );
    assert!(w.text.contains("composite"));
}
