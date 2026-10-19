//! NCF → LDF: NCF의 노드 하나를 열린 LDF의 슬레이브로 가져오는 편집 연산을 만든다.
//!
//! 결과는 `EditOp::Batch` 하나라서 Undo 한 번으로 되돌린다. 이 모듈은 모델을 바꾸지 않고 연산만 만든다(만든 뒤 복사본에 적용해 본다).
//!
//! 옮기는 규칙(LIN 2.2A 8장의 NCF → 9장의 LDF):
//! - `node 이름` → 슬레이브 노드, `general`·`diagnostic` → 노드 속성(`LIN_protocol`, `product_id`, `configured_NAD`, `P2_min` …).
//! - `publish` 프레임은 그 노드가 발행하는 무조건 프레임, `subscribe` 프레임은 마스터가 발행하는 무조건 프레임(그 노드가 구독).
//!   이미 있는 프레임·시그널은 모양이 같을 때만 그대로 쓰고 구독자만 더한다.
//! - NCF에는 프레임 ID가 없으므로(`message_ID`는 메시지 ID) 0부터 비어 있는 프레임 ID를 배정한다.
//! - 시그널이 없고 다른 프레임이 `event_triggered_frame`(또는 `event_triggered_message_ID`)로 가리키는 `publish` 프레임은
//!   이벤트 트리거 프레임이 된다.
//! - 인코딩은 같은 이름이 이미 있으면 내용이 같을 때만 쓴다.
//! - 스케줄 테이블에는 넣지 않는다.

use ldf_model::{
    AttrsDraft, EditOp, FrameDraft, FrameDraftKind, FrameKind, InitValue, LdfModel, NodeRole,
    ProductId, Ref, SignalDraft,
};
use ldf_ncf::{Direction, EncodingRef, EventTriggered, Nad, NcfFrame, NcfNode, NcfSignal};
use std::collections::{BTreeMap, BTreeSet};

/// 가져오기 계획.
#[derive(Clone, Debug)]
pub struct NcfImport {
    /// 적용할 연산(`EditOp::Batch`).
    pub op: EditOp,
    /// 가정한 것과 옮기지 않은 것에 대한 설명.
    pub notes: Vec<String>,
    /// 새로 생기는 프레임 수.
    pub frames: usize,
    /// 새로 생기는 시그널 수.
    pub signals: usize,
    /// 새로 생기는 인코딩 수.
    pub encodings: usize,
}

/// LIN 프레임 ID(0–0x3B) 중 비어 있는 것을 낮은 번호부터 내준다.
struct IdPool {
    used: BTreeSet<u32>,
}

impl IdPool {
    fn new(m: &LdfModel) -> Self {
        Self {
            used: m.frames.iter().filter_map(|(_, f)| f.lin_id()).collect(),
        }
    }

    fn take(&mut self) -> Option<u32> {
        let id = (0..=0x3B).find(|i| !self.used.contains(i))?;
        self.used.insert(id);
        Some(id)
    }
}

struct Planner<'a> {
    m: &'a LdfModel,
    n: &'a NcfNode,
    master: String,
    ops: Vec<EditOp>,
    notes: Vec<String>,
    errs: Vec<String>,
    ids: IdPool,
    /// 이 계획이 만드는 시그널 이름 → 크기.
    new_signals: BTreeMap<String, u32>,
    /// 이 계획이 만드는 프레임 수·시그널 수·인코딩 수.
    frames: usize,
    signals: usize,
    encodings: usize,
}

fn node_name(m: &LdfModel, r: &Ref<ldf_model::NodeId>) -> String {
    match r {
        Ref::Resolved(id) => m
            .nodes
            .get(*id)
            .map_or_else(String::new, |n| n.name.clone()),
        Ref::Unresolved(n) => n.clone(),
    }
}

/// 이미 있는 시그널을 시그널 초안으로(구독자를 더해 다시 보내려고).
fn draft_of(m: &LdfModel, name: &str) -> Option<SignalDraft> {
    let s = m.signals.get(m.find_signal(name)?)?;
    Some(SignalDraft {
        name: s.name.clone(),
        size_bits: s.size_bits,
        init: s.init.clone(),
        publisher: s.publisher.as_ref().map(|p| node_name(m, p)),
        subscribers: s.subscribers.iter().map(|r| node_name(m, r)).collect(),
        encoding: s.encodings.first().and_then(|r| match r {
            Ref::Resolved(id) => m.encodings.get(*id).map(|e| e.name.clone()),
            Ref::Unresolved(n) => Some(n.clone()),
        }),
    })
}

