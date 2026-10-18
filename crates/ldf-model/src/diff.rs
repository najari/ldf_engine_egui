//! 두 모델의 차이(FR-REV-03): 추가·삭제·변경된 객체와 필드별 이전/이후 값.
//!
//! 객체는 종류 안에서 **이름**으로 짝짓는다. 이름이 사라진 프레임이 다른 이름으로 나타났고 종류와 LIN ID가 같으면
//! 둘을 "이름 변경"으로 본다(공급사가 이름만 바꿔 보낸 경우를 삭제+추가로 오해하지 않도록).

use crate::{EncodingEntry, FrameKind, InitValue, LdfModel, NodeRole, Ref, SignalKind, SlotEntry};
use serde::Serialize;

/// 차이가 난 객체의 종류.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    /// 파일 머리(버전, 속도 등).
    Header,
    /// 노드.
    Node,
    /// 시그널.
    Signal,
    /// 프레임.
    Frame,
    /// 인코딩 타입.
    Encoding,
    /// 스케줄 테이블.
    Schedule,
    /// 슬레이브 노드 속성.
    NodeAttributes,
}

impl ObjectKind {
    /// 사람이 읽는 이름.
    pub fn label(self) -> &'static str {
        match self {
            ObjectKind::Header => "Header",
            ObjectKind::Node => "Node",
            ObjectKind::Signal => "Signal",
            ObjectKind::Frame => "Frame",
            ObjectKind::Encoding => "Encoding",
            ObjectKind::Schedule => "Schedule",
            ObjectKind::NodeAttributes => "Node attributes",
        }
    }
}

/// 변경의 종류.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    /// 새로 생겼다.
    Added,
    /// 없어졌다.
    Removed,
    /// 같은 이름의 객체가 바뀌었다.
    Changed,
    /// 이름만(또는 이름과 함께 값이) 바뀐 같은 프레임. `from`은 옛 이름.
    Renamed {
        /// 옛 이름.
        from: String,
    },
}

impl Change {
    /// 사람이 읽는 이름.
    pub fn label(&self) -> &'static str {
        match self {
            Change::Added => "추가",
            Change::Removed => "삭제",
            Change::Changed => "변경",
            Change::Renamed { .. } => "이름 변경",
        }
    }
}

/// 필드 하나의 이전/이후 값. 없는 값은 빈 문자열이다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FieldChange {
    /// 필드 이름.
    pub field: String,
    /// 이전 값.
    pub before: String,
    /// 이후 값.
    pub after: String,
}

/// 객체 하나의 차이.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiffEntry {
    /// 종류.
    pub kind: ObjectKind,
    /// 이름(이름 변경이면 새 이름).
    pub name: String,
    /// 변경의 종류.
    pub change: Change,
    /// 필드별 차이. 추가·삭제는 그 객체의 모든 필드 값을 담는다.
    pub fields: Vec<FieldChange>,
}

/// 두 모델의 차이.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Diff {
    /// 차이 목록(종류 순, 같은 종류 안에서는 새 모델의 순서, 삭제는 맨 뒤).
    pub entries: Vec<DiffEntry>,
}

impl Diff {
    /// 차이가 없는지.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// (추가, 삭제, 변경, 이름 변경) 개수.
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        let n = |f: fn(&Change) -> bool| self.entries.iter().filter(|e| f(&e.change)).count();
        (
            n(|c| matches!(c, Change::Added)),
            n(|c| matches!(c, Change::Removed)),
            n(|c| matches!(c, Change::Changed)),
            n(|c| matches!(c, Change::Renamed { .. })),
        )
    }
}

type Fields = Vec<(String, String)>;

fn f(name: &str, value: impl ToString) -> (String, String) {
    (name.to_owned(), value.to_string())
}

fn opt<T: ToString>(v: Option<T>) -> String {
    v.map_or_else(String::new, |v| v.to_string())
}

fn join(items: impl IntoIterator<Item = String>) -> String {
    items.into_iter().collect::<Vec<_>>().join(", ")
}

/// 이름으로 대상을 가리키는 참조의 표시 문자열.
fn rname<T: crate::arena::ArenaId>(r: &Ref<T>, lookup: impl Fn(T) -> Option<String>) -> String {
    match r {
        Ref::Resolved(id) => lookup(*id).unwrap_or_default(),
        Ref::Unresolved(n) => n.clone(),
    }
}

