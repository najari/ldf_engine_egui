//! 모델 변환 테스트. 샘플 LDF는 직접 작성한 것이다(상용 도구의 테스트 파일은 쓰지 않는다).

use super::*;

/// 대부분의 섹션을 갖춘 LIN 2.1 샘플.
const SAMPLE: &str = r#"
// 샘플 클러스터
LIN_description_file;
LIN_protocol_version = "2.1";
LIN_language_version = "2.1";
LIN_speed = 19.2 kbps;
Channel_name = "Body";

Nodes {
  Master: Gateway, 5 ms, 0.1 ms;
  Slaves: Door, Lamp;
}

Node_composition {
  configuration Cfg { Cluster: Door, Lamp; }
}

Signals {
  DoorState: 2, 0, Door, Gateway;
  LampCmd: 8, 0x10, Gateway, Lamp, Door;
  LampPos: 32, {1, 2, 3, 4}, Lamp, Gateway;
  Fault: 1, 0, Lamp, Gateway;
  Orphan: 4, 0, Ghost, Gateway;
}

Diagnostic_signals {
  MasterReqB0: 8, 0;
  SlaveRespB0: 8, 0;
}

Frames {
  DoorFrame: 0x10, Door, 1 {
    DoorState, 0;
  }
  LampFrame: 0x11, Gateway, 8 {
    LampCmd, 0;
    LampPos, 8;
  }
  LampStatus: 0x12, Lamp, 2 {
    Fault, 0;
    Missing, 8;
  }
}

Sporadic_frames {
  Sporadic1: LampFrame, DoorFrame;
}

Event_triggered_frames {
  Etf1: Collision, 0x20, DoorFrame, LampStatus;
}

Diagnostic_frames {
  MasterReq: 0x3c {
    MasterReqB0, 0;
  }
  SlaveResp: 0x3d {
    SlaveRespB0, 0;
  }
}

Node_attributes {
  Lamp {
    LIN_protocol = "2.1";
    configured_NAD = 0x20;
    initial_NAD = 0x20;
    product_id = 0x1a, 0x2b, 3;
    response_error = Fault;
    fault_state_signals = Fault;
    P2_min = 50 ms;
    ST_min = 0 ms;
    N_As_timeout = 1000 ms;
    N_Cr_timeout = 1000 ms;
    configurable_frames {
      LampFrame;
      LampStatus;
    }
  }
}

Schedule_tables {
  Normal {
    LampFrame delay 10 ms;
    DoorFrame delay 10 ms;
    Etf1 delay 20 ms;
    MasterReq delay 10 ms;
  }
  Collision {
    DoorFrame delay 10 ms;
    LampStatus delay 10 ms;
  }
  Config {
    AssignNAD { Lamp } delay 10 ms;
    FreeFormat { 1, 2, 3, 4, 5, 6, 7, 8 } delay 10 ms;
  }
}

Signal_encoding_types {
  LampEnc {
    logical_value, 0, "off";
    logical_value, 1, "on";
    physical_value, 0, 100, 0.5, 0, "percent";
  }
}

Signal_representation {
  LampEnc: LampCmd, Ghost;
}

Vendor_extension {
  x = 1;
}
"#;

fn loaded() -> Loaded {
    load(SAMPLE)
}

#[test]
fn header_is_read() {
    let m = loaded().model;
    assert!(m.header.has_description_marker);
    assert_eq!(m.header.protocol_version.as_deref(), Some("2.1"));
    assert_eq!(m.header.language_version.as_deref(), Some("2.1"));
    assert_eq!(m.header.speed_bps, Some(19_200));
    assert_eq!(m.header.channel_name.as_deref(), Some("Body"));
}

#[test]
fn nodes_and_roles() {
    let m = loaded().model;
    let nodes: Vec<_> = m
        .nodes
        .iter()
        .map(|(_, n)| (n.name.as_str(), n.role))
        .collect();
    assert_eq!(
        nodes,
        [
            ("Gateway", NodeRole::Master),
            ("Door", NodeRole::Slave),
            ("Lamp", NodeRole::Slave)
        ]
    );
    let master = m.nodes.get(NodeId(0)).expect("master");
    assert_eq!(master.timebase_ms, Some(5.0));
    assert_eq!(master.jitter_ms, Some(0.1));
}

