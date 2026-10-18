//! 모델 편집 연산(FR-EDT).
//!
//! 연산은 객체를 **이름**으로 가리킨다. ID는 삭제할 때 당겨지고 다시 읽으면 달라질 수 있지만 이름은 변하지 않으므로,
//! 대화상자 하나의 결과를 `EditOp::Batch` 하나로 묶어도 안에서 방금 만든 객체를 이름으로 이어 쓸 수 있다.
//!
//! `apply`는 전부 적용하거나 전혀 적용하지 않는다(실패하면 모델은 그대로다).
//! Undo/Redo는 엔진이 편집 전후의 텍스트를 보관해 처리하므로 여기에는 역연산이 없다.

use crate::arena::ArenaId;
use crate::{
    ConfigurableFrame, EncodingEntry, EncodingId, EncodingType, Frame, FrameId, FrameKind,
    FrameSignal, InitValue, LdfModel, Node, NodeAttrs, NodeId, NodeRole, ProductId, Ref,
    ScheduleId, ScheduleTable, Signal, SignalId, SignalKind, Slot, SlotEntry,
};
use std::fmt;

/// 편집 실패. 모델은 바뀌지 않았다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    /// 이름의 객체가 없다.
    NotFound {
        /// 객체 종류(`"노드"` 등).
        kind: &'static str,
        /// 찾던 이름.
        name: String,
    },
    /// 같은 이름이 이미 있다.
    Duplicate {
        /// 객체 종류.
        kind: &'static str,
        /// 겹친 이름.
        name: String,
    },
    /// 값이 맞지 않다.
    Invalid(String),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::NotFound { kind, name } => write!(f, "{kind} `{name}`이(가) 없다"),
            EditError::Duplicate { kind, name } => write!(f, "{kind} `{name}`이(가) 이미 있다"),
            EditError::Invalid(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for EditError {}

type Result<T = ()> = std::result::Result<T, EditError>;

fn not_found<T>(kind: &'static str, name: &str) -> Result<T> {
    Err(EditError::NotFound {
        kind,
        name: name.to_owned(),
    })
}

fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(EditError::Invalid(msg.into()))
}

/// 시그널 초안(대화상자가 들고 있다가 OK에서 한 번에 보낸다).
#[derive(Clone, Debug, PartialEq)]
pub struct SignalDraft {
    /// 이름.
    pub name: String,
    /// 길이(bit).
    pub size_bits: u32,
    /// 초기값.
    pub init: InitValue,
    /// 발행 노드 이름(진단 시그널은 `None`).
    pub publisher: Option<String>,
    /// 구독 노드 이름들.
    pub subscribers: Vec<String>,
    /// 연결할 인코딩 타입 이름.
    pub encoding: Option<String>,
}

/// 프레임 종류별 초안.
#[derive(Clone, Debug, PartialEq)]
pub enum FrameDraftKind {
    /// 무조건 프레임.
    Unconditional {
        /// LIN 프레임 ID.
        id: u32,
        /// 발행 노드 이름.
        publisher: String,
        /// 크기(byte).
        size: u32,
    },
    /// 스포라딕 프레임.
    Sporadic {
        /// 연관 프레임 이름(우선순위 순).
        associated: Vec<String>,
    },
    /// 이벤트 트리거 프레임.
    EventTriggered {
        /// LIN 프레임 ID.
        id: u32,
        /// 충돌 해결 스케줄 테이블 이름.
        collision_table: Option<String>,
        /// 연관 프레임 이름.
        associated: Vec<String>,
    },
}

/// 프레임 초안. 배치된 시그널은 건드리지 않는다(`PlaceSignal` 등으로 따로 고친다).
#[derive(Clone, Debug, PartialEq)]
pub struct FrameDraft {
    /// 이름.
    pub name: String,
    /// 종류와 속성.
    pub kind: FrameDraftKind,
}

/// 스케줄 슬롯 내용 초안.
#[derive(Clone, Debug, PartialEq)]
pub enum SlotDraft {
    /// 프레임.
    Frame(String),
    /// `AssignNAD`.
    AssignNad(String),
    /// `ConditionalChangeNAD`.
    ConditionalChangeNad(Vec<u32>),
    /// `DataDump`.
    DataDump(String, Vec<u32>),
    /// `SaveConfiguration`.
    SaveConfiguration(String),
    /// `AssignFrameIdRange`.
    AssignFrameIdRange(String, u32, Vec<u32>),
    /// `AssignFrameId`.
    AssignFrameId(String, String),
    /// `FreeFormat`.
    FreeFormat(Vec<u32>),
}