impl<'a> Planner<'a> {
    fn encoding_name(s: &NcfSignal) -> Option<&str> {
        match &s.encoding {
            Some(EncodingRef::Named(n)) => Some(n),
            Some(EncodingRef::Inline(e)) => Some(&e.name),
            None => None,
        }
    }

    /// 인코딩: 없으면 만들고, 있으면 같을 때만 쓴다.
    fn encodings(&mut self) {
        let mut added: BTreeMap<&str, &[ldf_model::EncodingEntry]> = BTreeMap::new();
        for e in self.n.all_encodings() {
            let existing = self
                .m
                .find_encoding(&e.name)
                .and_then(|id| self.m.encodings.get(id));
            match (existing, added.get(e.name.as_str())) {
                (Some(old), _) if old.entries == e.entries => self
                    .notes
                    .push(format!("인코딩 `{}`은(는) 이미 있어 그대로 쓴다", e.name)),
                (Some(_), _) => self.errs.push(format!(
                    "인코딩 `{}`이(가) 이미 있고 내용이 NCF와 다르다",
                    e.name
                )),
                (None, Some(prev)) if *prev != e.entries.as_slice() => self.errs.push(format!(
                    "인코딩 `{}`이(가) NCF에서 서로 다른 내용으로 두 번 정의됐다",
                    e.name
                )),
                (None, Some(_)) => {}
                (None, None) => {
                    added.insert(&e.name, &e.entries);
                    self.ops.push(EditOp::AddEncoding {
                        name: e.name.clone(),
                        entries: e.entries.clone(),
                    });
                    self.encodings += 1;
                }
            }
        }
    }

    fn next_id(&mut self, frame: &str) -> Option<u32> {
        match self.ids.take() {
            Some(id) => {
                self.notes.push(format!(
                    "프레임 `{frame}`의 ID를 {id:#04X}로 배정했다(NCF에는 프레임 ID가 없다)"
                ));
                Some(id)
            }
            None => {
                self.errs.push(format!(
                    "프레임 `{frame}`에 줄 수 있는 프레임 ID가 남아 있지 않다"
                ));
                None
            }
        }
    }

    /// 시그널이 이 프레임에서 쓸 수 있는 값(크기·시작 비트)을 갖췄는지.
    fn bits(&mut self, frame: &str, s: &NcfSignal) -> Option<(u32, u32)> {
        match (s.size, s.offset) {
            (Some(size), Some(offset)) if size > 0 => Some((size, offset)),
            _ => {
                self.errs.push(format!(
                    "프레임 `{frame}`의 시그널 `{}`에 size와 offset이 필요하다",
                    s.name
                ));
                None
            }
        }
    }

    fn init(s: &NcfSignal) -> InitValue {
        s.init.clone().unwrap_or(InitValue::Scalar(0))
    }

    /// 프레임을 만드는 연산(프레임 → 시그널 배치)을 쌓는다.
    fn place(&mut self, frame: &str, placed: &[(String, u32)]) {
        for (signal, offset) in placed {
            self.ops.push(EditOp::PlaceSignal {
                frame: frame.to_owned(),
                signal: signal.clone(),
                offset: *offset,
            });
        }
    }

    /// 새로 만들 시그널 하나(이미 같은 이름이 있으면 `reuse`로 처리한다).
    fn new_signal(
        &mut self,
        frame: &str,
        s: &NcfSignal,
        publisher: &str,
        subscribers: Vec<String>,
    ) -> Option<(String, u32)> {
        let (size, offset) = self.bits(frame, s)?;
        if self.new_signals.insert(s.name.clone(), size).is_some() {
            self.errs
                .push(format!("시그널 `{}`이(가) NCF에서 두 번 정의됐다", s.name));
            return None;
        }
        self.ops.push(EditOp::AddSignal(SignalDraft {
            name: s.name.clone(),
            size_bits: size,
            init: Self::init(s),
            publisher: Some(publisher.to_owned()),
            subscribers,
            encoding: Self::encoding_name(s).map(str::to_owned),
        }));
        self.signals += 1;
        Some((s.name.clone(), offset))
    }

