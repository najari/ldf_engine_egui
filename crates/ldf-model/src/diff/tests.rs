use super::*;
use crate::{EditOp, FrameDraft, FrameDraftKind, load};

const A: &str = r#"
LIN_description_file;
LIN_protocol_version = "2.1";
LIN_language_version = "2.1";
LIN_speed = 19.2 kbps;
Nodes { Master: M, 5 ms, 0.1 ms; Slaves: S1, S2; }
Signals { Cmd: 8, 0, M, S1; Pos: 16, 0, S1, M; Old: 4, 0, S2, M; }
Frames { FCmd: 1, M, 1 { Cmd, 0; } FPos: 2, S1, 2 { Pos, 0; } FOld: 3, S2, 1 { Old, 0; } }
Node_attributes { S1 { LIN_protocol = "2.1"; configured_NAD = 1; P2_min = 50 ms; configurable_frames { FCmd; FPos; } } }
Schedule_tables { T { FCmd delay 10 ms; FPos delay 10 ms; } }
Signal_encoding_types { E { logical_value, 0, "off"; } }
"#;

fn apply(m: &LdfModel, ops: Vec<EditOp>) -> LdfModel {
    let mut c = m.clone();
    EditOp::Batch(ops)
        .apply(&mut c)
        .unwrap_or_else(|e| panic!("{e}"));
    c
}

fn find<'a>(d: &'a Diff, kind: ObjectKind, name: &str) -> Option<&'a DiffEntry> {
    d.entries.iter().find(|e| e.kind == kind && e.name == name)
}

fn field<'a>(e: &'a DiffEntry, name: &str) -> Option<&'a FieldChange> {
    e.fields.iter().find(|f| f.field == name)
}

#[test]
fn identical_models_have_no_differences() {
    let m = load(A).model;
    assert!(diff(&m, &m).is_empty());
    // 공백·주석만 다른 텍스트도 차이가 없다
    let spaced = load(&format!("// 주석\n{}", A.replace("; ", ";   "))).model;
    assert!(diff(&m, &spaced).is_empty(), "{:?}", diff(&m, &spaced));
}

#[test]
fn changed_fields_report_before_and_after() {
    let a = load(A).model;
    let b = apply(
        &a,
        vec![
            EditOp::SetSpeed(10_400),
            EditOp::MoveSignal {
                frame: "FPos".into(),
                signal: "Pos".into(),
                offset: 8,
            },
            EditOp::SetSlotDelay {
                table: "T".into(),
                index: 1,
                delay_ms: 20.0,
            },
        ],
    );
    let d = diff(&a, &b);
    let h = find(&d, ObjectKind::Header, "header").expect("header");
    let speed = field(h, "speed_bps").expect("speed");
    assert_eq!(
        (speed.before.as_str(), speed.after.as_str()),
        ("19200", "10400")
    );
    let fr = find(&d, ObjectKind::Frame, "FPos").expect("FPos");
    assert_eq!(fr.change, Change::Changed);
    let sigs = field(fr, "signals").expect("signals");
    assert_eq!(
        (sigs.before.as_str(), sigs.after.as_str()),
        ("Pos@0", "Pos@8")
    );
    let t = find(&d, ObjectKind::Schedule, "T").expect("T");
    let slot = field(t, "slot[2]").expect("slot");
    assert_eq!(slot.before, "FPos delay 10 ms");
    assert_eq!(slot.after, "FPos delay 20 ms");
    // 바뀌지 않은 것은 나오지 않는다
    assert!(find(&d, ObjectKind::Frame, "FCmd").is_none());
    assert_eq!(d.counts(), (0, 0, 3, 0));
}