#[test]
fn signals_resolve_to_node_ids() {
    let m = loaded().model;
    let lamp_cmd = m
        .signals
        .iter()
        .find(|(_, s)| s.name == "LampCmd")
        .map(|(_, s)| s)
        .expect("sig");
    assert_eq!(lamp_cmd.size_bits, 8);
    assert_eq!(lamp_cmd.init, InitValue::Scalar(0x10));
    assert_eq!(lamp_cmd.publisher, Some(Ref::Resolved(NodeId(0))));
    assert_eq!(
        lamp_cmd.subscribers,
        [Ref::Resolved(NodeId(2)), Ref::Resolved(NodeId(1))]
    );
    assert_eq!(lamp_cmd.encodings, [Ref::Resolved(EncodingId(0))]);

    let pos = m
        .signals
        .iter()
        .find(|(_, s)| s.name == "LampPos")
        .map(|(_, s)| s)
        .expect("sig");
    assert_eq!(pos.init, InitValue::Array(vec![1, 2, 3, 4]));
}

#[test]
fn unresolved_references_are_kept_by_name() {
    let m = loaded().model;
    let orphan = m
        .signals
        .iter()
        .find(|(_, s)| s.name == "Orphan")
        .map(|(_, s)| s)
        .expect("sig");
    assert_eq!(orphan.publisher, Some(Ref::Unresolved("Ghost".into())));

    let status = m
        .frames
        .iter()
        .find(|(_, f)| f.name == "LampStatus")
        .map(|(_, f)| f)
        .expect("f");
    let sigs = status.signals();
    assert_eq!(sigs.len(), 2);
    assert!(sigs[0].signal.id().is_some());
    assert_eq!(sigs[1].signal, Ref::Unresolved("Missing".into()));
}

#[test]
fn representation_to_unknown_signal_is_dropped_with_a_warning() {
    let l = loaded();
    assert!(
        l.diagnostics
            .iter()
            .any(|d| d.code == codes::UNRESOLVED_REPRESENTATION)
    );
}

#[test]
fn frames_by_kind() {
    let m = loaded().model;
    let by_name = |n: &str| {
        m.frames
            .iter()
            .find(|(_, f)| f.name == n)
            .map(|(_, f)| f)
            .expect(n)
    };

    let door = by_name("DoorFrame");
    assert_eq!(door.lin_id(), Some(0x10));
    assert_eq!(door.size_bytes(), Some(1));
    assert!(matches!(
        &door.kind,
        FrameKind::Unconditional {
            publisher: Ref::Resolved(_),
            ..
        }
    ));

    let sp = by_name("Sporadic1");
    assert_eq!(sp.lin_id(), None);
    assert!(
        matches!(&sp.kind, FrameKind::Sporadic { associated } if associated.iter().all(|r| r.id().is_some()))
    );

    let etf = by_name("Etf1");
    assert_eq!(etf.lin_id(), Some(0x20));
    let FrameKind::EventTriggered {
        collision_table,
        associated,
        ..
    } = &etf.kind
    else {
        panic!("etf")
    };
    assert_eq!(*collision_table, Some(Ref::Resolved(ScheduleId(1))));
    assert_eq!(associated.len(), 2);

    let req = by_name("MasterReq");
    assert_eq!(req.lin_id(), Some(0x3c));
    assert_eq!(req.size_bytes(), Some(8));
}

#[test]
fn node_attributes_are_attached() {
    let m = loaded().model;
    assert_eq!(m.node_attrs.len(), 1);
    let a = &m.node_attrs[0];
    assert_eq!(a.node, Ref::Resolved(NodeId(2)));
    assert_eq!(a.configured_nad, Some(0x20));
    assert_eq!(
        a.product_id,
        Some(ProductId {
            supplier: 0x1a,
            function: 0x2b,
            variant: Some(3)
        })
    );
    assert!(a.response_error.as_ref().and_then(Ref::id).is_some());
    assert_eq!(a.p2_min_ms, Some(50.0));
    assert_eq!(a.configurable_frames.len(), 2);
    assert!(a.configurable_frames.iter().all(|c| c.frame.id().is_some()));
}