    /// 이미 LDF에 있는 시그널을 그대로 쓴다: 크기가 같아야 하고, 이 노드를 구독자로 더한다.
    fn reuse_signal(&mut self, frame: &str, s: &NcfSignal) -> Option<(String, u32)> {
        let (size, offset) = self.bits(frame, s)?;
        let draft = draft_of(self.m, &s.name)?;
        if draft.size_bits != size {
            self.errs.push(format!(
                "시그널 `{}`의 크기가 LDF({}비트)와 NCF({size}비트)에서 다르다",
                s.name, draft.size_bits
            ));
            return None;
        }
        if !draft.subscribers.contains(&self.n.name) {
            let mut edited = draft.clone();
            edited.subscribers.push(self.n.name.clone());
            self.ops.push(EditOp::EditSignal {
                name: s.name.clone(),
                draft: edited,
            });
            self.notes.push(format!(
                "시그널 `{}`은(는) 이미 있어 `{}`을(를) 구독자에 더한다",
                s.name, self.n.name
            ));
        }
        Some((s.name.clone(), offset))
    }

    fn publish(&mut self, f: &NcfFrame) {
        let Some(length) = f.length else {
            self.errs
                .push(format!("프레임 `{}`에 length가 필요하다", f.name));
            return;
        };
        if self.m.find_frame(&f.name).is_some() {
            self.errs
                .push(format!("프레임 `{}`이(가) 이미 있다", f.name));
            return;
        }
        let Some(id) = self.next_id(&f.name) else {
            return;
        };
        let mut placed = Vec::new();
        for s in &f.signals {
            if self.m.find_signal(&s.name).is_some() {
                self.errs
                    .push(format!("시그널 `{}`이(가) 이미 있다", s.name));
                continue;
            }
            let subscribers = vec![self.master.clone()];
            let node = self.n.name.clone();
            if let Some(p) = self.new_signal(&f.name, s, &node, subscribers) {
                placed.push(p);
            }
        }
        self.ops.push(EditOp::AddFrame(FrameDraft {
            name: f.name.clone(),
            kind: FrameDraftKind::Unconditional {
                id,
                publisher: self.n.name.clone(),
                size: length,
            },
        }));
        self.frames += 1;
        self.place(&f.name, &placed);
    }

    fn subscribe(&mut self, f: &NcfFrame) {
        let Some(length) = f.length else {
            self.errs
                .push(format!("프레임 `{}`에 length가 필요하다", f.name));
            return;
        };
        if let Some(fid) = self.m.find_frame(&f.name) {
            // 이미 있는 프레임: 길이와 시그널 배치가 같아야 한다
            let Some(frame) = self.m.frames.get(fid) else {
                return;
            };
            let FrameKind::Unconditional { size, signals, .. } = &frame.kind else {
                self.errs.push(format!(
                    "프레임 `{}`이(가) 이미 있지만 무조건 프레임이 아니다",
                    f.name
                ));
                return;
            };
            if *size != length {
                self.errs.push(format!(
                    "프레임 `{}`의 길이가 LDF({size}바이트)와 NCF({length}바이트)에서 다르다",
                    f.name
                ));
                return;
            }
            for s in &f.signals {
                let same = signals.iter().any(|p| {
                    let name = match &p.signal {
                        Ref::Resolved(id) => self.m.signals.get(*id).map(|x| x.name.as_str()),
                        Ref::Unresolved(n) => Some(n.as_str()),
                    };
                    name == Some(s.name.as_str()) && Some(p.offset) == s.offset
                });
                if !same {
                    self.errs.push(format!(
                        "프레임 `{}`의 시그널 `{}`이(가) LDF의 배치와 맞지 않는다",
                        f.name, s.name
                    ));
                } else {
                    self.reuse_signal(&f.name, s);
                }
            }
            return;
        }
        let Some(id) = self.next_id(&f.name) else {
            return;
        };
        let mut placed = Vec::new();
        for s in &f.signals {
            let p = if self.m.find_signal(&s.name).is_some() {
                self.reuse_signal(&f.name, s)
            } else {
                let master = self.master.clone();
                let node = vec![self.n.name.clone()];
                self.new_signal(&f.name, s, &master, node)
            };
            placed.extend(p);
        }
        self.ops.push(EditOp::AddFrame(FrameDraft {
            name: f.name.clone(),
            kind: FrameDraftKind::Unconditional {
                id,
                publisher: self.master.clone(),
                size: length,
            },
        }));
        self.frames += 1;
        self.place(&f.name, &placed);
    }