#[test]
fn added_and_removed_objects_list_their_values() {
    let a = load(A).model;
    let b = apply(
        &a,
        vec![
            EditOp::DeleteFrame("FOld".into()),
            EditOp::DeleteSignal("Old".into()),
            EditOp::AddSlave("S3".into()),
            EditOp::AddEncoding {
                name: "E2".into(),
                entries: vec![crate::EncodingEntry::Ascii],
            },
        ],
    );
    let d = diff(&a, &b);
    assert_eq!(
        find(&d, ObjectKind::Frame, "FOld").expect("FOld").change,
        Change::Removed
    );
    let removed = find(&d, ObjectKind::Signal, "Old").expect("Old");
    assert_eq!(removed.change, Change::Removed);
    assert!(
        removed
            .fields
            .iter()
            .any(|f| f.field == "size_bits" && f.before == "4" && f.after.is_empty())
    );
    let added = find(&d, ObjectKind::Node, "S3").expect("S3");
    assert_eq!(added.change, Change::Added);
    assert!(
        added
            .fields
            .iter()
            .any(|f| f.field == "role" && f.after == "slave")
    );
    assert_eq!(
        find(&d, ObjectKind::Encoding, "E2").expect("E2").change,
        Change::Added
    );
    // 삭제는 같은 종류 안에서 맨 뒤에 온다
    let frame_entries: Vec<_> = d
        .entries
        .iter()
        .filter(|e| e.kind == ObjectKind::Frame)
        .collect();
    assert_eq!(
        frame_entries.last().map(|e| &e.change),
        Some(&Change::Removed)
    );
}

#[test]
fn a_frame_with_the_same_id_under_a_new_name_is_a_rename() {
    let a = load(A).model;
    let b = apply(
        &a,
        vec![EditOp::EditFrame {
            name: "FPos".into(),
            draft: FrameDraft {
                name: "PositionFrame".into(),
                kind: FrameDraftKind::Unconditional {
                    id: 2,
                    publisher: "S1".into(),
                    size: 4,
                },
            },
        }],
    );
    let d = diff(&a, &b);
    let e = find(&d, ObjectKind::Frame, "PositionFrame").expect("renamed");
    assert_eq!(
        e.change,
        Change::Renamed {
            from: "FPos".into()
        }
    );
    // 이름과 함께 바뀐 값도 알려 준다(크기 2 -> 4)
    let size = field(e, "size").expect("size");
    assert_eq!((size.before.as_str(), size.after.as_str()), ("2", "4"));
    assert!(
        find(&d, ObjectKind::Frame, "FPos").is_none(),
        "옛 이름이 삭제로 또 나오면 안 된다"
    );
    assert_eq!(d.counts().3, 1);
}

#[test]
fn different_ids_are_not_renames() {
    let a = load(A).model;
    let b = apply(
        &a,
        vec![
            EditOp::DeleteFrame("FOld".into()),
            EditOp::AddFrame(FrameDraft {
                name: "FNew".into(),
                kind: FrameDraftKind::Unconditional {
                    id: 9,
                    publisher: "S2".into(),
                    size: 1,
                },
            }),
        ],
    );
    let d = diff(&a, &b);
    assert_eq!(
        find(&d, ObjectKind::Frame, "FNew").expect("FNew").change,
        Change::Added
    );
    assert_eq!(
        find(&d, ObjectKind::Frame, "FOld").expect("FOld").change,
        Change::Removed
    );
}

#[test]
fn node_attributes_diff_by_node() {
    let a = load(A).model;
    let b = apply(
        &a,
        vec![EditOp::SetNodeAttrs {
            node: "S1".into(),
            attrs: crate::AttrsDraft {
                lin_protocol: Some("2.1".into()),
                configured_nad: Some(9),
                p2_min_ms: Some(100.0),
                configurable_frames: vec![("FCmd".into(), None)],
                ..crate::AttrsDraft::default()
            },
        }],
    );
    let d = diff(&a, &b);
    let e = find(&d, ObjectKind::NodeAttributes, "S1").expect("S1 attrs");
    assert_eq!(
        field(e, "configured_nad").map(|f| f.after.as_str()),
        Some("9")
    );
    assert_eq!(
        field(e, "p2_min_ms").map(|f| (f.before.as_str(), f.after.as_str())),
        Some(("50", "100"))
    );
    assert_eq!(
        field(e, "configurable_frames").map(|f| f.after.as_str()),
        Some("FCmd")
    );
}

