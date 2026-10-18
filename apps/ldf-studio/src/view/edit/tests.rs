use super::*;
use ldf_model::{load, semantically_equal};

const SAMPLE: &str = r#"
LIN_description_file;
LIN_protocol_version = "2.1";
LIN_language_version = "2.1";
LIN_speed = 19.2 kbps;
Nodes { Master: M, 5 ms, 0.1 ms; Slaves: A, B; }
Signals {
  Cmd: 8, 0, M, A, B;
  Pos: 16, 0, A, M;
  Err: 1, 0, A, M;
  Blob: 16, {1, 2}, B, M;
}
Frames {
  FCmd: 1, M, 2 { Cmd, 0; }
  FA: 2, A, 4 { Pos, 0; Err, 16; }
  FB: 3, B, 2 { Blob, 0; }
}
Sporadic_frames { SP: FCmd; }
Event_triggered_frames { ETF: Coll, 10, FA, FB; }
Node_attributes {
  A {
    LIN_protocol = "2.1"; configured_NAD = 1; product_id = 1, 2, 3; response_error = Err;
    P2_min = 50 ms; ST_min = 0 ms; configurable_frames { FCmd; FA; ETF; }
  }
  B {
    LIN_protocol = "2.1"; configured_NAD = 2; product_id = 1, 2, 3; response_error = Err;
    P2_min = 50 ms; ST_min = 0 ms; configurable_frames { FCmd; FB; ETF; }
  }
}
Schedule_tables {
  Main { FCmd delay 10 ms; FA delay 10 ms; AssignNAD { A } delay 10 ms; AssignFrameIdRange { A, 0, 0xC1 } delay 10 ms; SP delay 10 ms; }
  Coll { FA delay 10 ms; FB delay 10 ms; }
}
Signal_encoding_types { Enc { logical_value, 0, "off"; physical_value, 0, 255, 1, 0, "u"; } }
Signal_representation { Enc: Err; }
"#;

fn model() -> LdfModel {
    load(SAMPLE).model
}

fn apply(m: &LdfModel, op: &EditOp) -> LdfModel {
    let mut c = m.clone();
    op.apply(&mut c).unwrap_or_else(|e| panic!("{op:?}: {e}"));
    c
}

#[test]
fn integers_and_lists_parse() {
    assert_eq!(parse_uint(" 12 "), Some(12));
    assert_eq!(parse_uint("0x3C"), Some(60));
    assert_eq!(parse_uint("abc"), None);
    assert_eq!(parse_list("1, 0x2,3"), Some(vec![1, 2, 3]));
    assert_eq!(parse_list(""), Some(vec![]));
    assert_eq!(parse_list("1,,2"), None);
}

#[test]
fn opening_and_confirming_unchanged_dialogs_changes_nothing() {
    let m = model();
    for (_, f) in m.frames.iter() {
        let id = m.find_frame(&f.name).expect("frame");
        let ui = FrameUi::from_frame(&m, id).expect("dialog");
        assert_eq!(
            apply(&m, &ui.to_op(&m).expect("op")),
            m,
            "프레임 {}",
            f.name
        );
    }
    for (id, s) in m.signals.iter() {
        let ui = SignalUi::from_signal(&m, id).expect("dialog");
        assert_eq!(apply(&m, &ui.to_op().expect("op")), m, "시그널 {}", s.name);
    }
    for (id, n) in m.nodes.iter() {
        let ui = NodeUi::from_node(&m, id).expect("dialog");
        assert_eq!(apply(&m, &ui.to_op().expect("op")), m, "노드 {}", n.name);
    }
    for (id, t) in m.schedules.iter() {
        let ui = ScheduleUi::from_schedule(&m, id).expect("dialog");
        let after = apply(&m, &ui.to_op(&m).expect("op"));
        assert!(semantically_equal(&after, &m), "테이블 {}", t.name);
    }
    let enc = EncodingUi::from_name(&m, "Enc").expect("dialog");
    assert_eq!(apply(&m, &enc.to_op().expect("op")), m);
}

