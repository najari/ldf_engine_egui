use super::*;
use crate::{CompositeNode, Configuration, load};

/// 모든 종류의 참조를 가진 작은 클러스터(참조가 전부 해석된다).
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
  Warn: 1, 0, B, M;
  Blob: 16, {1, 2}, B, M;
}
Frames {
  FCmd: 1, M, 2 { Cmd, 0; }
  FA: 2, A, 4 { Pos, 0; Err, 16; }
  FB: 3, B, 2 { Warn, 0; Blob, 8; }
}
Sporadic_frames { SP: FCmd; }
Event_triggered_frames { ETF: Coll, 10, FA, FB; }
Node_attributes {
  A {
    LIN_protocol = "2.1"; configured_NAD = 1; product_id = 1, 2, 3; response_error = Err;
    fault_state_signals = Err; P2_min = 50 ms; ST_min = 0 ms;
    configurable_frames { FCmd; FA; ETF; }
  }
  B {
    LIN_protocol = "2.1"; configured_NAD = 2; product_id = 1, 2, 3; response_error = Warn;
    P2_min = 50 ms; ST_min = 0 ms;
    configurable_frames { FCmd; FB; ETF; }
  }
}
Schedule_tables {
  Main { FCmd delay 10 ms; FA delay 10 ms; AssignNAD { A } delay 10 ms; AssignFrameId { B, FB } delay 10 ms; SP delay 10 ms; }
  Coll { FA delay 10 ms; FB delay 10 ms; }
}
Signal_encoding_types {
  Enc { logical_value, 0, "off"; logical_value, 1, "on"; }
  Phys { physical_value, 0, 255, 1, 0, "u"; }
}
Signal_representation { Enc: Err, Warn; Phys: Cmd; }
"#;

fn model() -> LdfModel {
    let l = load(SAMPLE);
    assert!(l.diagnostics.is_empty(), "{:?}", l.diagnostics);
    l.model
}

/// 모든 참조가 해석되어 있고 범위 안인지.
fn assert_consistent(m: &mut LdfModel) {
    let (nodes, signals, frames, encodings, schedules) = (
        m.nodes.len(),
        m.signals.len(),
        m.frames.len(),
        m.encodings.len(),
        m.schedules.len(),
    );
    fn check<T: ArenaId>(r: &Ref<T>, len: usize, what: &str) {
        match r {
            Ref::Resolved(id) => assert!(
                id.index() < len,
                "{what} 참조가 범위를 벗어났다: {}",
                id.index()
            ),
            Ref::Unresolved(n) => panic!("{what} 참조 `{n}`이(가) 해석되지 않았다"),
        }
    }
    visit_node_refs(m, &mut |r| check(r, nodes, "노드"));
    visit_signal_refs(m, &mut |r| check(r, signals, "시그널"));
    visit_frame_refs(m, &mut |r| check(r, frames, "프레임"));
    visit_encoding_refs(m, &mut |r| check(r, encodings, "인코딩"));
    visit_schedule_refs(m, &mut |r| check(r, schedules, "스케줄"));
}

fn applied(op: EditOp) -> LdfModel {
    let mut m = model();
    op.apply(&mut m).unwrap_or_else(|e| panic!("{op:?}: {e}"));
    assert_consistent(&mut m);
    m
}

fn names<I, T>(it: impl Iterator<Item = (I, T)>, name: impl Fn(&T) -> String) -> Vec<String> {
    it.map(|(_, t)| name(&t)).collect()
}

#[test]
fn the_sample_itself_is_consistent() {
    let mut m = model();
    assert_consistent(&mut m);
}

#[test]
fn deleting_every_object_leaves_no_dangling_reference() {
    let base = model();
    let mut ops = Vec::new();
    ops.extend(
        names(base.nodes.iter(), |n| n.name.clone())
            .into_iter()
            .skip(1)
            .map(EditOp::DeleteNode),
    );
    ops.extend(
        names(base.signals.iter(), |s| s.name.clone())
            .into_iter()
            .map(EditOp::DeleteSignal),
    );
    ops.extend(
        names(base.frames.iter(), |f| f.name.clone())
            .into_iter()
            .map(EditOp::DeleteFrame),
    );
    ops.extend(
        names(base.encodings.iter(), |e| e.name.clone())
            .into_iter()
            .map(EditOp::DeleteEncoding),
    );
    ops.extend(
        names(base.schedules.iter(), |s| s.name.clone())
            .into_iter()
            .map(EditOp::DeleteSchedule),
    );
    assert!(ops.len() >= 15);
    for op in ops {
        let mut m = base.clone();
        op.apply(&mut m).unwrap_or_else(|e| panic!("{op:?}: {e}"));
        assert_consistent(&mut m);
    }
}

