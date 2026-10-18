//! 편집 → Undo → Redo가 모든 연산에서 대칭인지, 편집 결과가 항상 읽을 수 있는 LDF인지 확인한다(PRD G3).

#![allow(clippy::expect_used, clippy::panic)]

use ldf_engine::{
    AttrsDraft, EditOp, FrameDraft, FrameDraftKind, ModelSnapshot, Session, SignalDraft, SlotDraft,
};
use ldf_model::{CompositeNode, Composition, Configuration, InitValue, semantically_equal};
use std::sync::Arc;

const SAMPLE: &str = "// 샘플 클러스터\r\n\
LIN_description_file;\r\n\
LIN_protocol_version = \"2.1\";\r\n\
LIN_language_version = \"2.1\";\r\n\
LIN_speed = 19.2 kbps;\r\n\
Nodes { Master: M, 5 ms, 0.1 ms; Slaves: A, B; }\r\n\
Signals {\r\n\
  Cmd: 8, 0, M, A, B;\r\n\
  Pos: 16, 0, A, M;\r\n\
  Err: 1, 0, A, M;\r\n\
  Warn: 1, 0, B, M;\r\n\
  Blob: 16, {1, 2}, B, M;\r\n\
}\r\n\
Frames {\r\n\
  FCmd: 1, M, 2 { Cmd, 0; }\r\n\
  FA: 2, A, 4 { Pos, 0; Err, 16; }\r\n\
  FB: 3, B, 2 { Warn, 0; Blob, 8; }\r\n\
}\r\n\
Sporadic_frames { SP: FCmd; }\r\n\
Event_triggered_frames { ETF: Coll, 10, FA, FB; }\r\n\
Node_attributes {\r\n\
  A {\r\n\
    LIN_protocol = \"2.1\"; configured_NAD = 1; product_id = 1, 2, 3; response_error = Err;\r\n\
    P2_min = 50 ms; ST_min = 0 ms; configurable_frames { FCmd; FA; ETF; }\r\n\
  }\r\n\
  B {\r\n\
    LIN_protocol = \"2.1\"; configured_NAD = 2; product_id = 1, 2, 3; response_error = Warn;\r\n\
    P2_min = 50 ms; ST_min = 0 ms; configurable_frames { FCmd; FB; ETF; }\r\n\
  }\r\n\
}\r\n\
Schedule_tables {\r\n\
  Main { FCmd delay 10 ms; FA delay 10 ms; AssignNAD { A } delay 10 ms; SP delay 10 ms; }\r\n\
  Coll { FA delay 10 ms; FB delay 10 ms; }\r\n\
}\r\n\
Signal_encoding_types { Enc { logical_value, 0, \"off\"; logical_value, 1, \"on\"; } }\r\n\
Signal_representation { Enc: Err, Warn; }\r\n";

fn open() -> (Session, Arc<ModelSnapshot>) {
    let mut s = Session::open_in_memory().expect("session");
    let snap = s
        .import_bytes("sample.ldf", SAMPLE.as_bytes())
        .expect("import")
        .snapshot;
    (s, snap)
}

