//! 오브젝트 컨텍스트 뷰: 선택한 객체의 하위 목록을 표로 만든다.

use super::Selection;
use super::derive::{self, Named, uint};
use super::nav;
use ldf_model::{FrameId, FrameKind, LdfModel, Ref, ScheduleId, SignalId};

/// 표의 칸.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    /// 표시 텍스트.
    pub text: String,
    /// 이름을 못 찾은 참조인지(빨갛게 표시).
    pub unresolved: bool,
}

impl Cell {
    fn text(t: impl Into<String>) -> Self {
        Self {
            text: t.into(),
            unresolved: false,
        }
    }

    fn named(n: &Named) -> Self {
        Self {
            text: n.text.clone(),
            unresolved: n.unresolved,
        }
    }

    fn names(list: &[Named]) -> Self {
        Self {
            text: list
                .iter()
                .map(|n| n.text.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            unresolved: list.iter().any(|n| n.unresolved),
        }
    }
}

/// 표의 행. 클릭하면 `target`이 선택된다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// 칸.
    pub cells: Vec<Cell>,
    /// 클릭 시 선택할 대상.
    pub target: Selection,
}

/// 컨텍스트 뷰 표.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    /// 표 제목.
    pub title: String,
    /// 열 이름.
    pub columns: Vec<&'static str>,
    /// 행.
    pub rows: Vec<Row>,
}

const FRAME_COLS: [&str; 5] = ["Frame", "ID", "Length [Byte]", "Publisher", "Subscribers"];
const SIGNAL_COLS: [&str; 7] = [
    "Signal",
    "Start bit",
    "Length [Bit]",
    "Init",
    "Publisher",
    "Subscribers",
    "Encoding",
];
const SLOT_COLS: [&str; 7] = [
    "Frame / Command",
    "Type",
    "ID",
    "Length [Byte]",
    "Publisher",
    "Subscribers",
    "Delay [ms]",
];

fn dash() -> Cell {
    Cell::text("—")
}

fn frame_row(m: &LdfModel, id: FrameId, hex: bool) -> Row {
    let Some(f) = m.frames.get(id) else {
        return Row {
            cells: vec![Cell::text("?")],
            target: Selection::None,
        };
    };
    let lin = f.lin_id().map_or_else(dash, |v| Cell::text(uint(v, hex)));
    let size = derive::frame_size(m, id).map_or_else(dash, |v| Cell::text(v.to_string()));
    Row {
        cells: vec![
            Cell::text(f.name.clone()),
            lin,
            size,
            Cell::names(&derive::frame_publishers(m, id)),
            Cell::names(&derive::frame_subscribers(m, id)),
        ],
        target: Selection::Frame(id),
    }
}

fn frame_table(
    m: &LdfModel,
    title: String,
    ids: impl IntoIterator<Item = FrameId>,
    hex: bool,
) -> Table {
    Table {
        title,
        columns: FRAME_COLS.to_vec(),
        rows: ids.into_iter().map(|id| frame_row(m, id, hex)).collect(),
    }
}

/// 시그널 한 줄. `placement`가 있으면 프레임 안 시작 비트를 보여준다.
fn signal_row(m: &LdfModel, id: SignalId, placement: Option<u32>, hex: bool) -> Row {
    let Some(s) = m.signals.get(id) else {
        return Row {
            cells: vec![Cell::text("?")],
            target: Selection::None,
        };
    };
    let publisher = s.publisher.as_ref().map(|p| derive::node_ref(m, p));
    let subs: Vec<Named> = s
        .subscribers
        .iter()
        .map(|r| derive::node_ref(m, r))
        .collect();
    Row {
        cells: vec![
            Cell::text(s.name.clone()),
            placement.map_or_else(dash, |o| Cell::text(uint(o, hex))),
            Cell::text(s.size_bits.to_string()),
            Cell::text(derive::init_text(&s.init, hex)),
            publisher.as_ref().map_or_else(dash, Cell::named),
            Cell::names(&subs),
            s.encoding()
                .map_or_else(dash, |e| Cell::named(&derive::encoding_ref(m, e))),
        ],
        target: Selection::Signal(id),
    }
}

