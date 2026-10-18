//! 속성 기반 시험(PRD WP11/G3).
//!
//! 1. 임의의 편집 열(列)을 적용해도 모델은 쓰고 다시 읽으면 같고(왕복), 출력은 고정점이다.
//! 2. 임의의 편집 열은 Undo로 정확히 되돌아간다.
//! 3. 임의로 망가뜨린 텍스트를 분석·출력해도 패닉하지 않는다.
//!
//! 기본 횟수는 빠르게 끝나도록 작다. G3 기준(10,000회)은 릴리스 빌드로 돌린다:
//! `PROPTEST_CASES=10000 cargo test --release -p ldf-engine --test properties`

#![allow(clippy::expect_used, clippy::panic)]

use ldf_engine::{
    AttrsDraft, EditOp, FrameDraft, FrameDraftKind, Session, SignalDraft, SlotDraft, analyze,
};
use ldf_model::{
    EncodingEntry, FrameKind, InitValue, LdfModel, NodeRole, SignalKind, load, semantically_equal,
};
use ldf_writer::{WriteOptions, write};
use proptest::prelude::*;

const BASE: &str = "// 기준 클러스터\r\n\
LIN_description_file;\r\n\
LIN_protocol_version = \"2.1\";\r\n\
LIN_language_version = \"2.1\";\r\n\
LIN_speed = 19.2 kbps;\r\n\
Nodes { Master: M, 5 ms, 0.1 ms; Slaves: A, B; }\r\n\
composite { configuration Cfg { Phys { A, B; } } }\r\n\
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

/// 편집 연산을 고르는 씨앗. 현재 모델에 맞춰 해석하므로 항상 의미 있는 대상을 가리킨다.
#[derive(Clone, Debug)]
struct Seed {
    kind: u8,
    a: u32,
    b: u32,
    c: u32,
}

fn seeds() -> impl Strategy<Value = Vec<Seed>> {
    prop::collection::vec(
        (0u8..31, any::<u32>(), any::<u32>(), any::<u32>()).prop_map(|(kind, a, b, c)| Seed {
            kind,
            a,
            b,
            c,
        }),
        0..14,
    )
}

fn pick<T: Clone>(v: &[T], i: u32) -> Option<T> {
    (!v.is_empty()).then(|| v[i as usize % v.len()].clone())
}

fn names<I, T>(it: impl Iterator<Item = (I, T)>, f: impl Fn(T) -> String) -> Vec<String> {
    it.map(|(_, t)| f(t)).collect()
}