/// 슬레이브 노드 속성 초안. 해석하지 않은 속성(`extra`)은 보존된다.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AttrsDraft {
    /// `LIN_protocol`.
    pub lin_protocol: Option<String>,
    /// `configured_NAD`.
    pub configured_nad: Option<u32>,
    /// `initial_NAD`.
    pub initial_nad: Option<u32>,
    /// `product_id`.
    pub product_id: Option<ProductId>,
    /// `response_error` 시그널 이름.
    pub response_error: Option<String>,
    /// `fault_state_signals` 이름들.
    pub fault_state_signals: Vec<String>,
    /// `P2_min` (ms).
    pub p2_min_ms: Option<f64>,
    /// `ST_min` (ms).
    pub st_min_ms: Option<f64>,
    /// `N_As_timeout` (ms).
    pub n_as_timeout_ms: Option<f64>,
    /// `N_Cr_timeout` (ms).
    pub n_cr_timeout_ms: Option<f64>,
    /// `configurable_frames`: (프레임 이름, 메시지 ID).
    pub configurable_frames: Vec<(String, Option<u32>)>,
}

/// 편집 연산.
#[derive(Clone, Debug, PartialEq)]
pub enum EditOp {
    /// 여러 연산을 하나로 묶는다(대화상자 OK 한 번). 하나라도 실패하면 전체가 취소된다.
    Batch(Vec<EditOp>),
    /// `LIN_speed`(bit/s).
    SetSpeed(u32),
    /// `Channel_name`.
    SetChannelName(Option<String>),
    /// 마스터의 타임베이스·지터(ms).
    SetMasterTiming {
        /// 마스터 이름.
        node: String,
        /// 타임베이스(ms).
        timebase_ms: f64,
        /// 지터(ms).
        jitter_ms: f64,
    },
    /// 슬레이브 노드를 만든다.
    AddSlave(String),
    /// 노드 이름을 바꾼다. 참조는 ID라서 자동으로 따라온다.
    RenameNode {
        /// 옛 이름.
        from: String,
        /// 새 이름.
        to: String,
    },
    /// 노드를 지운다. 그 노드가 발행하는 프레임·시그널, 노드 속성, 구독, 스케줄 명령도 함께 지운다.
    DeleteNode(String),
    /// 시그널을 만든다.
    AddSignal(SignalDraft),
    /// 시그널 속성을 바꾼다. 이름이 바뀌어도 프레임 배치·참조는 유지된다.
    EditSignal {
        /// 바꿀 시그널의 현재 이름.
        name: String,
        /// 새 속성.
        draft: SignalDraft,
    },
    /// 시그널을 지운다. 프레임 배치, `response_error`·`fault_state_signals` 참조도 함께 지운다.
    DeleteSignal(String),
    /// 프레임을 만든다.
    AddFrame(FrameDraft),
    /// 프레임 속성을 바꾼다(종류는 바꿀 수 없다).
    EditFrame {
        /// 바꿀 프레임의 현재 이름.
        name: String,
        /// 새 속성.
        draft: FrameDraft,
    },
    /// 프레임을 지운다. 연관 프레임 목록, 스케줄 슬롯, `configurable_frames` 항목도 함께 지운다.
    DeleteFrame(String),
    /// 무조건 프레임에 시그널을 놓는다.
    PlaceSignal {
        /// 프레임 이름.
        frame: String,
        /// 시그널 이름.
        signal: String,
        /// 시작 비트.
        offset: u32,
    },
    /// 프레임에서 시그널을 뺀다.
    UnplaceSignal {
        /// 프레임 이름.
        frame: String,
        /// 시그널 이름.
        signal: String,
    },
    /// 프레임에 놓인 시그널의 시작 비트를 바꾼다.
    MoveSignal {
        /// 프레임 이름.
        frame: String,
        /// 시그널 이름.
        signal: String,
        /// 새 시작 비트.
        offset: u32,
    },
    /// 인코딩 타입을 만든다.
    AddEncoding {
        /// 이름.
        name: String,
        /// 항목.
        entries: Vec<EncodingEntry>,
    },
    /// 인코딩 타입의 항목을 바꾼다(이름 변경 포함).
    EditEncoding {
        /// 현재 이름.
        name: String,
        /// 새 이름.
        new_name: String,
        /// 새 항목.
        entries: Vec<EncodingEntry>,
    },
    /// 인코딩 타입을 지운다. 시그널과의 연결도 끊는다.
    DeleteEncoding(String),
    /// 스케줄 테이블을 만든다.
    AddSchedule(String),
    /// 스케줄 테이블 이름을 바꾼다.
    RenameSchedule {
        /// 옛 이름.
        from: String,
        /// 새 이름.
        to: String,
    },
    /// 스케줄 테이블을 지운다. 이벤트 트리거 프레임의 충돌 해결 테이블 지정은 해제된다.
    DeleteSchedule(String),
    /// 스케줄에 슬롯을 넣는다(`index`가 `None`이면 끝에).
    AddSlot {
        /// 테이블 이름.
        table: String,
        /// 넣을 위치.
        index: Option<usize>,
        /// 내용.
        entry: SlotDraft,
        /// delay(ms).
        delay_ms: f64,
    },
    /// 슬롯을 지운다.
    RemoveSlot {
        /// 테이블 이름.
        table: String,
        /// 슬롯 위치.
        index: usize,
    },
    /// 슬롯의 위치를 바꾼다.
    MoveSlot {
        /// 테이블 이름.
        table: String,
        /// 옮길 슬롯 위치.
        from: usize,
        /// 옮겨 갈 위치.
        to: usize,
    },
    /// 슬롯의 delay를 바꾼다.
    SetSlotDelay {
        /// 테이블 이름.
        table: String,
        /// 슬롯 위치.
        index: usize,
        /// 새 delay(ms).
        delay_ms: f64,
    },
    /// 슬레이브의 노드 속성을 통째로 바꾼다(없으면 만든다).
    SetNodeAttrs {
        /// 노드 이름.
        node: String,
        /// 새 속성.
        attrs: AttrsDraft,
    },
}