#[test]
fn diagnostic_frames_do_not_open() {
    let m = load("Diagnostic_frames { MasterReq: 0x3c { } }").model;
    let id = m.find_frame("MasterReq").expect("frame");
    assert!(FrameUi::from_frame(&m, id).is_none());
}

#[test]
fn frame_dialog_moves_places_and_unplaces_signals() {
    let m = model();
    let mut ui = FrameUi::from_frame(&m, m.find_frame("FA").expect("FA")).expect("dialog");
    ui.name = "FA2".into();
    ui.placements.retain(|(n, _)| n != "Err");
    ui.placements[0].1 = 8; // Pos를 비트 8로
    ui.placements.push(("Cmd".into(), 24));
    let after = apply(&m, &ui.to_op(&m).expect("op"));
    let fa = after
        .frames
        .get(after.find_frame("FA2").expect("FA2"))
        .expect("frame");
    let by_name: Vec<(String, u32)> = fa
        .signals()
        .iter()
        .filter_map(|fs| {
            fs.signal.id().map(|s| {
                (
                    after
                        .signals
                        .get(s)
                        .map(|x| x.name.clone())
                        .unwrap_or_default(),
                    fs.offset,
                )
            })
        })
        .collect();
    assert_eq!(by_name, vec![("Pos".to_owned(), 8), ("Cmd".to_owned(), 24)]);
    assert!(after.find_frame("FA").is_none());
}

#[test]
fn new_frame_with_new_signals_in_one_batch() {
    let m = model();
    let mut ui = FrameUi::new_unconditional(&m);
    assert_eq!(ui.name, "Frame_1");
    assert!(
        matches!(&ui.kind, KindUi::Unconditional { id, publisher, size: 8 } if id == "0" && publisher == "M")
    );
    ui.added.push(NewSignalUi {
        name: "Level".into(),
        size: 8,
        offset: 0,
    });
    let after = apply(&m, &ui.to_op(&m).expect("op"));
    let f = after
        .frames
        .get(after.find_frame("Frame_1").expect("frame"))
        .expect("frame");
    assert_eq!(f.signals().len(), 1);
    let s = after
        .signals
        .get(after.find_signal("Level").expect("Level"))
        .expect("signal");
    // 마스터가 발행하는 새 시그널은 첫 슬레이브가 구독한다(구독자 없음 경고를 피한다)
    assert_eq!(s.subscribers.len(), 1);
}

#[test]
fn frame_input_problems_are_reported_while_typing() {
    let m = model();
    let mut ui = FrameUi::new_unconditional(&m);
    assert!(ui.problems().is_empty());
    if let KindUi::Unconditional { id, size, .. } = &mut ui.kind {
        *id = "64".into();
        *size = 9;
    }
    assert_eq!(ui.problems().len(), 2);
    if let KindUi::Unconditional { id, .. } = &mut ui.kind {
        *id = "x".into();
    }
    assert!(ui.to_op(&m).is_err());
}

#[test]
fn signal_dialog_validates_and_edits() {
    let m = model();
    let mut ui = SignalUi::new(&m);
    assert_eq!(ui.name, "Signal_1");
    assert!(ui.problems().is_empty());
    ui.size = 2;
    ui.init = "5".into();
    assert_eq!(ui.problems().len(), 1);
    ui.init = "1".into();
    let after = apply(&m, &ui.to_op().expect("op"));
    assert!(after.find_signal("Signal_1").is_some());

    let mut blob = SignalUi::from_signal(&m, m.find_signal("Blob").expect("Blob")).expect("dialog");
    assert!(blob.array && blob.init == "1, 2");
    blob.init = "1".into();
    assert!(!blob.problems().is_empty());
    blob.init = "1, 300".into();
    assert!(blob.to_op().is_err());
}