#[test]
fn deleting_a_signal_removes_its_placements_and_references() {
    let m = applied(EditOp::DeleteSignal("Err".into()));
    assert!(m.find_signal("Err").is_none());
    let a = m
        .find_node("A")
        .and_then(|id| m.node_attrs.iter().find(|x| x.node.id() == Some(id)));
    let a = a.expect("A의 속성");
    assert!(a.response_error.is_none());
    assert!(a.fault_state_signals.is_empty());
    let fa = m
        .frames
        .get(m.find_frame("FA").expect("FA"))
        .expect("frame");
    assert_eq!(fa.signals().len(), 1);
    // 뒤에 있던 시그널의 ID가 당겨졌어도 이름으로 같은 객체를 가리킨다
    let warn = m.find_signal("Warn").expect("Warn");
    let fb = m
        .frames
        .get(m.find_frame("FB").expect("FB"))
        .expect("frame");
    assert!(fb.signals().iter().any(|fs| fs.signal.id() == Some(warn)));
}

#[test]
fn deleting_a_frame_removes_slots_associations_and_configurable_entries() {
    let m = applied(EditOp::DeleteFrame("FA".into()));
    assert!(m.find_frame("FA").is_none());
    for (_, t) in m.schedules.iter() {
        assert!(t.slots.iter().all(|s| {
            match &s.entry {
                SlotEntry::Frame(r) => r
                    .id()
                    .and_then(|i| m.frames.get(i))
                    .is_some_and(|f| f.name != "FA"),
                _ => true,
            }
        }));
    }
    let etf = m
        .frames
        .get(m.find_frame("ETF").expect("ETF"))
        .expect("frame");
    match &etf.kind {
        FrameKind::EventTriggered { associated, .. } => assert_eq!(associated.len(), 1),
        other => panic!("{other:?}"),
    }
    assert!(
        m.node_attrs
            .iter()
            .all(|a| a.configurable_frames.iter().all(|c| {
                c.frame
                    .id()
                    .and_then(|i| m.frames.get(i))
                    .is_some_and(|f| f.name != "FA")
            }))
    );
}

#[test]
fn deleting_a_node_cascades_to_what_it_publishes() {
    let m = applied(EditOp::DeleteNode("A".into()));
    assert!(m.find_node("A").is_none());
    assert!(m.find_frame("FA").is_none());
    assert!(m.find_signal("Pos").is_none());
    assert!(m.find_signal("Err").is_none());
    assert_eq!(m.node_attrs.len(), 1);
    // B는 남고, Cmd의 구독자에서 A만 빠진다
    let cmd = m
        .signals
        .get(m.find_signal("Cmd").expect("Cmd"))
        .expect("signal");
    assert_eq!(cmd.subscribers.len(), 1);
    // AssignNAD { A } 슬롯도 사라졌다
    let main = m
        .schedules
        .get(m.find_schedule("Main").expect("Main"))
        .expect("table");
    assert!(
        main.slots
            .iter()
            .all(|s| !matches!(s.entry, SlotEntry::AssignNad { .. }))
    );
}

#[test]
fn renaming_keeps_every_reference() {
    let m = applied(EditOp::RenameNode {
        from: "A".into(),
        to: "Door".into(),
    });
    assert!(m.find_node("Door").is_some() && m.find_node("A").is_none());
    let m = applied(EditOp::EditSignal {
        name: "Pos".into(),
        draft: SignalDraft {
            name: "Position".into(),
            size_bits: 16,
            init: InitValue::Scalar(0),
            publisher: Some("A".into()),
            subscribers: vec!["M".into()],
            encoding: None,
        },
    });
    let fa = m
        .frames
        .get(m.find_frame("FA").expect("FA"))
        .expect("frame");
    let id = m.find_signal("Position").expect("Position");
    assert!(fa.signals().iter().any(|fs| fs.signal.id() == Some(id)));
}