    /// 이벤트 트리거 프레임 `etf`와 그것을 가리키는 프레임들.
    fn event_triggered(&mut self, etf: &NcfFrame, associated: Vec<String>) {
        if associated.is_empty() {
            self.notes.push(format!(
                "이벤트 트리거 프레임 `{}`에 연결된 프레임이 없어 만들지 않았다",
                etf.name
            ));
            return;
        }
        match self
            .m
            .find_frame(&etf.name)
            .and_then(|id| self.m.frames.get(id))
        {
            None => {
                let Some(id) = self.next_id(&etf.name) else {
                    return;
                };
                self.ops.push(EditOp::AddFrame(FrameDraft {
                    name: etf.name.clone(),
                    kind: FrameDraftKind::EventTriggered {
                        id,
                        collision_table: None,
                        associated,
                    },
                }));
                self.frames += 1;
            }
            Some(old) => {
                let FrameKind::EventTriggered {
                    id,
                    collision_table,
                    associated: have,
                } = &old.kind
                else {
                    self.errs.push(format!(
                        "프레임 `{}`이(가) 이미 있지만 이벤트 트리거 프레임이 아니다",
                        etf.name
                    ));
                    return;
                };
                let name_of = |r: &Ref<ldf_model::FrameId>| match r {
                    Ref::Resolved(f) => self.m.frames.get(*f).map(|x| x.name.clone()),
                    Ref::Unresolved(n) => Some(n.clone()),
                };
                let mut all: Vec<String> = have.iter().filter_map(name_of).collect();
                all.extend(associated);
                let table = collision_table.as_ref().and_then(|r| match r {
                    Ref::Resolved(s) => self.m.schedules.get(*s).map(|t| t.name.clone()),
                    Ref::Unresolved(n) => Some(n.clone()),
                });
                self.ops.push(EditOp::EditFrame {
                    name: etf.name.clone(),
                    draft: FrameDraft {
                        name: etf.name.clone(),
                        kind: FrameDraftKind::EventTriggered {
                            id: *id,
                            collision_table: table,
                            associated: all,
                        },
                    },
                });
                self.notes.push(format!(
                    "이벤트 트리거 프레임 `{}`은(는) 이미 있어 연관 프레임만 더한다",
                    etf.name
                ));
            }
        }
    }

    fn frames(&mut self) {
        // 시그널이 없고 다른 프레임이 가리키는 `publish` 프레임이 이벤트 트리거 프레임이다
        let by_name: BTreeSet<&str> = self
            .n
            .frames
            .iter()
            .filter_map(|f| match &f.event_triggered {
                Some(EventTriggered::Frame(x)) => Some(x.as_str()),
                _ => None,
            })
            .collect();
        let by_id: BTreeSet<u32> = self
            .n
            .frames
            .iter()
            .filter_map(|f| match &f.event_triggered {
                Some(EventTriggered::MessageId(x)) => Some(*x),
                _ => None,
            })
            .collect();
        let is_etf = |f: &NcfFrame| {
            f.direction == Direction::Publish
                && f.signals.is_empty()
                && (by_name.contains(f.name.as_str())
                    || f.message_id.is_some_and(|id| by_id.contains(&id)))
        };
        let n = self.n;
        for f in n.frames.iter().filter(|f| !is_etf(f)) {
            match f.direction {
                Direction::Publish => self.publish(f),
                Direction::Subscribe => self.subscribe(f),
            }
        }
        for etf in n.frames.iter().filter(|f| is_etf(f)) {
            let associated: Vec<String> = n
                .frames
                .iter()
                .filter(|f| !is_etf(f))
                .filter(|f| match &f.event_triggered {
                    Some(EventTriggered::Frame(x)) => *x == etf.name,
                    Some(EventTriggered::MessageId(x)) => etf.message_id == Some(*x),
                    None => false,
                })
                .map(|f| f.name.clone())
                .collect();
            self.event_triggered(etf, associated);
        }
    }