#[test]
fn node_dialog_renames_and_edits_attributes() {
    let m = model();
    let mut ui = NodeUi::from_node(&m, m.find_node("A").expect("A")).expect("dialog");
    ui.name = "Door".into();
    if let Some(a) = &mut ui.attrs {
        a.configured_nad = "9".into();
        a.frames.push(("FB".into(), String::new()));
        a.p2_min = "x".into();
    }
    assert!(ui.to_op().is_err(), "숫자가 아닌 P2_min");
    if let Some(a) = &mut ui.attrs {
        a.p2_min = "60".into();
    }
    let after = apply(&m, &ui.to_op().expect("op"));
    let door = after.find_node("Door").expect("Door");
    let attrs = after
        .node_attrs
        .iter()
        .find(|a| a.node.id() == Some(door))
        .expect("attrs");
    assert_eq!(attrs.configured_nad, Some(9));
    assert_eq!(attrs.p2_min_ms, Some(60.0));
    assert_eq!(attrs.configurable_frames.len(), 4);

    let master = NodeUi::from_node(&m, m.find_node("M").expect("M")).expect("dialog");
    assert!(master.master && master.attrs.is_none() && master.timebase == "5");
}

#[test]
fn new_slave_dialog_suggests_a_free_name() {
    let m = model();
    let ui = NodeUi::new_slave(&m);
    assert_eq!(ui.name, "Slave_1");
    let after = apply(&m, &ui.to_op().expect("op"));
    assert_eq!(after.nodes.len(), m.nodes.len() + 1);
}

#[test]
fn schedule_dialog_rebuilds_slots_and_warns() {
    let m = model();
    let main = m.find_schedule("Main").expect("Main");
    let mut ui = ScheduleUi::from_schedule(&m, main).expect("dialog");
    assert_eq!(ui.slots.len(), 5);
    // 위치를 바꾸고 하나를 지우고 새 슬롯을 더한다
    ui.slots.swap(0, 1);
    ui.slots.remove(2);
    ui.slots.push(SlotUi::frame("FB"));
    ui.name = "Primary".into();
    let after = apply(&m, &ui.to_op(&m).expect("op"));
    let t = after
        .schedules
        .get(after.find_schedule("Primary").expect("Primary"))
        .expect("table");
    assert_eq!(t.slots.len(), 5);
    assert!(
        matches!(&t.slots[0].entry, SlotEntry::Frame(r) if r.id().and_then(|i| after.frames.get(i)).is_some_and(|f| f.name == "FA"))
    );
    assert!(
        matches!(&t.slots[4].entry, SlotEntry::Frame(r) if r.id().and_then(|i| after.frames.get(i)).is_some_and(|f| f.name == "FB"))
    );
    // 이 테이블을 충돌 해결 테이블로 쓰는 ETF의 참조는 유지된다(Coll을 고쳐도 ETF가 계속 가리킨다)
    let coll = m.find_schedule("Coll").expect("Coll");
    let mut c = ScheduleUi::from_schedule(&m, coll).expect("dialog");
    c.slots.pop();
    let after = apply(&m, &c.to_op(&m).expect("op"));
    match &after
        .frames
        .get(after.find_frame("ETF").expect("ETF"))
        .expect("frame")
        .kind
    {
        FrameKind::EventTriggered {
            collision_table, ..
        } => assert!(collision_table.is_some()),
        other => panic!("{other:?}"),
    }
    // 경고: 타임베이스(5 ms)의 배수가 아닌 delay와 프레임 시간보다 짧은 delay
    ui.slots[0].delay = "7".into();
    ui.slots[1].delay = "5".into();
    let w = ui.slot_warnings(&m);
    assert!(w[0].as_deref().is_some_and(|s| s.contains("배수")), "{w:?}");
    assert!(
        w[1].is_none() || w[1].as_deref().is_some_and(|s| s.contains("최대")),
        "{w:?}"
    );
}

#[test]
fn schedule_command_arguments_round_trip() {
    let m = model();
    let main = m.find_schedule("Main").expect("Main");
    let ui = ScheduleUi::from_schedule(&m, main).expect("dialog");
    let range = ui
        .slots
        .iter()
        .find(|s| s.kind == SlotKind::AssignFrameIdRange)
        .expect("range");
    assert_eq!(range.args, "0, 193");
    assert_eq!(range.node, "A");
}

#[test]
fn encoding_dialog_edits_entries() {
    let m = model();
    let mut ui = EncodingUi::from_name(&m, "Enc").expect("dialog");
    ui.name = "Level".into();
    ui.entries.push(EntryUi::Logical("2".into(), "high".into()));
    let after = apply(&m, &ui.to_op().expect("op"));
    let e = after
        .encodings
        .get(after.find_encoding("Level").expect("Level"))
        .expect("enc");
    assert_eq!(e.entries.len(), 3);
    ui.entries.push(EntryUi::Logical("x".into(), String::new()));
    assert!(ui.to_op().is_err());
}