#[test]
fn creating_and_placing_in_one_batch() {
    let m = applied(EditOp::Batch(vec![
        EditOp::AddSlave("C".into()),
        EditOp::AddSignal(SignalDraft {
            name: "Level".into(),
            size_bits: 8,
            init: InitValue::Scalar(3),
            publisher: Some("C".into()),
            subscribers: vec!["M".into()],
            encoding: Some("Phys".into()),
        }),
        EditOp::AddFrame(FrameDraft {
            name: "FC".into(),
            kind: FrameDraftKind::Unconditional {
                id: 4,
                publisher: "C".into(),
                size: 1,
            },
        }),
        EditOp::PlaceSignal {
            frame: "FC".into(),
            signal: "Level".into(),
            offset: 0,
        },
        EditOp::MoveSignal {
            frame: "FC".into(),
            signal: "Level".into(),
            offset: 0,
        },
    ]));
    let fc = m
        .frames
        .get(m.find_frame("FC").expect("FC"))
        .expect("frame");
    assert_eq!(fc.signals().len(), 1);
    let level = m
        .signals
        .get(m.find_signal("Level").expect("Level"))
        .expect("signal");
    assert_eq!(level.encodings.len(), 1);
}

#[test]
fn a_failing_batch_changes_nothing() {
    let before = model();
    let mut m = before.clone();
    let err = EditOp::Batch(vec![
        EditOp::AddSlave("C".into()),
        EditOp::DeleteSignal("Nope".into()),
    ])
    .apply(&mut m)
    .expect_err("실패해야 한다");
    assert_eq!(
        err,
        EditError::NotFound {
            kind: "시그널",
            name: "Nope".into()
        }
    );
    assert_eq!(m, before);
}

#[test]
fn names_must_be_valid_and_unique() {
    let mut m = model();
    assert!(matches!(
        EditOp::AddSlave("A".into()).apply(&mut m),
        Err(EditError::Duplicate { .. })
    ));
    assert!(matches!(
        EditOp::AddSlave("1x".into()).apply(&mut m),
        Err(EditError::Invalid(_))
    ));
    assert!(matches!(
        EditOp::AddSlave("a b".into()).apply(&mut m),
        Err(EditError::Invalid(_))
    ));
    // 자기 이름으로 바꾸는 것은 허용
    assert!(
        EditOp::RenameNode {
            from: "A".into(),
            to: "A".into()
        }
        .apply(&mut m)
        .is_ok()
    );
}

#[test]
fn schedule_slot_operations() {
    let m = applied(EditOp::Batch(vec![
        EditOp::AddSchedule("Extra".into()),
        EditOp::AddSlot {
            table: "Extra".into(),
            index: None,
            entry: SlotDraft::Frame("FB".into()),
            delay_ms: 10.0,
        },
        EditOp::AddSlot {
            table: "Extra".into(),
            index: Some(0),
            entry: SlotDraft::Frame("FA".into()),
            delay_ms: 11.0,
        },
        EditOp::AddSlot {
            table: "Extra".into(),
            index: None,
            entry: SlotDraft::SaveConfiguration("A".into()),
            delay_ms: 12.0,
        },
        EditOp::MoveSlot {
            table: "Extra".into(),
            from: 2,
            to: 0,
        },
        EditOp::SetSlotDelay {
            table: "Extra".into(),
            index: 1,
            delay_ms: 20.0,
        },
        EditOp::RemoveSlot {
            table: "Extra".into(),
            index: 2,
        },
    ]));
    let t = m
        .schedules
        .get(m.find_schedule("Extra").expect("Extra"))
        .expect("table");
    assert_eq!(t.slots.len(), 2);
    assert!(matches!(
        t.slots[0].entry,
        SlotEntry::SaveConfiguration { .. }
    ));
    assert!((t.slots[1].delay_ms - 20.0).abs() < 1e-9);
    let mut bad = model();
    assert!(
        EditOp::RemoveSlot {
            table: "Main".into(),
            index: 99
        }
        .apply(&mut bad)
        .is_err()
    );
}