#[test]
fn schedule_slots_and_commands() {
    let m = loaded().model;
    let normal = m.schedules.get(ScheduleId(0)).expect("sched");
    assert_eq!(normal.name, "Normal");
    assert_eq!(normal.slots.len(), 4);
    assert!(matches!(
        &normal.slots[0].entry,
        SlotEntry::Frame(Ref::Resolved(_))
    ));
    assert_eq!(normal.slots[0].delay_ms, 10.0);
    // MasterReq는 진단 프레임으로 해석된다
    assert!(matches!(
        &normal.slots[3].entry,
        SlotEntry::Frame(Ref::Resolved(_))
    ));

    let config = m.schedules.get(ScheduleId(2)).expect("sched");
    assert_eq!(
        config.slots[0].entry,
        SlotEntry::AssignNad {
            node: Ref::Resolved(NodeId(2))
        }
    );
    assert_eq!(
        config.slots[1].entry,
        SlotEntry::FreeFormat {
            data: vec![1, 2, 3, 4, 5, 6, 7, 8]
        }
    );
}

#[test]
fn encodings_are_parsed() {
    let m = loaded().model;
    let enc = m.encodings.get(EncodingId(0)).expect("enc");
    assert_eq!(enc.entries.len(), 3);
    assert_eq!(
        enc.entries[0],
        EncodingEntry::Logical {
            value: 0,
            text: "off".into()
        }
    );
    assert_eq!(
        enc.entries[2],
        EncodingEntry::Physical {
            min: 0,
            max: 100,
            scale: 0.5,
            offset: 0.0,
            unit: "percent".into()
        }
    );
}

#[test]
fn logical_value_text_is_optional() {
    // LIN 2.2A §9.2.6.1: `logical_value, <값> ( , <설명>) ;`
    let l = load(
        "Signal_encoding_types { E { logical_value, 0; logical_value, 1, \"on\"; physical_value, 2, 9, 1, 0; } }",
    );
    assert!(l.diagnostics.is_empty(), "{:?}", l.diagnostics);
    let enc = l.model.encodings.get(EncodingId(0)).expect("enc");
    assert_eq!(
        enc.entries[0],
        EncodingEntry::Logical {
            value: 0,
            text: String::new()
        }
    );
    assert_eq!(enc.entries.len(), 3);
}

#[test]
fn malformed_logical_value_is_still_an_error() {
    // 값이 없거나 숫자가 아니면 오류로 남는다
    for body in ["logical_value;", "logical_value, x, \"a\";"] {
        let l = load(&format!("Signal_encoding_types {{ E {{ {body} }} }}"));
        assert_eq!(l.diagnostics.len(), 1, "{body}: {:?}", l.diagnostics);
    }
}

#[test]
fn unknown_and_unmodelled_sections_are_kept_raw() {
    let l = loaded();
    let names: Vec<_> = l
        .model
        .unknown_sections
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    // Node_composition은 모델로 읽으므로 원문 보관 목록에 없다
    assert_eq!(names, ["Vendor_extension"]);
    let warned: Vec<_> = l
        .diagnostics
        .iter()
        .filter(|d| d.code == codes::UNKNOWN_SECTION)
        .collect();
    assert_eq!(warned.len(), 1);
    assert!(warned[0].message.contains("Vendor_extension"));
    assert!(l.model.unknown_sections[0].text.contains("x = 1"));
}