#[test]
fn preview_reports_failures_and_new_findings() {
    let m = model();
    let bad = preview(&m, &EditOp::DeleteSignal("Nope".into()));
    assert!(bad.model.is_none() && bad.error.as_deref().is_some_and(|e| e.contains("Nope")));

    // 두 시그널이 겹치게 놓으면 0406 겹침이 새로 생긴다
    let op = EditOp::MoveSignal {
        frame: "FA".into(),
        signal: "Err".into(),
        offset: 4,
    };
    let p = preview(&m, &op);
    assert!(p.error.is_none());
    assert!(
        p.new_findings.iter().any(|d| d.code == 406),
        "{:?}",
        p.new_findings
    );

    // 아무것도 바꾸지 않으면 새로 생기는 결과도 없다
    let same = preview(&m, &EditOp::SetSpeed(19_200));
    assert!(same.new_findings.is_empty());
}

#[test]
fn deletion_effects_list_what_goes_with_it() {
    let m = model();
    let after = apply(&m, &EditOp::DeleteNode("A".into()));
    let effects = deletion_effects(&m, &after);
    let text = effects.join("\n");
    assert!(text.contains("노드 1개 삭제: A"), "{text}");
    assert!(text.contains("프레임") && text.contains("FA"), "{text}");
    assert!(text.contains("시그널") && text.contains("Pos"), "{text}");
    assert!(text.contains("스케줄 슬롯"), "{text}");
    assert!(text.contains("노드 속성 블록 1개 정리"), "{text}");
    // 아무것도 지우지 않으면 비어 있다
    assert!(deletion_effects(&m, &m).is_empty());
}

#[test]
fn name_lists_for_combo_boxes() {
    let m = model();
    assert_eq!(node_names(&m, true), vec!["A", "B"]);
    assert_eq!(node_names(&m, false).len(), 3);
    assert_eq!(frame_names(&m, true), vec!["FCmd", "FA", "FB"]);
    assert!(frame_names(&m, false).contains(&"ETF".to_owned()));
    assert_eq!(master_name(&m), "M");
}

// ---- 속성 창에서 바로 고치기(FR-EDT-01) ----

use crate::view::Selection;

fn sel_frame(m: &LdfModel, name: &str) -> Selection {
    Selection::Frame(m.find_frame(name).expect("frame"))
}

#[test]
fn only_plain_values_are_editable_inline() {
    let m = model();
    let f = sel_frame(&m, "FA");
    for key in ["Name", "Frame ID", "Size [Byte]"] {
        assert!(inline_editable(InlineScope::Object, &f, key), "{key}");
    }
    // 계산한 값·참조는 바로 고치지 않는다
    for key in ["Protected ID", "Type", "Publisher", "Subscribers"] {
        assert!(!inline_editable(InlineScope::Object, &f, key), "{key}");
    }
    let t = Selection::Slot(m.find_schedule("Main").expect("Main"), 0);
    assert!(inline_editable(InlineScope::Schedule, &t, "Delay [ms]"));
    // 슬롯을 고른 상태에서 객체 표의 Name은 프레임 이름인데 스케줄 표와 헷갈리면 안 된다
    assert!(!inline_editable(InlineScope::Object, &t, "Name"));
    assert!(!inline_editable(
        InlineScope::Object,
        &Selection::Cluster,
        "Name"
    ));
}

#[test]
fn out_of_range_frame_id_is_rejected_while_typing() {
    let m = model();
    let sel = sel_frame(&m, "FA");
    // PRD: 프레임 ID에 64를 넣으면 필드가 빨개지고 적용되지 않는다
    let err = inline_edit(&m, InlineScope::Object, &sel, "Frame ID", "64").expect_err("범위 밖");
    assert!(err.contains("0–59"), "{err}");
    assert!(inline_edit(&m, InlineScope::Object, &sel, "Frame ID", "abc").is_err());
    // 맞는 값은 16진도 받는다
    let op = inline_edit(&m, InlineScope::Object, &sel, "Frame ID", "0x0A").expect("맞는 값");
    let after = apply(&m, &op);
    assert_eq!(
        after
            .frames
            .get(after.find_frame("FA").expect("FA"))
            .and_then(|f| f.lin_id()),
        Some(10)
    );
}