    /// `status_management`의 값(시그널 이름들) 중 LDF에 있거나 이 계획이 만드는 것.
    fn known_signals(&self, value: &str) -> Vec<String> {
        value
            .split(',')
            .map(str::trim)
            .filter(|s| {
                !s.is_empty()
                    && (self.new_signals.contains_key(*s) || self.m.find_signal(s).is_some())
            })
            .map(str::to_owned)
            .collect()
    }

    fn attrs(&mut self) -> AttrsDraft {
        let n = self.n;
        let proto = n
            .general
            .protocol_version
            .clone()
            .or_else(|| self.m.header.protocol_version.clone());
        // `LIN_protocol`이 2.0이면 `configurable_frames`에 메시지 ID를 쓰지 않는다(LIN 2.0 문법)
        let with_message_ids = proto.as_deref() != Some("2.0");
        let configured = n.diagnostic.nad.as_ref().and_then(|nad| match nad {
            Nad::List(v) => v.first().copied(),
            Nad::Range { from, .. } => Some(*from),
        });
        let version_has_initial_nad = proto
            .as_deref()
            .is_some_and(|p| p == "2.1" || p == "2.2" || p.starts_with("ISO"));
        if configured.is_some() && version_has_initial_nad {
            self.notes
                .push("initial_NAD를 configured_NAD와 같게 두었다".to_owned());
        }
        if matches!(&n.diagnostic.nad, Some(Nad::List(v)) if v.len() > 1)
            || matches!(&n.diagnostic.nad, Some(Nad::Range { .. }))
        {
            self.notes.push(
                "NCF가 NAD를 여러 개(또는 범위로) 허용해 첫 번째 값을 configured_NAD로 썼다"
                    .to_owned(),
            );
        }
        let status = |keys: &[&str]| {
            n.status_management
                .iter()
                .find(|i| keys.contains(&i.key.as_str()))
                .map(|i| i.value.clone())
        };
        let response_error = status(&["response_error", "error_response"])
            .and_then(|v| self.known_signals(&v).into_iter().next());
        let fault_state_signals = status(&["fault_state_signals"])
            .map(|v| self.known_signals(&v))
            .unwrap_or_default();
        AttrsDraft {
            lin_protocol: proto,
            configured_nad: configured,
            initial_nad: configured.filter(|_| version_has_initial_nad),
            product_id: match (n.general.supplier, n.general.function) {
                (Some(supplier), Some(function)) => Some(ProductId {
                    supplier,
                    function,
                    variant: n.general.variant,
                }),
                _ => None,
            },
            response_error,
            fault_state_signals,
            p2_min_ms: n.diagnostic.p2_min_ms,
            st_min_ms: n.diagnostic.st_min_ms,
            n_as_timeout_ms: n.diagnostic.n_as_timeout_ms,
            n_cr_timeout_ms: n.diagnostic.n_cr_timeout_ms,
            configurable_frames: n
                .frames
                .iter()
                .map(|f| (f.name.clone(), f.message_id.filter(|_| with_message_ids)))
                .collect(),
        }
    }