#[test]
fn composition_is_modelled_in_both_forms() {
    // 명세 §9.2.2.3 형식
    let l = load(
        "composite { configuration c1 { Phys { A, B; } Other { C; } } configuration c2 { Phys { A; } } }",
    );
    assert!(l.diagnostics.is_empty(), "{:?}", l.diagnostics);
    let c = l.model.composition.expect("composition");
    assert_eq!(c.keyword, "composite");
    assert!(!c.colon_form);
    assert_eq!(c.configurations.len(), 2);
    assert_eq!(
        c.configurations[0].composites,
        vec![
            CompositeNode {
                name: "Phys".into(),
                logical: vec!["A".into(), "B".into()]
            },
            CompositeNode {
                name: "Other".into(),
                logical: vec!["C".into()]
            },
        ]
    );
    // `이름: 논리, …;` 형식, 마지막 `;`가 없는 형식
    let l = load("Node_composition { configuration c { Phys: A, B; } }");
    let c = l.model.composition.expect("composition");
    assert_eq!(
        (c.keyword.as_str(), c.colon_form),
        ("Node_composition", true)
    );
    let l = load("composite { configuration c { RSM { logicalNode } } }");
    assert_eq!(
        l.model.composition.expect("composition").configurations[0].composites[0].logical,
        ["logicalNode"]
    );
}

#[test]
fn composition_with_an_unexpected_shape_stays_raw() {
    for src in [
        "composite { }",
        "composite { configuration c { } }",
        "composite { configuration c { P { } } }",
        "composite { configuration { P { A; } } }",
        "composite { other c { P { A; } } }",
        "composite { configuration c { P { A = 1; } } }",
    ] {
        let l = load(src);
        assert!(l.model.composition.is_none(), "{src}");
        assert_eq!(l.model.unknown_sections.len(), 1, "{src}");
    }
    // 두 번째 구성 섹션은 원문으로 남는다
    let l = load(
        "composite { configuration c { P { A; } } }\ncomposite { configuration d { Q { B; } } }",
    );
    assert!(l.model.composition.is_some());
    assert_eq!(l.model.unknown_sections.len(), 1);
}

#[test]
fn section_order_is_recorded() {
    let m = loaded().model;
    assert_eq!(
        m.section_order.first().map(String::as_str),
        Some("LIN_description_file")
    );
    assert_eq!(
        m.section_order.last().map(String::as_str),
        Some("Vendor_extension")
    );
}