/// 씨앗을 현재 모델에 맞는 편집 연산으로 해석한다. 해석할 대상이 없으면 `None`.
fn interpret(m: &LdfModel, s: &Seed) -> Option<EditOp> {
    let nodes = names(m.nodes.iter(), |n| n.name.clone());
    let slaves = names(
        m.nodes.iter().filter(|(_, n)| n.role == NodeRole::Slave),
        |n| n.name.clone(),
    );
    let signals = names(m.signals.iter(), |x| x.name.clone());
    let normal = names(
        m.signals
            .iter()
            .filter(|(_, x)| x.kind == SignalKind::Normal),
        |x| x.name.clone(),
    );
    let frames = names(m.frames.iter(), |f| f.name.clone());
    let uncond = names(
        m.frames
            .iter()
            .filter(|(_, f)| matches!(f.kind, FrameKind::Unconditional { .. })),
        |f| f.name.clone(),
    );
    let encodings = names(m.encodings.iter(), |e| e.name.clone());
    let schedules = names(m.schedules.iter(), |t| t.name.clone());
    let (a, b, c) = (s.a, s.b, s.c);
    Some(match s.kind {
        0 => EditOp::AddSlave(m.unique_name("Slave")),
        1 => EditOp::RenameNode {
            from: pick(&slaves, a)?,
            to: m.unique_name("Node"),
        },
        2 => EditOp::DeleteNode(pick(&slaves, a)?),
        3 | 4 => {
            let draft = SignalDraft {
                name: if s.kind == 3 {
                    m.unique_name("Sig")
                } else {
                    pick(&signals, a)?
                },
                size_bits: 1 + b % 16,
                init: InitValue::Scalar(0),
                publisher: pick(&nodes, a),
                subscribers: pick(&nodes, c).into_iter().collect(),
                encoding: pick(&encodings, b).filter(|_| c % 3 == 0),
            };
            if s.kind == 3 {
                EditOp::AddSignal(draft)
            } else {
                EditOp::EditSignal {
                    name: pick(&signals, a)?,
                    draft,
                }
            }
        }
        5 => EditOp::DeleteSignal(pick(&signals, a)?),
        6 => EditOp::AddFrame(FrameDraft {
            name: m.unique_name("Frame"),
            kind: FrameDraftKind::Unconditional {
                id: m.free_frame_id()?,
                publisher: pick(&nodes, a)?,
                size: 1 + b % 8,
            },
        }),
        7 => EditOp::AddFrame(FrameDraft {
            name: m.unique_name("Frame"),
            kind: if c % 2 == 0 {
                FrameDraftKind::Sporadic {
                    associated: uncond.iter().take(1 + (b % 2) as usize).cloned().collect(),
                }
            } else {
                FrameDraftKind::EventTriggered {
                    id: m.free_frame_id()?,
                    collision_table: pick(&schedules, a).filter(|_| b % 2 == 0),
                    associated: uncond
                        .iter()
                        .skip(a as usize % 2)
                        .take(2)
                        .cloned()
                        .collect(),
                }
            },
        }),
        8 => {
            let name = pick(&uncond, a)?;
            let f = m.frames.get(m.find_frame(&name)?)?;
            let FrameKind::Unconditional { publisher, .. } = &f.kind else {
                return None;
            };
            let publisher = match publisher {
                ldf_model::Ref::Resolved(id) => m.nodes.get(*id)?.name.clone(),
                ldf_model::Ref::Unresolved(n) => n.clone(),
            };
            EditOp::EditFrame {
                name,
                draft: FrameDraft {
                    name: m.unique_name("Renamed"),
                    kind: FrameDraftKind::Unconditional {
                        id: b % 60,
                        publisher,
                        size: 1 + c % 8,
                    },
                },
            }
        }
        9 => EditOp::DeleteFrame(pick(&frames, a)?),
        10 => EditOp::PlaceSignal {
            frame: pick(&uncond, a)?,
            signal: pick(&normal, b)?,
            offset: c % 64,
        },
        11 => EditOp::UnplaceSignal {
            frame: pick(&uncond, a)?,
            signal: pick(&normal, b)?,
        },
        12 => EditOp::MoveSignal {
            frame: pick(&uncond, a)?,
            signal: pick(&normal, b)?,
            offset: c % 64,
        },
        13 => EditOp::AddEncoding {
            name: m.unique_name("Enc"),
            entries: vec![EncodingEntry::Logical {
                value: u64::from(a % 100),
                text: "v".into(),
            }],
        },
        14 => EditOp::EditEncoding {
            name: pick(&encodings, a)?,
            new_name: m.unique_name("Enc"),
            entries: vec![EncodingEntry::Physical {
                min: 0,
                max: i64::from(b % 255),
                scale: 1.0,
                offset: 0.0,
                unit: "u".into(),
            }],
        },
        15 => EditOp::DeleteEncoding(pick(&encodings, a)?),
        16 => EditOp::AddSchedule(m.unique_name("Table")),
        17 => EditOp::RenameSchedule {
            from: pick(&schedules, a)?,
            to: m.unique_name("Table"),
        },
        18 => EditOp::DeleteSchedule(pick(&schedules, a)?),
        19 | 20 => EditOp::AddSlot {
            table: pick(&schedules, a)?,
            index: (b % 3 == 0).then_some(0),
            entry: if c % 2 == 0 {
                SlotDraft::Frame(pick(&frames, b)?)
            } else {
                SlotDraft::AssignNad(pick(&slaves, b)?)
            },
            delay_ms: f64::from(5 * (1 + c % 6)),
        },
        21 => EditOp::RemoveSlot {
            table: pick(&schedules, a)?,
            index: (b % 4) as usize,
        },
        22 => EditOp::MoveSlot {
            table: pick(&schedules, a)?,
            from: (b % 3) as usize,
            to: (c % 3) as usize,
        },
        23 => EditOp::SetSlotDelay {
            table: pick(&schedules, a)?,
            index: (b % 3) as usize,
            delay_ms: f64::from(1 + c % 50),
        },
        24 => EditOp::SetNodeAttrs {
            node: pick(&slaves, a)?,
            attrs: AttrsDraft {
                lin_protocol: Some("2.1".into()),
                configured_nad: Some(1 + b % 120),
                response_error: pick(&normal, c).filter(|_| b % 2 == 0),
                configurable_frames: uncond
                    .iter()
                    .take((c % 3) as usize)
                    .map(|f| (f.clone(), None))
                    .collect(),
                ..AttrsDraft::default()
            },
        },
        25 => EditOp::SetSpeed(1_000 + a % 19_000),
        26 => EditOp::SetChannelName(Some(m.unique_name("Chan"))),
        27 => EditOp::SetMasterTiming {
            node: pick(&nodes, 0)?,
            timebase_ms: f64::from(1 + a % 10),
            jitter_ms: f64::from(b % 3),
        },
        28 => EditOp::Batch(vec![
            EditOp::AddSlave(m.unique_name("Slave")),
            EditOp::DeleteSignal(pick(&signals, a)?),
        ]),
        30 => {
            // 노드 구성을 새로 정한다: 합성 노드 하나에 슬레이브 한두 개
            let first = pick(&slaves, a)?;
            let mut logical = vec![first.clone()];
            if let Some(second) = pick(&slaves, b)
                && second != first
            {
                logical.push(second);
            }
            EditOp::SetComposition(Some(ldf_model::Composition {
                keyword: if c % 2 == 0 {
                    "composite"
                } else {
                    "Node_composition"
                }
                .to_owned(),
                configurations: vec![ldf_model::Configuration {
                    name: "Cfg".to_owned(),
                    composites: vec![ldf_model::CompositeNode {
                        name: m.unique_name("Comp"),
                        logical,
                    }],
                }],
                colon_form: c % 3 == 0,
                span: None,
            }))
        }
        _ => EditOp::DeleteNode(pick(&slaves, b)?),
    })
}