fn frame_ids_where(m: &LdfModel, keep: impl Fn(&FrameKind) -> bool) -> Vec<FrameId> {
    m.frames
        .iter()
        .filter(|(_, f)| keep(&f.kind))
        .map(|(i, _)| i)
        .collect()
}

fn schedule_table(m: &LdfModel, sched: ScheduleId, hex: bool) -> Table {
    let Some(t) = m.schedules.get(sched) else {
        return Table::default();
    };
    let mut rows = Vec::new();
    for i in 0..t.slots.len() {
        let Some(info) = derive::slot_info(m, sched, i) else {
            continue;
        };
        let (id, size, publ, subs) = match info.frame {
            Some(f) => {
                let r = frame_row(m, f, hex);
                let get = |i: usize| r.cells.get(i).cloned().unwrap_or_else(dash);
                (get(1), get(2), get(3), get(4))
            }
            None => (dash(), dash(), dash(), dash()),
        };
        rows.push(Row {
            cells: vec![
                Cell::named(&info.name),
                Cell::text(info.kind.clone()),
                id,
                size,
                publ,
                subs,
                Cell::text(derive::ms(info.delay_ms)),
            ],
            target: Selection::Slot(sched, i),
        });
    }
    Table {
        title: format!("Frames in Schedule Table <{}>", t.name),
        columns: SLOT_COLS.to_vec(),
        rows,
    }
}