#[test]
fn sample_has_no_syntax_errors() {
    let l = loaded();
    let errors: Vec<_> = l
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn duplicate_names_are_diagnosed_and_first_wins() {
    let src = "Nodes { Master: M, 1 ms, 0 ms; Slaves: A, A; }\n\
               Signals { S: 1, 0, M, A; S: 2, 0, M, A; }\n\
               Frames { F: 1, M, 1 { S, 0; } }";
    let l = load(src);
    assert_eq!(
        l.diagnostics
            .iter()
            .filter(|d| d.code == codes::DUPLICATE_NAME)
            .count(),
        2
    );
    let FrameKind::Unconditional { signals, .. } = &l.model.frames.get(FrameId(0)).expect("f").kind
    else {
        panic!("frame")
    };
    assert_eq!(signals[0].signal, Ref::Resolved(SignalId(0)));
}

#[test]
fn structural_errors_are_diagnosed_but_the_model_keeps_going() {
    let src = "LIN_speed = fast;\n\
               Nodes { Master: M, 1 ms, 0 ms; Slaves: A; }\n\
               Signals { Bad: x, 0, M, A; Good: 4, 0, M, A; }\n\
               Frames { F: 1, M { Good, 0; } }";
    let l = load(src);
    assert!(l.diagnostics.iter().any(|d| d.code == codes::BAD_HEADER));
    assert!(
        l.diagnostics
            .iter()
            .any(|d| d.code == codes::EXPECTED_NUMBER)
    );
    assert_eq!(l.model.signals.len(), 2);
    assert_eq!(l.model.frames.len(), 1);
}

#[test]
fn event_triggered_frame_without_collision_table() {
    let src = "Nodes { Master: M, 1 ms, 0 ms; Slaves: A; }\n\
               Signals { S: 1, 0, A, M; }\n\
               Frames { F: 1, A, 1 { S, 0; } }\n\
               Event_triggered_frames { E: 0x2a, F; }";
    let l = load(src);
    let etf = l
        .model
        .frames
        .iter()
        .find(|(_, f)| f.name == "E")
        .map(|(_, f)| f)
        .expect("etf");
    assert_eq!(etf.lin_id(), Some(0x2a));
    assert!(matches!(
        &etf.kind,
        FrameKind::EventTriggered {
            collision_table: None,
            ..
        }
    ));
}

#[test]
fn speed_units() {
    // 값은 읽지만 명세에 없는 단위는 경고한다(LIN 2.2A §9.2.1.3)
    let l = load("LIN_speed = 9600 bps;");
    assert_eq!(l.model.header.speed_bps, Some(9600));
    assert_eq!(l.diagnostics.len(), 1, "{:?}", l.diagnostics);
    assert_eq!(l.diagnostics[0].code, 9110);
    let l = load("LIN_speed = 10.4 kbps;");
    assert_eq!(l.model.header.speed_bps, Some(10_400));
    assert!(l.diagnostics.is_empty());
    let l = load("LIN_speed = 19.2;");
    assert_eq!(l.model.header.speed_bps, Some(19_200));
    assert_eq!(l.diagnostics.len(), 1);
    assert_eq!(
        load("LIN_speed = 1 mbps;").model.header.speed_bps,
        Some(1_000_000)
    );
}

#[test]
fn iso17987_header_items() {
    let src = "LIN_description_file;\n\
               LIN_protocol_version = \"ISO17987:2015\";\n\
               LIN_language_version = \"ISO17987:2015\";\n\
               LDF_file_revision = \"1.2.3\";\n\
               LIN_speed = 19.2 kbps;\n\
               LIN_sig_byte_order_big_endian;\n";
    let l = load(src);
    assert!(l.diagnostics.is_empty(), "{:?}", l.diagnostics);
    assert_eq!(l.model.header.ldf_file_revision.as_deref(), Some("1.2.3"));
    assert!(l.model.header.sig_byte_order_big_endian);
}

#[test]
fn j2602_master_extras_and_percent_values() {
    let src = "Nodes { Master: TurnControl, 1 ms, 0 ms, 48 bits, 40 %; Slaves: Left; }\n\
               Node_attributes { Left { response_tolerance = 30 %; wakeup_time = 50 ms; } }";
    let l = load(src);
    assert!(l.diagnostics.is_empty(), "{:?}", l.diagnostics);
    let master = l.model.nodes.get(NodeId(0)).expect("master");
    assert_eq!(master.max_header_bits, Some(48));
    assert_eq!(master.response_tolerance_pct, Some(40.0));
    // J2602 전용 속성은 해석하지 않고 원문으로 보관한다
    let keys: Vec<_> = l.model.node_attrs[0]
        .extra
        .iter()
        .map(|a| a.key.as_str())
        .collect();
    assert_eq!(keys, ["response_tolerance", "wakeup_time"]);
}

#[test]
fn dynamic_frames_are_kept_raw_without_warning() {
    let l = load("dynamic_frames { Dyn: 0x10; }");
    assert_eq!(l.model.unknown_sections.len(), 1);
    assert!(
        l.diagnostics
            .iter()
            .all(|d| d.code != codes::UNKNOWN_SECTION)
    );
}

#[test]
fn model_serializes_to_json() {
    let json = serde_json::to_string(&loaded().model).expect("json");
    assert!(json.contains("\"LampCmd\""));
}

#[test]
fn arbitrary_garbage_never_panics() {
    for src in [
        "",
        "{",
        "}",
        "Nodes",
        "Nodes {",
        "Nodes { Master: ; }",
        "Signals { : ; , , }",
        "Frames { F: 1, M, 1 { , ; } }",
        "Schedule_tables { T { delay delay delay; AssignNAD { } delay; } }",
        "Signal_encoding_types { E { logical_value; physical_value, 1; } }",
        "Node_attributes { N { product_id = ; configurable_frames { ; } } }",
        "\u{feff}@@@ \"unterminated",
        &"{".repeat(10_000),
        &"Nodes { ".repeat(2_000),
    ] {
        let _ = load(src);
    }
}