#[test]
fn inline_names_must_be_valid_and_unique() {
    let m = model();
    let sel = sel_frame(&m, "FA");
    assert!(
        inline_edit(&m, InlineScope::Object, &sel, "Name", "FCmd").is_err(),
        "이미 있는 이름"
    );
    assert!(inline_edit(&m, InlineScope::Object, &sel, "Name", "9bad").is_err());
    let op = inline_edit(&m, InlineScope::Object, &sel, "Name", "  Renamed  ")
        .expect("공백은 떼고 받는다");
    assert!(apply(&m, &op).find_frame("Renamed").is_some());
    // 자기 이름 그대로는 허용
    assert!(inline_edit(&m, InlineScope::Object, &sel, "Name", "FA").is_ok());
}

#[test]
fn inline_signal_node_and_schedule_values() {
    let m = model();
    let sig = Selection::Signal(m.find_signal("Cmd").expect("Cmd"));
    assert!(inline_edit(&m, InlineScope::Object, &sig, "Length [Bit]", "17").is_err());
    assert!(
        inline_edit(&m, InlineScope::Object, &sig, "Init Value", "300").is_err(),
        "8비트로 표현 못 한다"
    );
    let op = inline_edit(&m, InlineScope::Object, &sig, "Init Value", "7").expect("ok");
    let after = apply(&m, &op);
    assert_eq!(
        after
            .signals
            .get(after.find_signal("Cmd").expect("Cmd"))
            .map(|s| s.init.clone()),
        Some(InitValue::Scalar(7))
    );

    let master = Selection::Node(m.find_node("M").expect("M"));
    assert!(inline_edit(&m, InlineScope::Object, &master, "Timebase [ms]", "x").is_err());
    let op = inline_edit(&m, InlineScope::Object, &master, "Timebase [ms]", "2").expect("ok");
    assert_eq!(
        apply(&m, &op)
            .nodes
            .iter()
            .next()
            .and_then(|(_, n)| n.timebase_ms),
        Some(2.0)
    );
    // 슬레이브에는 타임베이스가 없다
    let slave = Selection::Node(m.find_node("A").expect("A"));
    assert!(inline_edit(&m, InlineScope::Object, &slave, "Timebase [ms]", "2").is_err());

    let main = m.find_schedule("Main").expect("Main");
    assert!(
        inline_edit(
            &m,
            InlineScope::Schedule,
            &Selection::Slot(main, 0),
            "Delay [ms]",
            "-1"
        )
        .is_err()
    );
    let op = inline_edit(
        &m,
        InlineScope::Schedule,
        &Selection::Slot(main, 0),
        "Delay [ms]",
        "25",
    )
    .expect("ok");
    let after = apply(&m, &op);
    let slot = &after
        .schedules
        .get(after.find_schedule("Main").expect("Main"))
        .expect("table")
        .slots[0];
    assert!((slot.delay_ms - 25.0).abs() < 1e-9);
    assert!(
        inline_edit(
            &m,
            InlineScope::Schedule,
            &Selection::Slot(main, 99),
            "Delay [ms]",
            "25"
        )
        .is_err(),
        "없는 슬롯"
    );
    let op = inline_edit(
        &m,
        InlineScope::Schedule,
        &Selection::Schedule(main),
        "Name",
        "Primary",
    )
    .expect("ok");
    assert!(apply(&m, &op).find_schedule("Primary").is_some());
}

// ---- 스케줄 끌어다 놓기(FR-EDT-05) ----