#[test]
fn diff_is_antisymmetric_for_added_and_removed() {
    let a = load(A).model;
    let b = apply(
        &a,
        vec![
            EditOp::AddSlave("S3".into()),
            EditOp::DeleteSignal("Old".into()),
        ],
    );
    let (ab, ba) = (diff(&a, &b), diff(&b, &a));
    let (add_ab, rem_ab, _, _) = ab.counts();
    let (add_ba, rem_ba, _, _) = ba.counts();
    assert_eq!((add_ab, rem_ab), (rem_ba, add_ba));
}

#[test]
fn composition_changes_are_reported_per_configuration() {
    let a = load(&format!(
        "{A}composite {{ configuration c1 {{ P {{ S1; }} }} configuration c2 {{ Q {{ S2; }} }} }}"
    ))
    .model;
    let b = load(&format!(
        "{A}composite {{ configuration c1 {{ P {{ S1, S2; }} }} configuration c3 {{ R {{ S1; }} }} }}"
    ))
    .model;
    let d = diff(&a, &b);
    let changed = find(&d, ObjectKind::Composition, "c1").expect("c1");
    assert_eq!(changed.change, Change::Changed);
    let f = field(changed, "P").expect("P");
    assert_eq!((f.before.as_str(), f.after.as_str()), ("S1", "S1, S2"));
    assert_eq!(
        find(&d, ObjectKind::Composition, "c2").map(|e| &e.change),
        Some(&Change::Removed)
    );
    assert_eq!(
        find(&d, ObjectKind::Composition, "c3").map(|e| &e.change),
        Some(&Change::Added)
    );
    assert!(diff(&a, &a).is_empty());
}

#[test]
fn dynamic_frame_changes_are_reported_as_one_object() {
    let a = load(&format!("{A}dynamic_frames {{ 1 }}")).model;
    let b = load(&format!("{A}dynamic_frames {{ 1, 2 }}")).model;
    let d = diff(&a, &b);
    let e = find(&d, ObjectKind::DynamicFrames, "dynamic_frames").expect("dynamic_frames");
    assert_eq!(e.change, Change::Changed);
    let f = field(e, "ids").expect("ids");
    assert_eq!(
        (f.before.as_str(), f.after.as_str()),
        ("0x01", "0x01, 0x02")
    );
    // 섹션이 새로 생기거나 없어지는 경우
    let none = load(A).model;
    assert_eq!(
        find(
            &diff(&none, &a),
            ObjectKind::DynamicFrames,
            "dynamic_frames"
        )
        .map(|e| &e.change),
        Some(&Change::Added)
    );
    assert_eq!(
        find(
            &diff(&a, &none),
            ObjectKind::DynamicFrames,
            "dynamic_frames"
        )
        .map(|e| &e.change),
        Some(&Change::Removed)
    );
    assert!(diff(&a, &a).is_empty());
}

#[test]
fn signal_group_changes_are_reported_per_group() {
    let a = load(&format!(
        "{A}Signal_groups {{ G1: 16 {{ S1, 0; }} G2: 8 {{ S2, 0; }} }}"
    ))
    .model;
    let b = load(&format!(
        "{A}Signal_groups {{ G1: 16 {{ S1, 0; S2, 8; }} G3: 8 {{ S1, 0; }} }}"
    ))
    .model;
    let d = diff(&a, &b);
    let changed = find(&d, ObjectKind::SignalGroup, "G1").expect("G1");
    assert_eq!(changed.change, Change::Changed);
    let f = field(changed, "signals").expect("signals");
    assert_eq!(
        (f.before.as_str(), f.after.as_str()),
        ("S1@0", "S1@0, S2@8")
    );
    assert_eq!(
        find(&d, ObjectKind::SignalGroup, "G2").map(|e| &e.change),
        Some(&Change::Removed)
    );
    assert_eq!(
        find(&d, ObjectKind::SignalGroup, "G3").map(|e| &e.change),
        Some(&Change::Added)
    );
    assert!(diff(&a, &a).is_empty());
}