// ---- 이름 찾기 ---------------------------------------------------------------------------------

impl LdfModel {
    /// 이름으로 노드를 찾는다.
    pub fn find_node(&self, name: &str) -> Option<NodeId> {
        self.nodes
            .iter()
            .find(|(_, n)| n.name == name)
            .map(|(i, _)| i)
    }

    /// 이름으로 시그널을 찾는다.
    pub fn find_signal(&self, name: &str) -> Option<SignalId> {
        self.signals
            .iter()
            .find(|(_, s)| s.name == name)
            .map(|(i, _)| i)
    }

    /// 이름으로 프레임을 찾는다.
    pub fn find_frame(&self, name: &str) -> Option<FrameId> {
        self.frames
            .iter()
            .find(|(_, f)| f.name == name)
            .map(|(i, _)| i)
    }

    /// 이름으로 인코딩 타입을 찾는다.
    pub fn find_encoding(&self, name: &str) -> Option<EncodingId> {
        self.encodings
            .iter()
            .find(|(_, e)| e.name == name)
            .map(|(i, _)| i)
    }

    /// 이름으로 스케줄 테이블을 찾는다.
    pub fn find_schedule(&self, name: &str) -> Option<ScheduleId> {
        self.schedules
            .iter()
            .find(|(_, s)| s.name == name)
            .map(|(i, _)| i)
    }

    /// 새 객체에 쓸 고유한 기본 이름(`Frame_1`, `Frame_2`, …).
    pub fn unique_name(&self, prefix: &str) -> String {
        let taken = |n: &str| {
            self.nodes.iter().any(|(_, x)| x.name == n)
                || self.signals.iter().any(|(_, x)| x.name == n)
                || self.frames.iter().any(|(_, x)| x.name == n)
                || self.encodings.iter().any(|(_, x)| x.name == n)
                || self.schedules.iter().any(|(_, x)| x.name == n)
        };
        (1u32..)
            .map(|i| format!("{prefix}_{i}"))
            .find(|n| !taken(n))
            .unwrap_or_else(|| prefix.to_owned())
    }

    /// 새 무조건 프레임에 제안할 비어 있는 LIN ID(0–59). 없으면 `None`.
    pub fn free_frame_id(&self) -> Option<u32> {
        let used: Vec<u32> = self
            .frames
            .iter()
            .filter(|(_, f)| {
                matches!(
                    f.kind,
                    FrameKind::Unconditional { .. } | FrameKind::EventTriggered { .. }
                )
            })
            .filter_map(|(_, f)| f.lin_id())
            .collect();
        (0..=59).find(|id| !used.contains(id))
    }

    fn node_ref(&self, name: &str) -> Ref<NodeId> {
        self.find_node(name)
            .map_or_else(|| Ref::Unresolved(name.to_owned()), Ref::Resolved)
    }
}

fn node_by_name(m: &LdfModel, name: &str) -> Result<NodeId> {
    m.find_node(name)
        .map_or_else(|| not_found("노드", name), Ok)
}

fn signal_by_name(m: &LdfModel, name: &str) -> Result<SignalId> {
    m.find_signal(name)
        .map_or_else(|| not_found("시그널", name), Ok)
}

fn frame_by_name(m: &LdfModel, name: &str) -> Result<FrameId> {
    m.find_frame(name)
        .map_or_else(|| not_found("프레임", name), Ok)
}

fn schedule_by_name(m: &LdfModel, name: &str) -> Result<ScheduleId> {
    m.find_schedule(name)
        .map_or_else(|| not_found("스케줄 테이블", name), Ok)
}

fn encoding_by_name(m: &LdfModel, name: &str) -> Result<EncodingId> {
    m.find_encoding(name)
        .map_or_else(|| not_found("인코딩 타입", name), Ok)
}