    /// 옮기지 않은 항목과 버전 차이를 알린다.
    fn leftovers(&mut self) {
        let n = self.n;
        let mut skipped: Vec<&str> = Vec::new();
        skipped.extend(n.general.extra.iter().map(|i| i.key.as_str()));
        skipped.extend(n.diagnostic.extra.iter().map(|i| i.key.as_str()));
        skipped.extend(n.extra.iter().map(|i| i.key.as_str()));
        if n.diagnostic.diagnostic_class.is_some() {
            skipped.push("diagnostic_class");
        }
        if n.diagnostic.max_message_length.is_some() {
            skipped.push("max_message_length");
        }
        if !n.diagnostic.support_sid.is_empty() {
            skipped.push("support_sid");
        }
        if n.general.sends_wake_up_signal.is_some() {
            skipped.push("sends_wake_up_signal");
        }
        if n.general.bitrate.is_some() {
            skipped.push("bitrate");
        }
        if !skipped.is_empty() {
            self.notes.push(format!(
                "LDF에 옮기지 않은 NCF 항목 {}개: {}",
                skipped.len(),
                skipped.join(", ")
            ));
        }
        if let (Some(ncf), Some(ldf)) = (
            n.general.protocol_version.as_deref(),
            self.m.header.protocol_version.as_deref(),
        ) && ncf != ldf
        {
            self.notes
                .push(format!("NCF 노드의 LIN 버전({ncf})이 LDF({ldf})와 다르다"));
        }
        self.notes.push("스케줄 테이블에는 넣지 않았다".to_owned());
    }
}