/// 한 객체의 이름, 필드, 이름 변경 짝짓기용 키.
struct Item {
    name: String,
    fields: Fields,
    key: Option<String>,
}

fn field_changes(a: &Fields, b: &Fields) -> Vec<FieldChange> {
    let mut out = Vec::new();
    for (k, va) in a {
        match b.iter().find(|(kb, _)| kb == k) {
            Some((_, vb)) if vb == va => {}
            Some((_, vb)) => out.push(FieldChange {
                field: k.clone(),
                before: va.clone(),
                after: vb.clone(),
            }),
            None => out.push(FieldChange {
                field: k.clone(),
                before: va.clone(),
                after: String::new(),
            }),
        }
    }
    for (k, vb) in b {
        if !a.iter().any(|(ka, _)| ka == k) {
            out.push(FieldChange {
                field: k.clone(),
                before: String::new(),
                after: vb.clone(),
            });
        }
    }
    out
}

fn all_fields(fields: &Fields, removed: bool) -> Vec<FieldChange> {
    fields
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| FieldChange {
            field: k.clone(),
            before: if removed { v.clone() } else { String::new() },
            after: if removed { String::new() } else { v.clone() },
        })
        .collect()
}

fn compare(kind: ObjectKind, a: &[Item], b: &[Item], out: &mut Vec<DiffEntry>) {
    let mut paired_from = vec![false; a.len()];
    let mut removed_candidates: Vec<usize> = Vec::new();
    for (i, x) in a.iter().enumerate() {
        if !b.iter().any(|y| y.name == x.name) {
            removed_candidates.push(i);
        }
    }
    for y in b {
        if let Some((i, x)) = a.iter().enumerate().find(|(_, x)| x.name == y.name) {
            paired_from[i] = true;
            let fields = field_changes(&x.fields, &y.fields);
            if !fields.is_empty() {
                out.push(DiffEntry {
                    kind,
                    name: y.name.clone(),
                    change: Change::Changed,
                    fields,
                });
            }
            continue;
        }
        // 새 이름: 사라진 같은 키의 객체가 있으면 이름 변경
        let renamed = y.key.as_ref().and_then(|k| {
            removed_candidates
                .iter()
                .copied()
                .find(|&i| !paired_from[i] && a[i].key.as_ref() == Some(k))
        });
        match renamed {
            Some(i) => {
                paired_from[i] = true;
                out.push(DiffEntry {
                    kind,
                    name: y.name.clone(),
                    change: Change::Renamed {
                        from: a[i].name.clone(),
                    },
                    fields: field_changes(&a[i].fields, &y.fields),
                });
            }
            None => out.push(DiffEntry {
                kind,
                name: y.name.clone(),
                change: Change::Added,
                fields: all_fields(&y.fields, false),
            }),
        }
    }
    for (i, x) in a.iter().enumerate() {
        if !paired_from[i] && !b.iter().any(|y| y.name == x.name) {
            out.push(DiffEntry {
                kind,
                name: x.name.clone(),
                change: Change::Removed,
                fields: all_fields(&x.fields, true),
            });
        }
    }
}

fn header_item(m: &LdfModel) -> Item {
    let h = &m.header;
    Item {
        name: "header".to_owned(),
        fields: vec![
            f("protocol_version", opt(h.protocol_version.as_ref())),
            f("language_version", opt(h.language_version.as_ref())),
            f("speed_bps", opt(h.speed_bps)),
            f("channel_name", opt(h.channel_name.as_ref())),
            f("ldf_file_revision", opt(h.ldf_file_revision.as_ref())),
            f("sig_byte_order_big_endian", h.sig_byte_order_big_endian),
        ],
        key: None,
    }
}

fn node_items(m: &LdfModel) -> Vec<Item> {
    m.nodes
        .iter()
        .map(|(_, n)| Item {
            name: n.name.clone(),
            fields: vec![
                f(
                    "role",
                    if n.role == NodeRole::Master {
                        "master"
                    } else {
                        "slave"
                    },
                ),
                f("timebase_ms", opt(n.timebase_ms)),
                f("jitter_ms", opt(n.jitter_ms)),
                f("max_header_bits", opt(n.max_header_bits)),
                f("response_tolerance_pct", opt(n.response_tolerance_pct)),
            ],
            key: None,
        })
        .collect()
}