fn every_op() -> Vec<EditOp> {
    let draft = |name: &str| SignalDraft {
        name: name.into(),
        size_bits: 8,
        init: InitValue::Scalar(0),
        publisher: Some("A".into()),
        subscribers: vec!["M".into()],
        encoding: None,
    };
    vec![
        EditOp::SetSpeed(10_400),
        EditOp::SetChannelName(Some("Body".into())),
        EditOp::SetMasterTiming {
            node: "M".into(),
            timebase_ms: 2.0,
            jitter_ms: 0.0,
        },
        EditOp::AddSlave("C".into()),
        EditOp::RenameNode {
            from: "A".into(),
            to: "Door".into(),
        },
        EditOp::DeleteNode("B".into()),
        EditOp::AddSignal(draft("New")),
        EditOp::EditSignal {
            name: "Pos".into(),
            draft: SignalDraft {
                name: "Position".into(),
                ..draft("x")
            },
        },
        EditOp::DeleteSignal("Err".into()),
        EditOp::AddFrame(FrameDraft {
            name: "FNew".into(),
            kind: FrameDraftKind::Unconditional {
                id: 4,
                publisher: "A".into(),
                size: 2,
            },
        }),
        EditOp::AddFrame(FrameDraft {
            name: "SP2".into(),
            kind: FrameDraftKind::Sporadic {
                associated: vec!["FCmd".into()],
            },
        }),
        EditOp::EditFrame {
            name: "FA".into(),
            draft: FrameDraft {
                name: "FA2".into(),
                kind: FrameDraftKind::Unconditional {
                    id: 7,
                    publisher: "A".into(),
                    size: 4,
                },
            },
        },
        EditOp::DeleteFrame("FB".into()),
        EditOp::PlaceSignal {
            frame: "FCmd".into(),
            signal: "Warn".into(),
            offset: 8,
        },
        EditOp::UnplaceSignal {
            frame: "FA".into(),
            signal: "Err".into(),
        },
        EditOp::MoveSignal {
            frame: "FA".into(),
            signal: "Err".into(),
            offset: 24,
        },
        EditOp::AddEncoding {
            name: "E2".into(),
            entries: vec![],
        },
        EditOp::EditEncoding {
            name: "Enc".into(),
            new_name: "OnOff".into(),
            entries: vec![],
        },
        EditOp::DeleteEncoding("Enc".into()),
        EditOp::AddSchedule("T2".into()),
        EditOp::RenameSchedule {
            from: "Main".into(),
            to: "Primary".into(),
        },
        EditOp::DeleteSchedule("Coll".into()),
        EditOp::AddSlot {
            table: "Main".into(),
            index: Some(0),
            entry: SlotDraft::Frame("FB".into()),
            delay_ms: 12.0,
        },
        EditOp::RemoveSlot {
            table: "Main".into(),
            index: 0,
        },
        EditOp::MoveSlot {
            table: "Main".into(),
            from: 0,
            to: 1,
        },
        EditOp::SetSlotDelay {
            table: "Main".into(),
            index: 0,
            delay_ms: 25.0,
        },
        EditOp::SetNodeAttrs {
            node: "A".into(),
            attrs: AttrsDraft {
                lin_protocol: Some("2.1".into()),
                configured_nad: Some(7),
                ..AttrsDraft::default()
            },
        },
        EditOp::SetComposition(Some(composition())),
    ]
}

fn composition() -> Composition {
    Composition {
        keyword: "composite".into(),
        configurations: vec![Configuration {
            name: "Cfg".into(),
            composites: vec![CompositeNode {
                name: "Phys".into(),
                logical: vec!["A".into()],
            }],
        }],
        colon_form: false,
        span: None,
    }
}

#[test]
fn a_composition_edit_shows_up_in_the_text_and_comes_back_on_undo() {
    let (mut s, base) = open();
    let after = s
        .edit(&base, &EditOp::SetComposition(Some(composition())))
        .expect("edit");
    assert!(after.source.contains("composite"), "{}", after.source);
    assert!(
        after.source.contains("configuration Cfg"),
        "{}",
        after.source
    );
    // 다시 읽은 모델은 원문 위치(span)만 더 갖는다
    let configs = |s: &ModelSnapshot| {
        s.model
            .composition
            .as_ref()
            .map(|c| c.configurations.clone())
    };
    assert_eq!(configs(&after), Some(composition().configurations));
    // 노드 이름을 바꾸면 노드 구성의 논리 노드도 따라온다
    let renamed = s
        .edit(
            &after,
            &EditOp::RenameNode {
                from: "A".into(),
                to: "A2".into(),
            },
        )
        .expect("rename");
    assert!(renamed.source.contains("A2;"), "{}", renamed.source);
    let back = s.undo(&renamed).expect("undo");
    assert_eq!(configs(&back), Some(composition().configurations));
}