#[test]
fn dropped_frames_get_a_sensible_delay_and_position() {
    let m = model();
    // 19.2 kbps에서 4바이트 프레임의 최대 시간은 약 6.4 ms라 10 ms를 넘지 않고, 타임베이스 5 ms의 배수가 된다
    assert!((default_slot_delay(&m, "FA") - 10.0).abs() < 1e-9);
    // 알 수 없는 프레임도 기본값이 나온다
    assert!(default_slot_delay(&m, "Nope") >= 10.0);
    let op = drop_frame_op(&m, "Main", Some(1), "FB");
    let after = apply(&m, &op);
    let t = after
        .schedules
        .get(after.find_schedule("Main").expect("Main"))
        .expect("table");
    assert_eq!(t.slots.len(), 6);
    assert!(
        matches!(&t.slots[2].entry, SlotEntry::Frame(r) if r.id().and_then(|i| after.frames.get(i)).is_some_and(|f| f.name == "FB"))
    );
    // 테이블 줄에 놓으면 끝에 붙는다
    let end = apply(&m, &drop_frame_op(&m, "Main", None, "FB"));
    let t = end
        .schedules
        .get(end.find_schedule("Main").expect("Main"))
        .expect("table");
    assert!(matches!(
        t.slots.last().map(|s| &s.entry),
        Some(SlotEntry::Frame(_))
    ));
    assert_eq!(t.slots.len(), 6);
}

#[test]
fn moving_a_slot_in_place_does_nothing() {
    assert!(move_slot_op("Main", 2, 2).is_none());
    let m = model();
    let after = apply(&m, &move_slot_op("Main", 0, 2).expect("op"));
    let t = after
        .schedules
        .get(after.find_schedule("Main").expect("Main"))
        .expect("table");
    assert!(matches!(t.slots[2].entry, SlotEntry::Frame(_)));
}

#[test]
fn composition_dialog_starts_empty_and_creates_a_composition() {
    let m = model();
    let mut ui = CompositionUi::from_model(&m);
    assert_eq!(ui.text, "");
    // 비운 채로 확인하면 아무것도 바뀌지 않는다
    assert_eq!(apply(&m, &ui.to_op().expect("op")), m);
    ui.text = "configuration cfg\n  Phys: A, B\n".to_owned();
    let after = apply(&m, &ui.to_op().expect("op"));
    let c = after.composition.as_ref().expect("composition");
    assert_eq!(c.keyword, "composite");
    assert_eq!(c.configurations[0].composites[0].logical, ["A", "B"]);
    // 다시 열면 같은 글이 나오고, 확인해도 바뀌지 않는다
    let again = CompositionUi::from_model(&after);
    assert_eq!(again.text, "configuration cfg\n  Phys: A, B\n");
    assert_eq!(apply(&after, &again.to_op().expect("op")), after);
    // 지우기
    let cleared = CompositionUi {
        text: String::new(),
        ..again
    };
    assert!(
        apply(&after, &cleared.to_op().expect("op"))
            .composition
            .is_none()
    );
}

#[test]
fn composition_text_errors_name_the_line() {
    let parse = |s: &str| parse_composition(s, "composite", false);
    assert_eq!(parse("").expect("empty"), None);
    assert_eq!(parse("  \n\n").expect("blank"), None);
    for (src, line) in [
        ("configuration\n", 1),
        ("P: A\n", 1),
        ("configuration c\n  P:\n", 2),
        ("configuration c\n  P A\n", 2),
        ("\nbogus\n", 2),
    ] {
        let e = parse(src).expect_err(src);
        assert!(e.starts_with(&format!("{line}번째 줄")), "{src:?}: {e}");
    }
    // 모델 규칙(이름·중복)은 연산이 따진다
    let m = model();
    let ui = CompositionUi {
        text: "configuration c\n  P: A, A\n".to_owned(),
        ..CompositionUi::from_model(&m)
    };
    let pv = preview(&m, &ui.to_op().expect("op"));
    assert!(pv.error.is_some());
}

#[test]
fn composition_preview_reports_unknown_logical_nodes() {
    let m = model();
    let ui = CompositionUi {
        text: "configuration c\n  P: A, Ghost\n".to_owned(),
        ..CompositionUi::from_model(&m)
    };
    let pv = preview(&m, &ui.to_op().expect("op"));
    assert!(pv.error.is_none());
    assert!(
        pv.new_findings.iter().any(|d| d.code == 109),
        "{:?}",
        pv.new_findings
    );
}