fn signal_items(m: &LdfModel) -> Vec<Item> {
    let node = |r: &Ref<crate::NodeId>| rname(r, |id| m.nodes.get(id).map(|n| n.name.clone()));
    let enc =
        |r: &Ref<crate::EncodingId>| rname(r, |id| m.encodings.get(id).map(|n| n.name.clone()));
    m.signals
        .iter()
        .map(|(_, s)| Item {
            name: s.name.clone(),
            fields: vec![
                f(
                    "kind",
                    if s.kind == SignalKind::Normal {
                        "normal"
                    } else {
                        "diagnostic"
                    },
                ),
                f("size_bits", s.size_bits),
                f(
                    "init",
                    match &s.init {
                        InitValue::Scalar(v) => v.to_string(),
                        InitValue::Array(b) => format!("{{{}}}", join(b.iter().map(u8::to_string))),
                    },
                ),
                f("publisher", opt(s.publisher.as_ref().map(&node))),
                f("subscribers", join(s.subscribers.iter().map(&node))),
                f("encodings", join(s.encodings.iter().map(&enc))),
            ],
            key: None,
        })
        .collect()
}

fn frame_items(m: &LdfModel) -> Vec<Item> {
    let node = |r: &Ref<crate::NodeId>| rname(r, |id| m.nodes.get(id).map(|n| n.name.clone()));
    let frame = |r: &Ref<crate::FrameId>| rname(r, |id| m.frames.get(id).map(|n| n.name.clone()));
    let sig = |r: &Ref<crate::SignalId>| rname(r, |id| m.signals.get(id).map(|n| n.name.clone()));
    let table =
        |r: &Ref<crate::ScheduleId>| rname(r, |id| m.schedules.get(id).map(|n| n.name.clone()));
    let placed = |v: &[crate::FrameSignal]| {
        join(
            v.iter()
                .map(|fs| format!("{}@{}", sig(&fs.signal), fs.offset)),
        )
    };
    m.frames
        .iter()
        .map(|(_, fr)| {
            let (fields, key) = match &fr.kind {
                FrameKind::Unconditional {
                    id,
                    publisher,
                    size,
                    signals,
                } => (
                    vec![
                        f("kind", "unconditional"),
                        f("id", id),
                        f("publisher", node(publisher)),
                        f("size", size),
                        f("signals", placed(signals)),
                    ],
                    Some(format!("unconditional:{id}")),
                ),
                FrameKind::Sporadic { associated } => (
                    vec![
                        f("kind", "sporadic"),
                        f("associated", join(associated.iter().map(&frame))),
                    ],
                    None,
                ),
                FrameKind::EventTriggered {
                    id,
                    collision_table,
                    associated,
                } => (
                    vec![
                        f("kind", "event_triggered"),
                        f("id", id),
                        f("collision_table", opt(collision_table.as_ref().map(&table))),
                        f("associated", join(associated.iter().map(&frame))),
                    ],
                    Some(format!("event_triggered:{id}")),
                ),
                FrameKind::Diagnostic { id, signals } => (
                    vec![
                        f("kind", "diagnostic"),
                        f("id", id),
                        f("signals", placed(signals)),
                    ],
                    Some(format!("diagnostic:{id}")),
                ),
            };
            Item {
                name: fr.name.clone(),
                fields,
                key,
            }
        })
        .collect()
}

fn encoding_items(m: &LdfModel) -> Vec<Item> {
    m.encodings
        .iter()
        .map(|(_, e)| {
            let entries = e.entries.iter().map(|x| match x {
                EncodingEntry::Logical { value, text } => format!("logical {value} \"{text}\""),
                EncodingEntry::Physical {
                    min,
                    max,
                    scale,
                    offset,
                    unit,
                } => format!("physical {min}..{max} x{scale} +{offset} \"{unit}\""),
                EncodingEntry::Bcd => "bcd".to_owned(),
                EncodingEntry::Ascii => "ascii".to_owned(),
            });
            Item {
                name: e.name.clone(),
                fields: vec![f("entries", entries.collect::<Vec<_>>().join("; "))],
                key: None,
            }
        })
        .collect()
}