#[test]
fn every_edit_undoes_and_redoes_exactly() {
    for op in every_op() {
        let (mut s, base) = open();
        let edited = s.edit(&base, &op).unwrap_or_else(|e| panic!("{op:?}: {e}"));
        assert!(
            edited.dirty && edited.can_undo && !edited.can_redo,
            "{op:?}"
        );
        assert_ne!(edited.source, base.source, "{op:?}: 텍스트가 바뀌어야 한다");

        let back = s.undo(&edited).expect("undo");
        assert_eq!(back.source, base.source, "{op:?}");
        assert_eq!(back.model, base.model, "{op:?}: 모델이 같아야 한다");
        assert_eq!(back.diagnostics, base.diagnostics, "{op:?}");
        assert!(!back.dirty && !back.can_undo && back.can_redo, "{op:?}");

        let again = s.redo(&back).expect("redo");
        assert_eq!(again.source, edited.source, "{op:?}");
        assert_eq!(again.model, edited.model, "{op:?}");
        assert!(again.dirty && again.can_undo && !again.can_redo, "{op:?}");
    }
}

#[test]
fn edited_text_is_always_valid_ldf() {
    for op in every_op() {
        let (mut s, base) = open();
        let edited = s.edit(&base, &op).unwrap_or_else(|e| panic!("{op:?}: {e}"));
        // 파서·lowering 오류와 미정의 참조(0020)가 없어야 한다
        let bad: Vec<_> = edited
            .diagnostics
            .iter()
            .filter(|d| d.code >= 9000 || d.code == 20)
            .collect();
        assert!(bad.is_empty(), "{op:?}: {bad:?}");
        // 쓴 텍스트를 다시 읽어도 같은 모델
        let again = ldf_model::load(&edited.source);
        assert!(semantically_equal(&edited.model, &again.model), "{op:?}");
        // 원본의 줄바꿈(CRLF)을 지킨다
        assert!(
            edited.source.contains("\r\n") && !edited.source.replace("\r\n", "").contains('\n'),
            "{op:?}"
        );
    }
}

#[test]
fn a_chain_of_edits_unwinds_in_reverse_order() {
    let (mut s, base) = open();
    let ops = every_op();
    let mut snap = Arc::clone(&base);
    let mut applied = Vec::new();
    for op in ops {
        // 앞선 연산 때문에 적용할 수 없게 된 것은 건너뛴다(예: 지운 노드)
        if let Ok(next) = s.edit(&snap, &op) {
            applied.push(Arc::clone(&snap));
            snap = next;
        }
    }
    assert!(applied.len() >= 10, "{}", applied.len());
    for before in applied.iter().rev() {
        snap = s.undo(&snap).expect("undo");
        assert_eq!(snap.source, before.source);
        assert_eq!(snap.model, before.model);
    }
    assert!(!snap.can_undo && !snap.dirty);
    assert!(s.undo(&snap).is_none());
}

#[test]
fn undo_keeps_at_least_one_hundred_steps() {
    let (mut s, base) = open();
    let mut snap = Arc::clone(&base);
    for i in 0..150u32 {
        snap = s.edit(&snap, &EditOp::SetSpeed(1_000 + i)).expect("edit");
    }
    let mut steps = 0;
    while let Some(prev) = s.undo(&snap) {
        snap = prev;
        steps += 1;
    }
    assert_eq!(steps, 150);
    assert_eq!(snap.source, base.source);
}

#[test]
fn a_new_edit_discards_the_redo_branch() {
    let (mut s, base) = open();
    let one = s.edit(&base, &EditOp::SetSpeed(10_400)).expect("edit");
    let back = s.undo(&one).expect("undo");
    assert!(back.can_redo);
    let other = s.edit(&back, &EditOp::SetSpeed(9_600)).expect("edit");
    assert!(!other.can_redo);
    assert!(s.redo(&other).is_none());
}

#[test]
fn a_failed_edit_leaves_everything_untouched() {
    let (mut s, base) = open();
    let err = s
        .edit(&base, &EditOp::DeleteSignal("Nope".into()))
        .expect_err("실패");
    assert!(err.to_string().contains("Nope"));
    // 실패는 Undo 기록을 만들지 않는다
    assert!(s.undo(&base).is_none());
}