/// NCF의 노드 하나를 `m`에 슬레이브로 가져오는 계획을 만든다. 충돌이 있으면 그 사유들을 돌려준다.
pub fn plan_ncf_import(m: &LdfModel, n: &NcfNode) -> Result<NcfImport, Vec<String>> {
    let Some(master) = m
        .nodes
        .iter()
        .find(|(_, x)| x.role == NodeRole::Master)
        .map(|(_, x)| x.name.clone())
    else {
        return Err(vec![
            "LDF에 마스터 노드가 없어 NCF 노드를 가져올 수 없다".to_owned(),
        ]);
    };
    if m.find_node(&n.name).is_some() {
        return Err(vec![format!("노드 `{}`이(가) 이미 있다", n.name)]);
    }
    let mut p = Planner {
        m,
        n,
        master,
        ops: vec![EditOp::AddSlave(n.name.clone())],
        notes: Vec::new(),
        errs: Vec::new(),
        ids: IdPool::new(m),
        new_signals: BTreeMap::new(),
        frames: 0,
        signals: 0,
        encodings: 0,
    };
    p.encodings();
    p.frames();
    if !p.errs.is_empty() {
        return Err(p.errs);
    }
    let attrs = p.attrs();
    p.ops.push(EditOp::SetNodeAttrs {
        node: n.name.clone(),
        attrs,
    });
    p.leftovers();

    // 복사본에 적용해 보고 모델 규칙(이름·범위·중복)에 걸리는지 확인한다
    let op = EditOp::Batch(p.ops);
    let mut copy = m.clone();
    if let Err(e) = op.apply(&mut copy) {
        return Err(vec![e.to_string()]);
    }
    Ok(NcfImport {
        op,
        notes: p.notes,
        frames: p.frames,
        signals: p.signals,
        encodings: p.encodings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::load;
    use ldf_ncf::parse;

    const LDF: &str = "LIN_description_file;\n\
LIN_protocol_version = \"2.1\";\nLIN_language_version = \"2.1\";\nLIN_speed = 19.2 kbps;\n\
Nodes { Master: M, 5 ms, 0.1 ms; Slaves: Old; }\n\
Signals { OldSig: 8, 0, Old, M; }\n\
Frames { OldFrame: 0, Old, 1 { OldSig, 0; } }\n\
Schedule_tables { T { OldFrame delay 10 ms; } }\n";

    const NCF: &str = "node_capability_file;\nLIN_language_version = \"2.1\";\n\
node Motor {\n\
 general { LIN_protocol_version = \"2.1\"; supplier = 0x1e; function = 1; variant = 2; bitrate = 19.2 kbps; }\n\
 diagnostic { NAD = 5; P2_min = 50 ms; ST_min = 0 ms; }\n\
 frames {\n\
  subscribe Control { length = 2; signals { Speed { init_value = 3; size = 16; offset = 0; } } }\n\
  publish State { length = 2; event_triggered_frame = StateEvent; message_ID = 0x10; signals { Temp { init_value = 0; size = 8; offset = 8; enc; } Fault { init_value = 0; size = 1; offset = 0; } } }\n\
  publish StateEvent { length = 2; }\n\
 }\n\
 encoding { enc { physical_value, 0, 200, 0.5, -20, \"Degree\"; } }\n\
 status_management { response_error = Fault; }\n\
}\n";

    fn node(src: &str) -> NcfNode {
        parse(src).file.nodes.into_iter().next().expect("node")
    }

    fn applied(ldf: &str, ncf: &str) -> (LdfModel, NcfImport) {
        let m = load(ldf).model;
        let plan = plan_ncf_import(&m, &node(ncf)).expect("plan");
        let mut after = m.clone();
        plan.op.apply(&mut after).expect("apply");
        (after, plan)
    }

    #[test]
    fn a_publishing_and_subscribing_node_becomes_a_slave_with_frames_and_attributes() {
        let (m, plan) = applied(LDF, NCF);
        // 노드와 속성
        let motor = m.find_node("Motor").expect("Motor");
        assert_eq!(m.nodes.get(motor).map(|n| n.role), Some(NodeRole::Slave));
        let attrs = m
            .node_attrs
            .iter()
            .find(|a| a.node.id() == Some(motor))
            .expect("attrs");
        assert_eq!(attrs.configured_nad, Some(5));
        assert_eq!(attrs.initial_nad, Some(5));
        assert_eq!(attrs.lin_protocol.as_deref(), Some("2.1"));
        assert_eq!(
            attrs.product_id,
            Some(ProductId {
                supplier: 0x1e,
                function: 1,
                variant: Some(2)
            })
        );
        assert_eq!((attrs.p2_min_ms, attrs.st_min_ms), (Some(50.0), Some(0.0)));
        assert!(attrs.response_error.is_some());
        // 프레임: 발행은 Motor, 구독은 마스터가 발행
        let state = m
            .frames
            .get(m.find_frame("State").expect("State"))
            .expect("frame");
        let FrameKind::Unconditional {
            publisher,
            size,
            signals,
            ..
        } = &state.kind
        else {
            panic!("{:?}", state.kind);
        };
        assert_eq!((publisher.id(), *size, signals.len()), (Some(motor), 2, 2));
        let control = m
            .frames
            .get(m.find_frame("Control").expect("Control"))
            .expect("frame");
        let FrameKind::Unconditional { publisher, .. } = &control.kind else {
            panic!("{:?}", control.kind);
        };
        assert_eq!(publisher.id(), m.find_node("M"));
        // 이벤트 트리거 프레임은 State를 연관 프레임으로 갖는다
        let ev = m
            .frames
            .get(m.find_frame("StateEvent").expect("StateEvent"))
            .expect("frame");
        assert!(
            matches!(&ev.kind, FrameKind::EventTriggered { associated, .. } if associated.len() == 1)
        );
        // 새 프레임 ID는 비어 있는 낮은 번호부터(0은 OldFrame이 쓴다)
        let ids: Vec<u32> = ["Control", "State", "StateEvent"]
            .iter()
            .filter_map(|n| m.frames.get(m.find_frame(n)?)?.lin_id())
            .collect();
        assert_eq!(ids, [1, 2, 3]);
        // 인코딩과 시그널의 연결, 시그널의 발행·구독
        assert!(m.find_encoding("enc").is_some());
        let temp = m
            .signals
            .get(m.find_signal("Temp").expect("Temp"))
            .expect("signal");
        assert_eq!(temp.encodings.len(), 1);
        let speed = m
            .signals
            .get(m.find_signal("Speed").expect("Speed"))
            .expect("signal");
        assert_eq!(speed.publisher.as_ref().and_then(Ref::id), m.find_node("M"));
        assert_eq!(
            speed
                .subscribers
                .iter()
                .filter_map(Ref::id)
                .collect::<Vec<_>>(),
            [motor]
        );
        assert_eq!(speed.init, InitValue::Scalar(3));
        // 요약과 설명
        assert_eq!((plan.frames, plan.signals, plan.encodings), (3, 3, 1));
        assert!(
            plan.notes
                .iter()
                .any(|n| n.contains("스케줄 테이블에는 넣지"))
        );
        assert!(plan.notes.iter().any(|n| n.contains("bitrate")));
    }

    #[test]
    fn the_result_is_a_valid_ldf_that_survives_a_round_trip() {
        let (m, _) = applied(LDF, NCF);
        let text = ldf_writer::write(&m, &ldf_writer::WriteOptions::default()).text;
        let again = load(&text).model;
        assert!(ldf_model::semantically_equal(&m, &again), "{text}");
        assert!(text.contains("Slaves: Old, Motor;"), "{text}");
    }

    #[test]
    fn existing_frames_and_signals_are_reused_when_they_match() {
        let ldf = LDF
            .replace("Slaves: Old;", "Slaves: Old, Other;")
            .replace(
                "Signals { OldSig: 8, 0, Old, M; }",
                "Signals { OldSig: 8, 0, Old, M; Shared: 8, 0, M, Other; }",
            )
            .replace(
                "Frames { OldFrame: 0, Old, 1 { OldSig, 0; } }",
                "Frames { OldFrame: 0, Old, 1 { OldSig, 0; } Broadcast: 1, M, 1 { Shared, 0; } }",
            );
        let ncf = "node_capability_file; node Motor { general { LIN_protocol_version = \"2.1\"; supplier = 1; function = 1; }\n\
          frames { subscribe Broadcast { length = 1; signals { Shared { init_value = 0; size = 8; offset = 0; } } } } }";
        let (m, plan) = applied(&ldf, ncf);
        let shared = m
            .signals
            .get(m.find_signal("Shared").expect("Shared"))
            .expect("signal");
        let subs: Vec<_> = shared.subscribers.iter().filter_map(Ref::id).collect();
        assert_eq!(subs.len(), 2, "기존 구독자에 Motor가 더해진다");
        assert_eq!((plan.frames, plan.signals), (0, 0));
        assert!(
            plan.notes
                .iter()
                .any(|n| n.contains("`Shared`은(는) 이미 있어"))
        );
    }

    #[test]
    fn conflicts_are_reported_instead_of_applied() {
        let m = load(LDF).model;
        // 같은 이름의 노드
        let dup = node("node_capability_file; node Old { }");
        assert_eq!(plan_ncf_import(&m, &dup).expect_err("dup").len(), 1);
        // 이미 있는 프레임·시그널과 모양이 다르다
        let bad = node(
            "node_capability_file; node N { frames {\n\
             publish OldFrame { length = 1; signals { X { size = 8; offset = 0; } } }\n\
             subscribe Nope { length = 1; signals { OldSig { init_value = 0; size = 16; offset = 0; } } } } }",
        );
        let errs = plan_ncf_import(&m, &bad).expect_err("conflicts");
        assert!(
            errs.iter()
                .any(|e| e.contains("`OldFrame`이(가) 이미 있다")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("`OldSig`의 크기가")),
            "{errs:?}"
        );
        // 크기·길이 같은 필수 값이 없다
        let missing =
            node("node_capability_file; node N { frames { publish F { signals { S { } } } } }");
        let errs = plan_ncf_import(&m, &missing).expect_err("missing");
        assert!(errs.iter().any(|e| e.contains("length가 필요")), "{errs:?}");
        // 같은 이름의 다른 인코딩
        let enc =
            node("node_capability_file; node N { encoding { E { logical_value, 0, \"a\"; } } }");
        let m2 = load(&format!(
            "{LDF}Signal_encoding_types {{ E {{ logical_value, 0, \"b\"; }} }}"
        ))
        .model;
        let errs = plan_ncf_import(&m2, &enc).expect_err("encoding");
        assert!(
            errs.iter().any(|e| e.contains("내용이 NCF와 다르다")),
            "{errs:?}"
        );
        // 마스터가 없는 LDF
        let empty = LdfModel::default();
        assert!(plan_ncf_import(&empty, &node(NCF)).is_err());
    }

    #[test]
    fn frame_ids_run_out_gracefully_and_a_node_without_frames_still_imports() {
        let m = load(LDF).model;
        let (after, plan) = applied(
            LDF,
            "node_capability_file; node Quiet { general { LIN_protocol_version = \"2.1\"; supplier = 1; function = 2; } diagnostic { NAD = 1 to 3; } }",
        );
        assert!(after.find_node("Quiet").is_some());
        assert!(
            plan.notes.iter().any(|n| n.contains("첫 번째 값")),
            "{:?}",
            plan.notes
        );
        assert_eq!(m.nodes.len() + 1, after.nodes.len());
    }
}