fn check_name(name: &str) -> Result {
    let ok = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ok {
        Ok(())
    } else {
        invalid(format!(
            "`{name}`은(는) 이름으로 쓸 수 없다(영문자·숫자·밑줄, 숫자로 시작 불가)"
        ))
    }
}

/// 이름이 어느 종류와도 겹치지 않는지. 같은 종류만 겹침을 막으면 되지만 LDF에서 이름은 한 공간처럼 쓰이므로 전체를 본다.
fn check_free(m: &LdfModel, kind: &'static str, name: &str, ignore: Option<&str>) -> Result {
    check_name(name)?;
    if Some(name) == ignore {
        return Ok(());
    }
    let clash = match kind {
        "노드" => m.find_node(name).is_some(),
        "시그널" => m.find_signal(name).is_some(),
        "프레임" => m.find_frame(name).is_some(),
        "인코딩 타입" => m.find_encoding(name).is_some(),
        _ => m.find_schedule(name).is_some(),
    };
    if clash {
        Err(EditError::Duplicate {
            kind,
            name: name.to_owned(),
        })
    } else {
        Ok(())
    }
}

// ---- 참조 방문 ---------------------------------------------------------------------------------

fn visit_node_refs(m: &mut LdfModel, f: &mut dyn FnMut(&mut Ref<NodeId>)) {
    for (_, s) in m.signals.iter_mut() {
        if let Some(p) = &mut s.publisher {
            f(p);
        }
        s.subscribers.iter_mut().for_each(&mut *f);
    }
    for (_, fr) in m.frames.iter_mut() {
        if let FrameKind::Unconditional { publisher, .. } = &mut fr.kind {
            f(publisher);
        }
    }
    for a in &mut m.node_attrs {
        f(&mut a.node);
    }
    for (_, t) in m.schedules.iter_mut() {
        for slot in &mut t.slots {
            match &mut slot.entry {
                SlotEntry::AssignNad { node }
                | SlotEntry::DataDump { node, .. }
                | SlotEntry::SaveConfiguration { node }
                | SlotEntry::AssignFrameIdRange { node, .. }
                | SlotEntry::AssignFrameId { node, .. } => f(node),
                _ => {}
            }
        }
    }
}

fn visit_signal_refs(m: &mut LdfModel, f: &mut dyn FnMut(&mut Ref<SignalId>)) {
    for (_, fr) in m.frames.iter_mut() {
        if let FrameKind::Unconditional { signals, .. } | FrameKind::Diagnostic { signals, .. } =
            &mut fr.kind
        {
            signals.iter_mut().for_each(|fs| f(&mut fs.signal));
        }
    }
    for a in &mut m.node_attrs {
        if let Some(r) = &mut a.response_error {
            f(r);
        }
        a.fault_state_signals.iter_mut().for_each(&mut *f);
    }
}

fn visit_frame_refs(m: &mut LdfModel, f: &mut dyn FnMut(&mut Ref<FrameId>)) {
    for (_, fr) in m.frames.iter_mut() {
        match &mut fr.kind {
            FrameKind::Sporadic { associated } | FrameKind::EventTriggered { associated, .. } => {
                associated.iter_mut().for_each(&mut *f);
            }
            _ => {}
        }
    }
    for (_, t) in m.schedules.iter_mut() {
        for slot in &mut t.slots {
            match &mut slot.entry {
                SlotEntry::Frame(r) | SlotEntry::AssignFrameId { frame: r, .. } => f(r),
                _ => {}
            }
        }
    }
    for a in &mut m.node_attrs {
        a.configurable_frames
            .iter_mut()
            .for_each(|c| f(&mut c.frame));
    }
}

fn visit_encoding_refs(m: &mut LdfModel, f: &mut dyn FnMut(&mut Ref<EncodingId>)) {
    for (_, s) in m.signals.iter_mut() {
        s.encodings.iter_mut().for_each(&mut *f);
    }
}

fn visit_schedule_refs(m: &mut LdfModel, f: &mut dyn FnMut(&mut Ref<ScheduleId>)) {
    for (_, fr) in m.frames.iter_mut() {
        if let FrameKind::EventTriggered {
            collision_table: Some(r),
            ..
        } = &mut fr.kind
        {
            f(r);
        }
    }
}

/// 인덱스 `removed`가 빠진 뒤 그보다 큰 ID를 하나씩 당긴다. `removed`를 가리키는 참조는 호출 전에 정리되어 있어야 한다.
fn shift<T: ArenaId>(removed: usize) -> impl FnMut(&mut Ref<T>) {
    move |r| {
        if let Ref::Resolved(id) = r
            && id.index() > removed
        {
            *id = T::from_index(id.index() - 1);
        }
    }
}

// ---- 삭제 ---------------------------------------------------------------------------------------