#[test]
fn saving_creates_a_child_revision_and_keeps_the_history() {
    let (mut s, base) = open();
    let edited = s.edit(&base, &EditOp::AddSlave("C".into())).expect("edit");
    let saved = s.save_edits(&edited, None).expect("save").snapshot;
    assert!(!saved.dirty, "저장 직후에는 변경 없음");
    assert_ne!(saved.revision.id, base.revision.id);
    assert_eq!(saved.revision.parent_id, Some(base.revision.id));
    assert!(saved.model.find_node("C").is_some());
    assert_eq!(s.revisions().expect("list").len(), 2);

    // 저장한 뒤에도 Undo할 수 있고, 되돌리면 저장본과 달라져 dirty가 된다
    let undone = s.undo(&saved).expect("undo");
    assert!(undone.dirty);
    assert!(undone.model.find_node("C").is_none());
    let redone = s.redo(&undone).expect("redo");
    assert!(!redone.dirty);
}

#[test]
fn exporting_the_open_snapshot_includes_unsaved_edits() {
    let (mut s, base) = open();
    let edited = s.edit(&base, &EditOp::AddSlave("C".into())).expect("edit");
    let dir = std::env::temp_dir().join(format!("ldf-edit-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("out.ldf");
    let opts = ldf_engine::ExportOptions {
        allow_errors: true,
        ..ldf_engine::ExportOptions::default()
    };
    s.export_snapshot(&edited, &path, &opts).expect("export");
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(text.contains("Slaves: A, B, C;"), "{text}");
    // 저장된 리비전을 읽어 내보내면 편집 전 내용이고, 그 과정이 편집 기록을 지우지 않는다
    let old = dir.join("old.ldf");
    s.export(base.revision.id, &old, &opts).expect("export");
    assert!(
        !std::fs::read_to_string(&old)
            .expect("read")
            .contains("Slaves: A, B, C;")
    );
    assert!(s.undo(&edited).is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

/// 시그널 500개짜리 LDF를 만든다(프레임 125개, 슬레이브 20개).
fn big_ldf() -> String {
    let mut s = String::from(
        "LIN_description_file;\nLIN_protocol_version = \"2.1\";\nLIN_language_version = \"2.1\";\nLIN_speed = 19.2 kbps;\n",
    );
    let slaves: Vec<String> = (0..20).map(|i| format!("S{i}")).collect();
    s.push_str(&format!(
        "Nodes {{ Master: M, 1 ms, 0 ms; Slaves: {}; }}\n",
        slaves.join(", ")
    ));
    s.push_str("Signals {\n");
    for i in 0..500 {
        s.push_str(&format!("  Sig{i}: 8, 0, M, {};\n", slaves[i % 20]));
    }
    s.push_str("}\nFrames {\n");
    for f in 0..125 {
        s.push_str(&format!("  Fr{f}: {}, M, 4 {{ ", f % 60));
        for k in 0..4 {
            s.push_str(&format!("Sig{}, {}; ", f * 4 + k, k * 8));
        }
        s.push_str("}\n");
    }
    s.push_str("}\nSchedule_tables { T { ");
    for f in 0..125 {
        s.push_str(&format!("Fr{f} delay 10 ms; "));
    }
    s.push_str("} }\n");
    s
}

/// FR-CHK-09: 시그널 500개 LDF에서 편집 뒤 결과가 200 ms 안에 갱신된다. 편집마다 전체를 다시 검사하므로 결과는 항상 전체 검사와 같다.
/// 속도는 최적화 빌드에서만 의미가 있으므로 `cargo test --release -p ldf-engine --test edit -- --ignored`로 잰다.
#[test]
#[ignore = "릴리스 빌드에서 속도를 잰다"]
fn editing_a_500_signal_file_updates_results_within_200_ms() {
    let mut s = Session::open_in_memory().expect("session");
    let base = s
        .import_bytes("big.ldf", big_ldf().as_bytes())
        .expect("import")
        .snapshot;
    assert_eq!(base.model.signals.len(), 500);
    let mut worst = std::time::Duration::ZERO;
    let mut snap = base;
    for i in 0..10 {
        let started = std::time::Instant::now();
        snap = s.edit(&snap, &EditOp::SetSpeed(10_000 + i)).expect("edit");
        worst = worst.max(started.elapsed());
    }
    eprintln!("최악 편집 시간 {worst:?}");
    assert!(worst < std::time::Duration::from_millis(200), "{worst:?}");
}