fn schedule_items(m: &LdfModel) -> Vec<Item> {
    let node = |r: &Ref<crate::NodeId>| rname(r, |id| m.nodes.get(id).map(|n| n.name.clone()));
    let frame = |r: &Ref<crate::FrameId>| rname(r, |id| m.frames.get(id).map(|n| n.name.clone()));
    let nums = |v: &[u32]| join(v.iter().map(u32::to_string));
    m.schedules
        .iter()
        .map(|(_, t)| {
            let fields = t
                .slots
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let what = match &s.entry {
                        SlotEntry::Frame(r) => frame(r),
                        SlotEntry::AssignNad { node: n } => format!("AssignNAD {{ {} }}", node(n)),
                        SlotEntry::ConditionalChangeNad { args } => {
                            format!("ConditionalChangeNAD {{ {} }}", nums(args))
                        }
                        SlotEntry::DataDump { node: n, data } => {
                            format!("DataDump {{ {}, {} }}", node(n), nums(data))
                        }
                        SlotEntry::SaveConfiguration { node: n } => {
                            format!("SaveConfiguration {{ {} }}", node(n))
                        }
                        SlotEntry::AssignFrameIdRange {
                            node: n,
                            start_index,
                            pids,
                        } => format!(
                            "AssignFrameIdRange {{ {}, {start_index}, {} }}",
                            node(n),
                            nums(pids)
                        ),
                        SlotEntry::AssignFrameId { node: n, frame: fr } => {
                            format!("AssignFrameId {{ {}, {} }}", node(n), frame(fr))
                        }
                        SlotEntry::FreeFormat { data } => {
                            format!("FreeFormat {{ {} }}", nums(data))
                        }
                        SlotEntry::Unknown(raw) => raw.trim().to_owned(),
                    };
                    (
                        format!("slot[{}]", i + 1),
                        format!("{what} delay {} ms", s.delay_ms),
                    )
                })
                .collect();
            Item {
                name: t.name.clone(),
                fields,
                key: None,
            }
        })
        .collect()
}

fn attr_items(m: &LdfModel) -> Vec<Item> {
    let sig = |r: &Ref<crate::SignalId>| rname(r, |id| m.signals.get(id).map(|n| n.name.clone()));
    let frame = |r: &Ref<crate::FrameId>| rname(r, |id| m.frames.get(id).map(|n| n.name.clone()));
    m.node_attrs
        .iter()
        .map(|a| Item {
            name: rname(&a.node, |id| m.nodes.get(id).map(|n| n.name.clone())),
            fields: vec![
                f("lin_protocol", opt(a.lin_protocol.as_ref())),
                f("configured_nad", opt(a.configured_nad)),
                f("initial_nad", opt(a.initial_nad)),
                f(
                    "product_id",
                    a.product_id.as_ref().map_or_else(String::new, |p| {
                        format!("{}, {}, {}", p.supplier, p.function, opt(p.variant))
                    }),
                ),
                f("response_error", opt(a.response_error.as_ref().map(&sig))),
                f(
                    "fault_state_signals",
                    join(a.fault_state_signals.iter().map(&sig)),
                ),
                f("p2_min_ms", opt(a.p2_min_ms)),
                f("st_min_ms", opt(a.st_min_ms)),
                f("n_as_timeout_ms", opt(a.n_as_timeout_ms)),
                f("n_cr_timeout_ms", opt(a.n_cr_timeout_ms)),
                f(
                    "configurable_frames",
                    join(a.configurable_frames.iter().map(|c| match c.message_id {
                        Some(id) => format!("{} = {id:#X}", frame(&c.frame)),
                        None => frame(&c.frame),
                    })),
                ),
            ],
            key: None,
        })
        .collect()
}

/// 두 모델의 차이를 구한다. `a`가 이전, `b`가 이후다.
pub fn diff(a: &LdfModel, b: &LdfModel) -> Diff {
    let mut entries = Vec::new();
    compare(
        ObjectKind::Header,
        &[header_item(a)],
        &[header_item(b)],
        &mut entries,
    );
    compare(
        ObjectKind::Node,
        &node_items(a),
        &node_items(b),
        &mut entries,
    );
    compare(
        ObjectKind::Signal,
        &signal_items(a),
        &signal_items(b),
        &mut entries,
    );
    compare(
        ObjectKind::Frame,
        &frame_items(a),
        &frame_items(b),
        &mut entries,
    );
    compare(
        ObjectKind::Encoding,
        &encoding_items(a),
        &encoding_items(b),
        &mut entries,
    );
    compare(
        ObjectKind::Schedule,
        &schedule_items(a),
        &schedule_items(b),
        &mut entries,
    );
    compare(
        ObjectKind::NodeAttributes,
        &attr_items(a),
        &attr_items(b),
        &mut entries,
    );
    Diff { entries }
}

#[cfg(test)]
mod tests;