fn remove_signal(m: &mut LdfModel, id: SignalId) {
    // 이 시그널을 가리키는 배치와 속성을 먼저 정리한다
    for (_, fr) in m.frames.iter_mut() {
        if let FrameKind::Unconditional { signals, .. } | FrameKind::Diagnostic { signals, .. } =
            &mut fr.kind
        {
            signals.retain(|fs| fs.signal.id() != Some(id));
        }
    }
    for a in &mut m.node_attrs {
        if a.response_error.as_ref().and_then(Ref::id) == Some(id) {
            a.response_error = None;
        }
        a.fault_state_signals.retain(|r| r.id() != Some(id));
    }
    m.signals.remove(id);
    visit_signal_refs(m, &mut shift(id.index()));
}

fn remove_frame(m: &mut LdfModel, id: FrameId) {
    for (_, fr) in m.frames.iter_mut() {
        match &mut fr.kind {
            FrameKind::Sporadic { associated } | FrameKind::EventTriggered { associated, .. } => {
                associated.retain(|r| r.id() != Some(id));
            }
            _ => {}
        }
    }
    for (_, t) in m.schedules.iter_mut() {
        t.slots.retain(|s| match &s.entry {
            SlotEntry::Frame(r) | SlotEntry::AssignFrameId { frame: r, .. } => r.id() != Some(id),
            _ => true,
        });
    }
    for a in &mut m.node_attrs {
        a.configurable_frames.retain(|c| c.frame.id() != Some(id));
    }
    m.frames.remove(id);
    visit_frame_refs(m, &mut shift(id.index()));
}

fn remove_node(m: &mut LdfModel, id: NodeId) {
    // 그 노드가 발행하는 것들을 먼저 지운다(지우면 인덱스가 당겨지므로 이름으로 다시 찾는다)
    let frames: Vec<String> = m
        .frames
        .iter()
        .filter(|(_, f)| {
            matches!(&f.kind, FrameKind::Unconditional { publisher, .. } if publisher.id() == Some(id))
        })
        .map(|(_, f)| f.name.clone())
        .collect();
    for name in frames {
        if let Some(fid) = m.find_frame(&name) {
            remove_frame(m, fid);
        }
    }
    let signals: Vec<String> = m
        .signals
        .iter()
        .filter(|(_, s)| s.publisher.as_ref().and_then(Ref::id) == Some(id))
        .map(|(_, s)| s.name.clone())
        .collect();
    for name in signals {
        if let Some(sid) = m.find_signal(&name) {
            remove_signal(m, sid);
        }
    }
    for (_, s) in m.signals.iter_mut() {
        s.subscribers.retain(|r| r.id() != Some(id));
    }
    m.node_attrs.retain(|a| a.node.id() != Some(id));
    for (_, t) in m.schedules.iter_mut() {
        t.slots.retain(|s| match &s.entry {
            SlotEntry::AssignNad { node }
            | SlotEntry::DataDump { node, .. }
            | SlotEntry::SaveConfiguration { node }
            | SlotEntry::AssignFrameIdRange { node, .. }
            | SlotEntry::AssignFrameId { node, .. } => node.id() != Some(id),
            _ => true,
        });
    }
    m.nodes.remove(id);
    visit_node_refs(m, &mut shift(id.index()));
}

fn remove_encoding(m: &mut LdfModel, id: EncodingId) {
    for (_, s) in m.signals.iter_mut() {
        s.encodings.retain(|r| r.id() != Some(id));
    }
    m.encodings.remove(id);
    visit_encoding_refs(m, &mut shift(id.index()));
}

fn remove_schedule(m: &mut LdfModel, id: ScheduleId) {
    for (_, fr) in m.frames.iter_mut() {
        if let FrameKind::EventTriggered {
            collision_table, ..
        } = &mut fr.kind
            && collision_table.as_ref().and_then(Ref::id) == Some(id)
        {
            *collision_table = None;
        }
    }
    m.schedules.remove(id);
    visit_schedule_refs(m, &mut shift(id.index()));
}

// ---- 적용 ---------------------------------------------------------------------------------------

fn frame_refs(m: &LdfModel, names: &[String]) -> Result<Vec<Ref<FrameId>>> {
    names
        .iter()
        .map(|n| frame_by_name(m, n).map(Ref::Resolved))
        .collect()
}