#[test]
fn deleting_a_collision_table_unsets_it() {
    let m = applied(EditOp::DeleteSchedule("Coll".into()));
    match &m
        .frames
        .get(m.find_frame("ETF").expect("ETF"))
        .expect("frame")
        .kind
    {
        FrameKind::EventTriggered {
            collision_table, ..
        } => assert!(collision_table.is_none()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn encodings_can_be_edited_and_deleted() {
    let m = applied(EditOp::EditEncoding {
        name: "Enc".into(),
        new_name: "OnOff".into(),
        entries: vec![EncodingEntry::Logical {
            value: 0,
            text: "no".into(),
        }],
    });
    assert!(m.find_encoding("OnOff").is_some());
    let m = applied(EditOp::DeleteEncoding("Enc".into()));
    let err = m
        .signals
        .get(m.find_signal("Err").expect("Err"))
        .expect("signal");
    assert!(err.encodings.is_empty());
    // 뒤에 있던 Phys의 연결이 당겨진 ID로 이어진다
    let cmd = m
        .signals
        .get(m.find_signal("Cmd").expect("Cmd"))
        .expect("signal");
    assert_eq!(cmd.encodings.len(), 1);
    assert_eq!(
        cmd.encodings[0]
            .id()
            .and_then(|i| m.encodings.get(i))
            .map(|e| e.name.as_str()),
        Some("Phys")
    );
}

#[test]
fn node_attributes_are_replaced_but_unparsed_ones_survive() {
    let mut m = model();
    let a = m.find_node("A").expect("A");
    m.node_attrs
        .iter_mut()
        .find(|x| x.node.id() == Some(a))
        .expect("attrs")
        .extra
        .push(crate::RawAttr {
            key: "wakeup_time".into(),
            text: "wakeup_time = 50 ms;".into(),
        });
    EditOp::SetNodeAttrs {
        node: "A".into(),
        attrs: AttrsDraft {
            lin_protocol: Some("2.1".into()),
            configured_nad: Some(9),
            response_error: Some("Err".into()),
            configurable_frames: vec![("FA".into(), None)],
            ..AttrsDraft::default()
        },
    }
    .apply(&mut m)
    .expect("apply");
    assert_consistent(&mut m);
    let attrs = m
        .node_attrs
        .iter()
        .find(|x| x.node.id() == Some(a))
        .expect("attrs");
    assert_eq!(attrs.configured_nad, Some(9));
    assert_eq!(attrs.configurable_frames.len(), 1);
    assert_eq!(attrs.extra.len(), 1);
}

#[test]
fn suggested_names_and_ids_are_free() {
    let m = model();
    assert_eq!(m.unique_name("Frame"), "Frame_1");
    assert_eq!(m.unique_name("Table"), "Table_1");
    assert_eq!(m.free_frame_id(), Some(0));
    // 사용 중인 이름을 피한다
    let mut m = m;
    EditOp::AddSlave("Frame_1".into())
        .apply(&mut m)
        .expect("apply");
    assert_eq!(m.unique_name("Frame"), "Frame_2");
}

#[test]
fn frames_only_accept_their_own_operations() {
    let mut m = model();
    let err = EditOp::PlaceSignal {
        frame: "SP".into(),
        signal: "Cmd".into(),
        offset: 0,
    }
    .apply(&mut m)
    .expect_err("스포라딕에는 놓을 수 없다");
    assert!(matches!(err, EditError::Invalid(_)));
    let dup = EditOp::PlaceSignal {
        frame: "FCmd".into(),
        signal: "Cmd".into(),
        offset: 8,
    }
    .apply(&mut m);
    assert!(dup.is_err());
    let kind = EditOp::EditFrame {
        name: "FCmd".into(),
        draft: FrameDraft {
            name: "FCmd".into(),
            kind: FrameDraftKind::Sporadic { associated: vec![] },
        },
    }
    .apply(&mut m);
    assert!(matches!(kind, Err(EditError::Invalid(_))));
}

#[allow(clippy::type_complexity)]
fn comp(keyword: &str, cfgs: &[(&str, &[(&str, &[&str])])]) -> Composition {
    Composition {
        keyword: keyword.to_owned(),
        configurations: cfgs
            .iter()
            .map(|(name, nodes)| Configuration {
                name: (*name).to_owned(),
                composites: nodes
                    .iter()
                    .map(|(n, l)| CompositeNode {
                        name: (*n).to_owned(),
                        logical: l.iter().map(|s| (*s).to_owned()).collect(),
                    })
                    .collect(),
            })
            .collect(),
        colon_form: false,
        span: None,
    }
}

#[test]
fn composition_can_be_set_replaced_and_removed() {
    let mut m = model();
    assert!(m.composition.is_none());
    let c = comp("composite", &[("cfg", &[("Phys", &["A", "B"])])]);
    EditOp::SetComposition(Some(c.clone()))
        .apply(&mut m)
        .expect("set");
    assert_eq!(m.composition, Some(c));
    // 새 섹션은 Nodes 바로 뒤에 놓인다
    let nodes = m
        .section_order
        .iter()
        .position(|n| n == "Nodes")
        .expect("Nodes");
    assert_eq!(m.section_order[nodes + 1], "composite");
    // 이름을 바꾸면 자리를 지킨다(중복 없이)
    let c2 = comp("Node_composition", &[("cfg", &[("Phys", &["A"])])]);
    EditOp::SetComposition(Some(c2))
        .apply(&mut m)
        .expect("replace");
    assert_eq!(
        m.section_order
            .iter()
            .filter(|n| n.contains("omposit"))
            .count(),
        1
    );
    assert_eq!(m.section_order[nodes + 1], "Node_composition");
    EditOp::SetComposition(None).apply(&mut m).expect("remove");
    assert!(m.composition.is_none());
    assert!(!m.section_order.iter().any(|n| n.contains("omposit")));
}

#[test]
fn invalid_compositions_are_refused_and_leave_the_model_alone() {
    let before = model();
    let bad = [
        comp("composite", &[]),
        comp("composite", &[("cfg", &[])]),
        comp("composite", &[("cfg", &[("P", &[])])]),
        comp("composite", &[("1bad", &[("P", &["A"])])]),
        comp("composite", &[("cfg", &[("P", &["A", "A"])])]),
        comp("composite", &[("cfg", &[("P", &["A"]), ("P", &["B"])])]),
        comp(
            "composite",
            &[("cfg", &[("P", &["A"])]), ("cfg", &[("Q", &["B"])])],
        ),
        comp("composite", &[("cfg", &[("P", &["A"]), ("Q", &["A"])])]),
        comp("other", &[("cfg", &[("P", &["A"])])]),
    ];
    for c in bad {
        let mut m = before.clone();
        assert!(
            EditOp::SetComposition(Some(c.clone()))
                .apply(&mut m)
                .is_err(),
            "{c:?}"
        );
        assert_eq!(m, before);
    }
}

#[test]
fn renaming_and_deleting_nodes_follow_into_the_composition() {
    let mut m = model();
    EditOp::SetComposition(Some(comp(
        "composite",
        &[
            ("c1", &[("Phys", &["A", "B"])]),
            ("c2", &[("Only", &["B"])]),
        ],
    )))
    .apply(&mut m)
    .expect("set");
    EditOp::RenameNode {
        from: "A".into(),
        to: "A2".into(),
    }
    .apply(&mut m)
    .expect("rename");
    let c = m.composition.as_ref().expect("composition");
    assert_eq!(c.configurations[0].composites[0].logical, ["A2", "B"]);
    // B를 지우면 c2는 비어 사라지고 c1에는 A2만 남는다
    EditOp::DeleteNode("B".into())
        .apply(&mut m)
        .expect("delete");
    let c = m.composition.as_ref().expect("composition");
    assert_eq!(c.configurations.len(), 1);
    assert_eq!(c.configurations[0].composites[0].logical, ["A2"]);
    // 마지막 논리 노드까지 지우면 노드 구성이 통째로 사라진다
    EditOp::DeleteNode("A2".into())
        .apply(&mut m)
        .expect("delete");
    assert!(m.composition.is_none());
    assert!(!m.section_order.iter().any(|n| n == "composite"));
}