fn config() -> ProptestConfig {
    // PROPTEST_CASES 환경 변수가 있으면 그것을 따르고, 없으면 빠르게 끝나는 값을 쓴다
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(48);
    ProptestConfig {
        cases,
        max_shrink_iters: 64,
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(config())]

    /// 편집한 모델은 쓰고 다시 읽어도 같고, 다시 쓴 텍스트는 처음 쓴 것과 같다. 실패한 연산은 모델을 바꾸지 않는다.
    #[test]
    fn edited_models_round_trip_through_text(seq in seeds()) {
        let mut m = load(BASE).model;
        for s in &seq {
            let Some(op) = interpret(&m, s) else { continue };
            let before = m.clone();
            match op.apply(&mut m) {
                Err(_) => prop_assert_eq!(&m, &before, "실패한 연산이 모델을 바꿨다: {:?}", op),
                Ok(()) => {
                    let text = write(&m, &WriteOptions::default()).text;
                    let again = load(&text);
                    prop_assert!(
                        again.diagnostics.iter().all(|d| d.code < 9000),
                        "{:?}\n{:?}\n{}", op, again.diagnostics, text
                    );
                    prop_assert!(
                        semantically_equal(&m, &again.model),
                        "{:?}: {:?}\n{}", op, ldf_model::semantic_diff(&m, &again.model), text
                    );
                    prop_assert_eq!(&write(&again.model, &WriteOptions::default()).text, &text, "고정점이 아니다");
                    // 해석된 참조는 끝까지 해석되어 있다(연산이 미정의 참조를 만들지 않는다)
                    let undefined: Vec<_> = ldf_check::run_all(&again.model)
                        .into_iter()
                        .filter(|d| d.code == 20 && d.message.contains("정의되지 않았다"))
                        .collect();
                    prop_assert!(undefined.is_empty(), "{:?}: {:?}", op, undefined);
                    m = again.model;
                }
            }
        }
    }

    /// 세션에서 편집을 쌓은 만큼 Undo하면 처음 텍스트와 모델로 정확히 돌아온다.
    #[test]
    fn sessions_undo_back_to_the_start(seq in seeds()) {
        let mut s = Session::open_in_memory().expect("session");
        let base = s.import_bytes("base.ldf", BASE.as_bytes()).expect("import").snapshot;
        let mut snap = base.clone();
        let mut steps = 0;
        for seed in &seq {
            let Some(op) = interpret(&snap.model, seed) else { continue };
            if let Ok(next) = s.edit(&snap, &op) {
                snap = next;
                steps += 1;
            }
        }
        let mut redo_target = None;
        for _ in 0..steps {
            redo_target.get_or_insert_with(|| snap.clone());
            snap = s.undo(&snap).expect("undo");
        }
        prop_assert_eq!(&snap.source, &base.source);
        prop_assert_eq!(&snap.model, &base.model);
        prop_assert!(!snap.dirty && !snap.can_undo);
        // 끝까지 다시 하면 편집한 마지막 상태다
        if let Some(last) = redo_target {
            for _ in 0..steps {
                snap = s.redo(&snap).expect("redo");
            }
            prop_assert_eq!(&snap.source, &last.source);
        }
    }

    /// 임의로 망가뜨린 텍스트를 분석해도 패닉하지 않고, 그 모델을 써도 패닉하지 않는다.
    #[test]
    fn mutated_text_never_panics(
        edits in prop::collection::vec((0usize..BASE.len(), 0u8..4, any::<char>()), 0..24),
        cut in 0usize..BASE.len(),
    ) {
        let mut chars: Vec<char> = BASE.chars().collect();
        for (pos, kind, ch) in edits {
            let i = pos % chars.len().max(1);
            match kind {
                0 if !chars.is_empty() => { chars.remove(i); }
                1 => chars.insert(i.min(chars.len()), ch),
                2 if !chars.is_empty() => chars[i] = ch,
                _ if chars.len() > 1 => {
                    let j = (i * 7 + 3) % chars.len();
                    chars.swap(i, j);
                }
                _ => {}
            }
        }
        chars.truncate(chars.len().saturating_sub(cut % 40));
        let text: String = chars.into_iter().collect();
        let a = analyze(text.as_bytes());
        let _ = write(&a.model, &WriteOptions::default());
    }

    /// 임의의 바이트(UTF-8이 아니어도)를 분석해도 패닉하지 않는다.
    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        let a = analyze(&bytes);
        let _ = write(&a.model, &WriteOptions::default());
    }
}