fn apply_signal_draft(m: &mut LdfModel, id: Option<SignalId>, d: &SignalDraft) -> Result {
    let publisher = match &d.publisher {
        Some(p) => Some(Ref::Resolved(node_by_name(m, p)?)),
        None => None,
    };
    let subscribers = d
        .subscribers
        .iter()
        .map(|n| node_by_name(m, n).map(Ref::Resolved))
        .collect::<Result<Vec<_>>>()?;
    let encodings = match &d.encoding {
        Some(e) => vec![Ref::Resolved(encoding_by_name(m, e)?)],
        None => Vec::new(),
    };
    match id {
        Some(id) => {
            if let Some(s) = m.signals.get_mut(id) {
                s.name = d.name.clone();
                s.size_bits = d.size_bits;
                s.init = d.init.clone();
                s.kind = if publisher.is_some() {
                    SignalKind::Normal
                } else {
                    s.kind
                };
                s.publisher = publisher;
                s.subscribers = subscribers;
                s.encodings = encodings;
            }
        }
        None => {
            let kind = if publisher.is_some() {
                SignalKind::Normal
            } else {
                SignalKind::Diagnostic
            };
            m.signals.push(Signal {
                name: d.name.clone(),
                size_bits: d.size_bits,
                init: d.init.clone(),
                publisher,
                subscribers,
                encodings,
                kind,
                span: None,
            });
        }
    }
    Ok(())
}

fn slot_entry(m: &LdfModel, d: &SlotDraft) -> Result<SlotEntry> {
    Ok(match d {
        SlotDraft::Frame(n) => SlotEntry::Frame(Ref::Resolved(frame_by_name(m, n)?)),
        SlotDraft::AssignNad(n) => SlotEntry::AssignNad {
            node: Ref::Resolved(node_by_name(m, n)?),
        },
        SlotDraft::ConditionalChangeNad(args) => {
            SlotEntry::ConditionalChangeNad { args: args.clone() }
        }
        SlotDraft::DataDump(n, data) => SlotEntry::DataDump {
            node: Ref::Resolved(node_by_name(m, n)?),
            data: data.clone(),
        },
        SlotDraft::SaveConfiguration(n) => SlotEntry::SaveConfiguration {
            node: Ref::Resolved(node_by_name(m, n)?),
        },
        SlotDraft::AssignFrameIdRange(n, start, pids) => SlotEntry::AssignFrameIdRange {
            node: Ref::Resolved(node_by_name(m, n)?),
            start_index: *start,
            pids: pids.clone(),
        },
        SlotDraft::AssignFrameId(n, f) => SlotEntry::AssignFrameId {
            node: Ref::Resolved(node_by_name(m, n)?),
            frame: Ref::Resolved(frame_by_name(m, f)?),
        },
        SlotDraft::FreeFormat(data) => SlotEntry::FreeFormat { data: data.clone() },
    })
}

fn slots_mut<'a>(m: &'a mut LdfModel, table: &str) -> Result<&'a mut Vec<Slot>> {
    let id = schedule_by_name(m, table)?;
    m.schedules
        .get_mut(id)
        .map(|t| &mut t.slots)
        .map_or_else(|| not_found("스케줄 테이블", table), Ok)
}

fn unconditional_signals_mut<'a>(
    m: &'a mut LdfModel,
    frame: &str,
) -> Result<&'a mut Vec<FrameSignal>> {
    let id = frame_by_name(m, frame)?;
    match m.frames.get_mut(id).map(|f| &mut f.kind) {
        Some(FrameKind::Unconditional { signals, .. }) => Ok(signals),
        _ => invalid(format!(
            "프레임 `{frame}`은(는) 시그널을 담을 수 있는 무조건 프레임이 아니다"
        )),
    }
}

impl EditOp {
    /// 모델에 적용한다. 실패하면 모델은 그대로다.
    pub fn apply(&self, model: &mut LdfModel) -> std::result::Result<(), EditError> {
        let mut work = model.clone();
        self.apply_in_place(&mut work)?;
        *model = work;
        Ok(())
    }