/// 선택에 맞는 컨텍스트 뷰 표.
pub fn context_table(m: &LdfModel, sel: &Selection, hex: bool) -> Table {
    match sel {
        Selection::None => Table::default(),
        Selection::Cluster => {
            frame_table(m, "Frames".to_owned(), m.frames.iter().map(|(i, _)| i), hex)
        }
        Selection::Group(g) => {
            let ids = frame_ids_where(m, |k| {
                matches!(
                    (g, k),
                    (
                        super::FrameGroup::Unconditional,
                        FrameKind::Unconditional { .. }
                    ) | (super::FrameGroup::Diagnostic, FrameKind::Diagnostic { .. })
                        | (
                            super::FrameGroup::EventTriggered,
                            FrameKind::EventTriggered { .. }
                        )
                        | (super::FrameGroup::Sporadic, FrameKind::Sporadic { .. })
                )
            });
            frame_table(m, g.label().to_owned(), ids, hex)
        }
        Selection::Frame(id) => {
            let Some(f) = m.frames.get(*id) else {
                return Table::default();
            };
            match &f.kind {
                FrameKind::Unconditional { signals, .. }
                | FrameKind::Diagnostic { signals, .. } => Table {
                    title: format!("Signals in Frame <{}>", f.name),
                    columns: SIGNAL_COLS.to_vec(),
                    rows: signals
                        .iter()
                        .map(|fs| match fs.signal {
                            Ref::Resolved(sid) => signal_row(m, sid, Some(fs.offset), hex),
                            Ref::Unresolved(ref name) => Row {
                                cells: vec![
                                    Cell {
                                        text: name.clone(),
                                        unresolved: true,
                                    },
                                    Cell::text(uint(fs.offset, hex)),
                                ],
                                target: Selection::None,
                            },
                        })
                        .collect(),
                },
                FrameKind::Sporadic { associated }
                | FrameKind::EventTriggered { associated, .. } => {
                    let mut t = frame_table(
                        m,
                        format!("Associated Frames of <{}>", f.name),
                        associated.iter().filter_map(Ref::id),
                        hex,
                    );
                    for r in associated.iter().filter(|r| r.id().is_none()) {
                        if let Some(n) = r.unresolved_name() {
                            t.rows.push(Row {
                                cells: vec![Cell {
                                    text: n.to_owned(),
                                    unresolved: true,
                                }],
                                target: Selection::None,
                            });
                        }
                    }
                    t
                }
            }
        }
        Selection::Command(c) => Table {
            title: format!("Command <{c}>"),
            columns: vec!["설명"],
            rows: vec![Row {
                cells: vec![Cell::text("스케줄 테이블 슬롯으로 사용하는 진단 명령")],
                target: Selection::None,
            }],
        },
        Selection::Node(n) => {
            let name = m.nodes.get(*n).map(|x| x.name.clone()).unwrap_or_default();
            let mut ids = nav::node_frames(m, *n, true);
            for f in nav::node_frames(m, *n, false) {
                if !ids.contains(&f) {
                    ids.push(f);
                }
            }
            frame_table(m, format!("Frames of <{name}>"), ids, hex)
        }
        Selection::NodeGroup(n, g) => {
            use super::NodeGroup::*;
            let name = m.nodes.get(*n).map(|x| x.name.clone()).unwrap_or_default();
            let title = format!("{} of <{name}>", g.label());
            match g {
                PublishedSignals | SubscribedSignals => Table {
                    title,
                    columns: SIGNAL_COLS.to_vec(),
                    rows: nav::node_signals(m, *n, *g == PublishedSignals)
                        .into_iter()
                        .map(|s| signal_row(m, s, None, hex))
                        .collect(),
                },
                PublishedFrames | SubscribedFrames => frame_table(
                    m,
                    title,
                    nav::node_frames(m, *n, *g == PublishedFrames),
                    hex,
                ),
            }
        }
        Selection::Signal(s) => {
            let name = m
                .signals
                .get(*s)
                .map(|x| x.name.clone())
                .unwrap_or_default();
            let ids = m
                .frames
                .iter()
                .filter(|(_, f)| f.signals().iter().any(|fs| fs.signal.id() == Some(*s)))
                .map(|(i, _)| i);
            frame_table(m, format!("Frames containing <{name}>"), ids, hex)
        }
        Selection::Schedule(s) | Selection::Slot(s, _) => schedule_table(m, *s, hex),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::NodeGroup;
    use ldf_model::NodeId;

    fn model() -> LdfModel {
        ldf_model::load(
            "LIN_description_file; LIN_speed = 19.2 kbps;\n\
             Nodes { Master: GW, 5 ms, 0.1 ms; Slaves: A, B; }\n\
             Signals { S1: 4, 0x5, A, GW, B; S2: 8, {1, 2}, B, GW; Lost: 1, 0, Ghost, GW; }\n\
             Frames { FA: 0x10, A, 2 { S1, 0; S2, 8; Missing, 12; } FB: 0x11, B, 1 { S2, 0; } }\n\
             Sporadic_frames { Sp: FA, Gone; }\n\
             Schedule_tables { T { FA delay 10 ms; Sp delay 10 ms; AssignNAD { A } delay 5 ms; } }\n\
             Signal_encoding_types { E { logical_value, 0, \"off\"; } }\n\
             Signal_representation { E: S1; }\n",
        )
        .model
    }

    fn frame(m: &LdfModel, name: &str) -> FrameId {
        m.frames
            .iter()
            .find(|(_, f)| f.name == name)
            .map(|(i, _)| i)
            .expect("frame")
    }

    fn row_texts(t: &Table) -> Vec<Vec<String>> {
        t.rows
            .iter()
            .map(|r| r.cells.iter().map(|c| c.text.clone()).collect())
            .collect()
    }

    #[test]
    fn frame_selection_lists_its_signals_with_start_bits() {
        let m = model();
        let t = context_table(&m, &Selection::Frame(frame(&m, "FA")), false);
        assert_eq!(t.title, "Signals in Frame <FA>");
        assert_eq!(t.columns, SIGNAL_COLS);
        let rows = row_texts(&t);
        assert_eq!(rows[0], ["S1", "0", "4", "5", "A", "GW, B", "E"]);
        assert_eq!(rows[1], ["S2", "8", "8", "{1, 2}", "B", "GW", "—"]);
        // 없는 시그널은 빨갛게 표시한다
        assert!(t.rows[2].cells[0].unresolved);
        assert_eq!(t.rows[2].cells[0].text, "Missing");
        assert_eq!(t.rows[2].target, Selection::None);
        assert!(matches!(t.rows[0].target, Selection::Signal(_)));
    }

    #[test]
    fn hex_toggle_changes_ids_and_start_bits() {
        let m = model();
        let t = context_table(
            &m,
            &Selection::Group(crate::view::FrameGroup::Unconditional),
            true,
        );
        assert_eq!(row_texts(&t)[0][1], "0x10");
        let t = context_table(&m, &Selection::Frame(frame(&m, "FA")), true);
        assert_eq!(row_texts(&t)[1][1], "0x08");
    }

    #[test]
    fn group_selection_lists_frames_of_that_kind() {
        let m = model();
        let t = context_table(
            &m,
            &Selection::Group(crate::view::FrameGroup::Unconditional),
            false,
        );
        assert_eq!(
            row_texts(&t)
                .iter()
                .map(|r| r[0].as_str())
                .collect::<Vec<_>>(),
            ["FA", "FB"]
        );
        assert_eq!(row_texts(&t)[0], ["FA", "16", "2", "A", "GW, B"]);
        let t = context_table(
            &m,
            &Selection::Group(crate::view::FrameGroup::Sporadic),
            false,
        );
        assert_eq!(row_texts(&t)[0][0], "Sp");
    }

    #[test]
    fn sporadic_frame_lists_associated_frames_including_broken_refs() {
        let m = model();
        let t = context_table(&m, &Selection::Frame(frame(&m, "Sp")), false);
        assert_eq!(t.title, "Associated Frames of <Sp>");
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0].cells[0].text, "FA");
        assert!(t.rows[1].cells[0].unresolved);
    }

    #[test]
    fn schedule_selection_lists_slots() {
        let m = model();
        let t = context_table(&m, &Selection::Schedule(ScheduleId(0)), false);
        assert_eq!(t.title, "Frames in Schedule Table <T>");
        assert_eq!(t.rows.len(), 3);
        assert_eq!(
            row_texts(&t)[0],
            ["FA", "Unconditional", "16", "2", "A", "GW, B", "10.00"]
        );
        assert_eq!(row_texts(&t)[2][..2], ["AssignNAD", "AssignNAD"]);
        assert_eq!(t.rows[1].target, Selection::Slot(ScheduleId(0), 1));
        // 슬롯을 골라도 같은 표가 나온다
        assert_eq!(
            context_table(&m, &Selection::Slot(ScheduleId(0), 2), false),
            t
        );
    }

    #[test]
    fn node_and_node_group_selections() {
        let m = model();
        let b = NodeId(2);
        let t = context_table(&m, &Selection::Node(b), false);
        assert_eq!(t.title, "Frames of <B>");
        assert_eq!(
            row_texts(&t)
                .iter()
                .map(|r| r[0].as_str())
                .collect::<Vec<_>>(),
            // 발행하는 프레임(FB)이 먼저, 그다음 구독하는 프레임
            ["FB", "FA", "Sp"]
        );

        let t = context_table(
            &m,
            &Selection::NodeGroup(b, NodeGroup::PublishedSignals),
            false,
        );
        assert_eq!(t.title, "Published Signals of <B>");
        assert_eq!(row_texts(&t)[0][0], "S2");
        // 프레임에 배치되지 않은 보기에서는 시작 비트가 없다
        assert_eq!(row_texts(&t)[0][1], "—");
    }

    #[test]
    fn signal_selection_lists_frames_containing_it() {
        let m = model();
        let s2 = m
            .signals
            .iter()
            .find(|(_, s)| s.name == "S2")
            .map(|(i, _)| i)
            .expect("sig");
        let t = context_table(&m, &Selection::Signal(s2), false);
        assert_eq!(
            row_texts(&t)
                .iter()
                .map(|r| r[0].as_str())
                .collect::<Vec<_>>(),
            ["FA", "FB"]
        );
    }

    #[test]
    fn empty_selections_give_empty_tables() {
        let m = model();
        assert_eq!(context_table(&m, &Selection::None, false), Table::default());
        assert!(
            context_table(&m, &Selection::Frame(FrameId(999)), false)
                .rows
                .is_empty()
        );
        assert!(
            context_table(&m, &Selection::Schedule(ScheduleId(9)), false)
                .rows
                .is_empty()
        );
    }

    #[test]
    fn unresolved_publisher_is_flagged() {
        let m = model();
        let lost = m
            .signals
            .iter()
            .find(|(_, s)| s.name == "Lost")
            .map(|(i, _)| i)
            .expect("sig");
        let t = context_table(
            &m,
            &Selection::NodeGroup(NodeId(0), NodeGroup::SubscribedSignals),
            false,
        );
        let row = t
            .rows
            .iter()
            .find(|r| r.target == Selection::Signal(lost))
            .expect("row");
        assert!(row.cells[4].unresolved);
        assert_eq!(row.cells[4].text, "Ghost");
    }
}