    fn apply_in_place(&self, m: &mut LdfModel) -> Result {
        match self {
            EditOp::Batch(ops) => {
                for op in ops {
                    op.apply_in_place(m)?;
                }
            }
            EditOp::SetSpeed(bps) => m.header.speed_bps = Some(*bps),
            EditOp::SetChannelName(name) => m.header.channel_name = name.clone(),
            EditOp::SetMasterTiming {
                node,
                timebase_ms,
                jitter_ms,
            } => {
                let id = node_by_name(m, node)?;
                match m.nodes.get_mut(id) {
                    Some(n) if n.role == NodeRole::Master => {
                        n.timebase_ms = Some(*timebase_ms);
                        n.jitter_ms = Some(*jitter_ms);
                    }
                    _ => return invalid(format!("`{node}`은(는) 마스터가 아니다")),
                }
            }
            EditOp::AddSlave(name) => {
                check_free(m, "노드", name, None)?;
                m.nodes.push(Node {
                    name: name.clone(),
                    role: NodeRole::Slave,
                    timebase_ms: None,
                    jitter_ms: None,
                    max_header_bits: None,
                    response_tolerance_pct: None,
                    span: None,
                });
            }
            EditOp::RenameNode { from, to } => {
                let id = node_by_name(m, from)?;
                check_free(m, "노드", to, Some(from))?;
                if let Some(n) = m.nodes.get_mut(id) {
                    n.name = to.clone();
                }
            }
            EditOp::DeleteNode(name) => {
                let id = node_by_name(m, name)?;
                remove_node(m, id);
            }
            EditOp::AddSignal(d) => {
                check_free(m, "시그널", &d.name, None)?;
                apply_signal_draft(m, None, d)?;
            }
            EditOp::EditSignal { name, draft } => {
                let id = signal_by_name(m, name)?;
                check_free(m, "시그널", &draft.name, Some(name))?;
                apply_signal_draft(m, Some(id), draft)?;
            }
            EditOp::DeleteSignal(name) => {
                let id = signal_by_name(m, name)?;
                remove_signal(m, id);
            }
            EditOp::AddFrame(d) => {
                check_free(m, "프레임", &d.name, None)?;
                let kind = match &d.kind {
                    FrameDraftKind::Unconditional {
                        id,
                        publisher,
                        size,
                    } => FrameKind::Unconditional {
                        id: *id,
                        publisher: m.node_ref(publisher),
                        size: *size,
                        signals: Vec::new(),
                    },
                    FrameDraftKind::Sporadic { associated } => FrameKind::Sporadic {
                        associated: frame_refs(m, associated)?,
                    },
                    FrameDraftKind::EventTriggered {
                        id,
                        collision_table,
                        associated,
                    } => FrameKind::EventTriggered {
                        id: *id,
                        collision_table: match collision_table {
                            Some(t) => Some(Ref::Resolved(schedule_by_name(m, t)?)),
                            None => None,
                        },
                        associated: frame_refs(m, associated)?,
                    },
                };
                if let FrameKind::Unconditional { publisher, .. } = &kind
                    && publisher.id().is_none()
                {
                    return not_found("노드", publisher.unresolved_name().unwrap_or(""));
                }
                m.frames.push(Frame {
                    name: d.name.clone(),
                    kind,
                    span: None,
                });
            }
            EditOp::EditFrame { name, draft } => {
                let id = frame_by_name(m, name)?;
                check_free(m, "프레임", &draft.name, Some(name))?;
                let new_kind = match (&draft.kind, m.frames.get(id).map(|f| &f.kind)) {
                    (
                        FrameDraftKind::Unconditional {
                            id: lin,
                            publisher,
                            size,
                        },
                        Some(FrameKind::Unconditional { signals, .. }),
                    ) => FrameKind::Unconditional {
                        id: *lin,
                        publisher: Ref::Resolved(node_by_name(m, publisher)?),
                        size: *size,
                        signals: signals.clone(),
                    },
                    (FrameDraftKind::Sporadic { associated }, Some(FrameKind::Sporadic { .. })) => {
                        FrameKind::Sporadic {
                            associated: frame_refs(m, associated)?,
                        }
                    }
                    (
                        FrameDraftKind::EventTriggered {
                            id: lin,
                            collision_table,
                            associated,
                        },
                        Some(FrameKind::EventTriggered { .. }),
                    ) => FrameKind::EventTriggered {
                        id: *lin,
                        collision_table: match collision_table {
                            Some(t) => Some(Ref::Resolved(schedule_by_name(m, t)?)),
                            None => None,
                        },
                        associated: frame_refs(m, associated)?,
                    },
                    _ => return invalid(format!("프레임 `{name}`의 종류는 바꿀 수 없다")),
                };
                if let Some(f) = m.frames.get_mut(id) {
                    f.name = draft.name.clone();
                    f.kind = new_kind;
                }
            }
            EditOp::DeleteFrame(name) => {
                let id = frame_by_name(m, name)?;
                remove_frame(m, id);
            }
            EditOp::PlaceSignal {
                frame,
                signal,
                offset,
            } => {
                let sid = signal_by_name(m, signal)?;
                let signals = unconditional_signals_mut(m, frame)?;
                if signals.iter().any(|fs| fs.signal.id() == Some(sid)) {
                    return invalid(format!(
                        "시그널 `{signal}`은(는) 이미 프레임 `{frame}`에 있다"
                    ));
                }
                signals.push(FrameSignal {
                    signal: Ref::Resolved(sid),
                    offset: *offset,
                });
            }
            EditOp::UnplaceSignal { frame, signal } => {
                let sid = signal_by_name(m, signal)?;
                let signals = unconditional_signals_mut(m, frame)?;
                let before = signals.len();
                signals.retain(|fs| fs.signal.id() != Some(sid));
                if signals.len() == before {
                    return not_found("프레임 안의 시그널", signal);
                }
            }
            EditOp::MoveSignal {
                frame,
                signal,
                offset,
            } => {
                let sid = signal_by_name(m, signal)?;
                let signals = unconditional_signals_mut(m, frame)?;
                match signals.iter_mut().find(|fs| fs.signal.id() == Some(sid)) {
                    Some(fs) => fs.offset = *offset,
                    None => return not_found("프레임 안의 시그널", signal),
                }
            }
            EditOp::AddEncoding { name, entries } => {
                check_free(m, "인코딩 타입", name, None)?;
                m.encodings.push(EncodingType {
                    name: name.clone(),
                    entries: entries.clone(),
                    span: None,
                });
            }
            EditOp::EditEncoding {
                name,
                new_name,
                entries,
            } => {
                let id = encoding_by_name(m, name)?;
                check_free(m, "인코딩 타입", new_name, Some(name))?;
                if let Some(e) = m.encodings.get_mut(id) {
                    e.name = new_name.clone();
                    e.entries = entries.clone();
                }
            }
            EditOp::DeleteEncoding(name) => {
                let id = encoding_by_name(m, name)?;
                remove_encoding(m, id);
            }
            EditOp::AddSchedule(name) => {
                check_free(m, "스케줄 테이블", name, None)?;
                m.schedules.push(ScheduleTable {
                    name: name.clone(),
                    slots: Vec::new(),
                    span: None,
                });
            }
            EditOp::RenameSchedule { from, to } => {
                let id = schedule_by_name(m, from)?;
                check_free(m, "스케줄 테이블", to, Some(from))?;
                if let Some(t) = m.schedules.get_mut(id) {
                    t.name = to.clone();
                }
            }
            EditOp::DeleteSchedule(name) => {
                let id = schedule_by_name(m, name)?;
                remove_schedule(m, id);
            }
            EditOp::AddSlot {
                table,
                index,
                entry,
                delay_ms,
            } => {
                let entry = slot_entry(m, entry)?;
                let slots = slots_mut(m, table)?;
                let at = index.unwrap_or(slots.len());
                if at > slots.len() {
                    return invalid(format!("슬롯 위치 {at}이(가) 범위를 벗어났다"));
                }
                slots.insert(
                    at,
                    Slot {
                        entry,
                        delay_ms: *delay_ms,
                        span: None,
                    },
                );
            }
            EditOp::RemoveSlot { table, index } => {
                let slots = slots_mut(m, table)?;
                if *index >= slots.len() {
                    return invalid(format!("슬롯 위치 {index}이(가) 범위를 벗어났다"));
                }
                slots.remove(*index);
            }
            EditOp::MoveSlot { table, from, to } => {
                let slots = slots_mut(m, table)?;
                if *from >= slots.len() || *to >= slots.len() {
                    return invalid("슬롯 위치가 범위를 벗어났다");
                }
                let s = slots.remove(*from);
                slots.insert(*to, s);
            }
            EditOp::SetSlotDelay {
                table,
                index,
                delay_ms,
            } => {
                let slots = slots_mut(m, table)?;
                match slots.get_mut(*index) {
                    Some(s) => s.delay_ms = *delay_ms,
                    None => return invalid(format!("슬롯 위치 {index}이(가) 범위를 벗어났다")),
                }
            }
            EditOp::SetNodeAttrs { node, attrs } => {
                let nid = node_by_name(m, node)?;
                if m.nodes.get(nid).is_some_and(|n| n.role != NodeRole::Slave) {
                    return invalid(format!(
                        "`{node}`은(는) 슬레이브가 아니라 노드 속성을 둘 수 없다"
                    ));
                }
                let response_error = match &attrs.response_error {
                    Some(n) => Some(Ref::Resolved(signal_by_name(m, n)?)),
                    None => None,
                };
                let fault_state_signals = attrs
                    .fault_state_signals
                    .iter()
                    .map(|n| signal_by_name(m, n).map(Ref::Resolved))
                    .collect::<Result<Vec<_>>>()?;
                let configurable_frames = attrs
                    .configurable_frames
                    .iter()
                    .map(|(n, id)| {
                        frame_by_name(m, n).map(|f| ConfigurableFrame {
                            frame: Ref::Resolved(f),
                            message_id: *id,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let fresh = NodeAttrs {
                    node: Ref::Resolved(nid),
                    lin_protocol: attrs.lin_protocol.clone(),
                    configured_nad: attrs.configured_nad,
                    initial_nad: attrs.initial_nad,
                    product_id: attrs.product_id.clone(),
                    response_error,
                    fault_state_signals,
                    p2_min_ms: attrs.p2_min_ms,
                    st_min_ms: attrs.st_min_ms,
                    n_as_timeout_ms: attrs.n_as_timeout_ms,
                    n_cr_timeout_ms: attrs.n_cr_timeout_ms,
                    configurable_frames,
                    extra: Vec::new(),
                    span: None,
                };
                match m.node_attrs.iter_mut().find(|a| a.node.id() == Some(nid)) {
                    Some(old) => {
                        let (extra, span) = (std::mem::take(&mut old.extra), old.span);
                        *old = NodeAttrs {
                            extra,
                            span,
                            ..fresh
                        };
                    }
                    None => m.node_attrs.push(fresh),
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
