//! 편집 대화상자의 초안과 그 검증(egui 무관, 단위 테스트 대상).
//!
//! 대화상자는 초안 구조체를 들고 있다가 OK에서 `EditOp::Batch` 하나로 바꿔 보낸다(Undo 한 번에 대화상자 전체가 취소된다).
//! 초안은 [`preview`]로 모델 복사본에 미리 적용해 보고, 적용 실패와 이 변경으로 새로 생기는 검사 결과를 입력 중에 보여 준다.

use crate::view::Selection;
use ldf_model::{
    AttrsDraft, Diagnostic, EditOp, EncodingEntry, FrameDraft, FrameDraftKind, FrameId, FrameKind,
    InitValue, LdfModel, NodeId, NodeRole, ProductId, Ref, SignalDraft, SignalId, SignalKind,
    SlotDraft, SlotEntry,
};
use std::collections::BTreeMap;

/// 정수 입력(`12`, `0x3C`)을 읽는다.
pub fn parse_uint(s: &str) -> Option<u32> {
    let s = s.trim();
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => u32::from_str_radix(h, 16).ok(),
        None => s.parse().ok(),
    }
}

/// `1, 2, 0x3` 같은 목록 입력을 읽는다. 하나라도 정수가 아니면 `None`.
pub fn parse_list(s: &str) -> Option<Vec<u32>> {
    let s = s.trim();
    if s.is_empty() {
        return Some(Vec::new());
    }
    s.split(',').map(parse_uint).collect()
}

fn node_name(m: &LdfModel, r: &Ref<NodeId>) -> String {
    match r {
        Ref::Resolved(id) => m
            .nodes
            .get(*id)
            .map_or_else(String::new, |n| n.name.clone()),
        Ref::Unresolved(n) => n.clone(),
    }
}

fn signal_name(m: &LdfModel, id: SignalId) -> String {
    m.signals
        .get(id)
        .map_or_else(String::new, |s| s.name.clone())
}

fn frame_name(m: &LdfModel, r: &Ref<FrameId>) -> String {
    match r {
        Ref::Resolved(id) => m
            .frames
            .get(*id)
            .map_or_else(String::new, |f| f.name.clone()),
        Ref::Unresolved(n) => n.clone(),
    }
}

/// 마스터 노드 이름(없으면 첫 노드).
pub fn master_name(m: &LdfModel) -> String {
    m.nodes
        .iter()
        .find(|(_, n)| n.role == NodeRole::Master)
        .or_else(|| m.nodes.iter().next())
        .map(|(_, n)| n.name.clone())
        .unwrap_or_default()
}

// ---- 프레임 -------------------------------------------------------------------------------------

/// 프레임 종류별 입력.
#[derive(Clone, Debug, PartialEq)]
pub enum KindUi {
    /// 무조건 프레임.
    Unconditional {
        /// ID 입력(10진·16진).
        id: String,
        /// 발행 노드.
        publisher: String,
        /// 크기(byte).
        size: u32,
    },
    /// 스포라딕 프레임.
    Sporadic {
        /// 연관 프레임.
        associated: Vec<String>,
    },
    /// 이벤트 트리거 프레임.
    EventTriggered {
        /// ID 입력.
        id: String,
        /// 충돌 해결 테이블.
        collision: Option<String>,
        /// 연관 프레임.
        associated: Vec<String>,
    },
}

/// 새로 만들 시그널(프레임 대화상자에서 함께 만든다).
#[derive(Clone, Debug, PartialEq)]
pub struct NewSignalUi {
    /// 이름.
    pub name: String,
    /// 길이(bit).
    pub size: u32,
    /// 프레임 안 시작 비트.
    pub offset: u32,
}

/// 프레임 대화상자 초안.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameUi {
    /// 편집 대상의 원래 이름(새로 만들면 `None`).
    pub orig: Option<String>,
    /// 이름.
    pub name: String,
    /// 종류별 입력.
    pub kind: KindUi,
    /// 놓인 시그널(이름, 시작 비트).
    pub placements: Vec<(String, u32)>,
    /// 이 대화상자에서 새로 만들 시그널.
    pub added: Vec<NewSignalUi>,
    orig_placements: Vec<(String, u32)>,
}

impl FrameUi {
    /// 새 무조건 프레임: 고유한 이름, 비어 있는 ID, 마스터 발행.
    pub fn new_unconditional(m: &LdfModel) -> Self {
        Self::fresh(
            m,
            KindUi::Unconditional {
                id: m
                    .free_frame_id()
                    .map_or_else(String::new, |i| i.to_string()),
                publisher: master_name(m),
                size: 8,
            },
        )
    }

    /// 새 이벤트 트리거 프레임.
    pub fn new_event_triggered(m: &LdfModel) -> Self {
        Self::fresh(
            m,
            KindUi::EventTriggered {
                id: m
                    .free_frame_id()
                    .map_or_else(String::new, |i| i.to_string()),
                collision: None,
                associated: Vec::new(),
            },
        )
    }

    /// 새 스포라딕 프레임.
    pub fn new_sporadic(m: &LdfModel) -> Self {
        Self::fresh(
            m,
            KindUi::Sporadic {
                associated: Vec::new(),
            },
        )
    }

    fn fresh(m: &LdfModel, kind: KindUi) -> Self {
        Self {
            orig: None,
            name: m.unique_name("Frame"),
            kind,
            placements: Vec::new(),
            added: Vec::new(),
            orig_placements: Vec::new(),
        }
    }

    /// 기존 프레임 편집. 진단 프레임은 열리지 않는다.
    pub fn from_frame(m: &LdfModel, id: FrameId) -> Option<Self> {
        let f = m.frames.get(id)?;
        let (kind, placements) = match &f.kind {
            FrameKind::Unconditional {
                id: lin,
                publisher,
                size,
                signals,
            } => (
                KindUi::Unconditional {
                    id: lin.to_string(),
                    publisher: node_name(m, publisher),
                    size: *size,
                },
                signals
                    .iter()
                    .filter_map(|fs| fs.signal.id().map(|s| (signal_name(m, s), fs.offset)))
                    .collect::<Vec<_>>(),
            ),
            FrameKind::Sporadic { associated } => (
                KindUi::Sporadic {
                    associated: associated.iter().map(|r| frame_name(m, r)).collect(),
                },
                Vec::new(),
            ),
            FrameKind::EventTriggered {
                id: lin,
                collision_table,
                associated,
            } => (
                KindUi::EventTriggered {
                    id: lin.to_string(),
                    collision: collision_table
                        .as_ref()
                        .and_then(Ref::id)
                        .and_then(|t| m.schedules.get(t))
                        .map(|t| t.name.clone()),
                    associated: associated.iter().map(|r| frame_name(m, r)).collect(),
                },
                Vec::new(),
            ),
            FrameKind::Diagnostic { .. } => return None,
        };
        Some(Self {
            orig: Some(f.name.clone()),
            name: f.name.clone(),
            kind,
            orig_placements: placements.clone(),
            placements,
            added: Vec::new(),
        })
    }

    /// 입력값 자체의 문제(범위 등). 모델과의 관계는 [`preview`]가 본다.
    pub fn problems(&self) -> Vec<String> {
        let mut p = Vec::new();
        let id_text = match &self.kind {
            KindUi::Unconditional { id, size, .. } => {
                if !(1..=8).contains(size) {
                    p.push("크기는 1–8 byte여야 한다".to_owned());
                }
                Some(id)
            }
            KindUi::EventTriggered { id, .. } => Some(id),
            KindUi::Sporadic { .. } => None,
        };
        if let Some(id) = id_text {
            match parse_uint(id) {
                None => p.push("ID는 정수여야 한다".to_owned()),
                Some(v) if v > 59 => p.push("ID는 0–59여야 한다".to_owned()),
                Some(_) => {}
            }
        }
        p
    }

    /// 대화상자 OK에 보낼 연산.
    pub fn to_op(&self, m: &LdfModel) -> Result<EditOp, String> {
        let kind = match &self.kind {
            KindUi::Unconditional {
                id,
                publisher,
                size,
            } => FrameDraftKind::Unconditional {
                id: parse_uint(id).ok_or("ID는 정수여야 한다")?,
                publisher: publisher.clone(),
                size: *size,
            },
            KindUi::Sporadic { associated } => FrameDraftKind::Sporadic {
                associated: associated.clone(),
            },
            KindUi::EventTriggered {
                id,
                collision,
                associated,
            } => FrameDraftKind::EventTriggered {
                id: parse_uint(id).ok_or("ID는 정수여야 한다")?,
                collision_table: collision.clone(),
                associated: associated.clone(),
            },
        };
        let draft = FrameDraft {
            name: self.name.clone(),
            kind,
        };
        let mut ops = Vec::new();
        match &self.orig {
            Some(orig) => ops.push(EditOp::EditFrame {
                name: orig.clone(),
                draft,
            }),
            None => ops.push(EditOp::AddFrame(draft)),
        }
        // 새로 만들 시그널은 프레임 발행자가 발행하고 마스터(또는 다른 노드)가 구독한다
        for n in &self.added {
            ops.push(EditOp::AddSignal(SignalDraft {
                name: n.name.clone(),
                size_bits: n.size,
                init: InitValue::Scalar(0),
                publisher: match &self.kind {
                    KindUi::Unconditional { publisher, .. } => Some(publisher.clone()),
                    _ => None,
                },
                subscribers: self.default_subscribers(m),
                encoding: None,
            }));
        }
        // 놓인 시그널의 차이: 빠진 것, 옮긴 것, 새로 놓인 것
        for (s, _) in &self.orig_placements {
            if !self.placements.iter().any(|(n, _)| n == s) {
                ops.push(EditOp::UnplaceSignal {
                    frame: self.name_for_ops(),
                    signal: s.clone(),
                });
            }
        }
        for (s, off) in &self.placements {
            match self.orig_placements.iter().find(|(n, _)| n == s) {
                Some((_, old)) if old == off => {}
                Some(_) => ops.push(EditOp::MoveSignal {
                    frame: self.name_for_ops(),
                    signal: s.clone(),
                    offset: *off,
                }),
                None => ops.push(EditOp::PlaceSignal {
                    frame: self.name_for_ops(),
                    signal: s.clone(),
                    offset: *off,
                }),
            }
        }
        for n in &self.added {
            ops.push(EditOp::PlaceSignal {
                frame: self.name_for_ops(),
                signal: n.name.clone(),
                offset: n.offset,
            });
        }
        Ok(EditOp::Batch(ops))
    }

    /// `EditFrame`이 이름을 바꾼 뒤의 연산들이 가리킬 이름.
    fn name_for_ops(&self) -> String {
        self.name.clone()
    }

    /// 새 시그널의 기본 구독자: 발행자가 마스터면 첫 슬레이브, 아니면 마스터.
    fn default_subscribers(&self, m: &LdfModel) -> Vec<String> {
        let publisher = match &self.kind {
            KindUi::Unconditional { publisher, .. } => publisher.as_str(),
            _ => return Vec::new(),
        };
        let master = master_name(m);
        if publisher != master {
            return vec![master];
        }
        node_names(m, true).into_iter().take(1).collect()
    }
}

// ---- 시그널 -------------------------------------------------------------------------------------

/// 시그널 대화상자 초안.
#[derive(Clone, Debug, PartialEq)]
pub struct SignalUi {
    /// 편집 대상의 원래 이름.
    pub orig: Option<String>,
    /// 이름.
    pub name: String,
    /// 길이(bit).
    pub size: u32,
    /// 바이트 배열 여부.
    pub array: bool,
    /// 초기값 입력(스칼라는 정수, 배열은 쉼표 목록).
    pub init: String,
    /// 발행 노드(진단 시그널은 `None`).
    pub publisher: Option<String>,
    /// 구독 노드.
    pub subscribers: Vec<String>,
    /// 연결된 인코딩 타입.
    pub encoding: Option<String>,
}

impl SignalUi {
    /// 새 시그널.
    pub fn new(m: &LdfModel) -> Self {
        let publisher = m
            .nodes
            .iter()
            .find(|(_, n)| n.role == NodeRole::Slave)
            .map(|(_, n)| n.name.clone());
        Self {
            orig: None,
            name: m.unique_name("Signal"),
            size: 8,
            array: false,
            init: "0".to_owned(),
            publisher,
            subscribers: vec![master_name(m)],
            encoding: None,
        }
    }

    /// 기존 시그널 편집.
    pub fn from_signal(m: &LdfModel, id: SignalId) -> Option<Self> {
        let s = m.signals.get(id)?;
        let (array, init) = match &s.init {
            InitValue::Scalar(v) => (false, v.to_string()),
            InitValue::Array(b) => (
                true,
                b.iter().map(u8::to_string).collect::<Vec<_>>().join(", "),
            ),
        };
        Some(Self {
            orig: Some(s.name.clone()),
            name: s.name.clone(),
            size: s.size_bits,
            array,
            init,
            publisher: s.publisher.as_ref().map(|p| node_name(m, p)),
            subscribers: s.subscribers.iter().map(|r| node_name(m, r)).collect(),
            encoding: s
                .encodings
                .first()
                .and_then(Ref::id)
                .and_then(|e| m.encodings.get(e).map(|e| e.name.clone())),
        })
    }

    /// 입력값 자체의 문제.
    pub fn problems(&self) -> Vec<String> {
        let mut p = Vec::new();
        if self.array {
            if self.size == 0 || self.size > 64 || !self.size.is_multiple_of(8) {
                p.push("바이트 배열의 길이는 8의 배수로 8–64비트여야 한다".to_owned());
            }
            match parse_list(&self.init) {
                None => p.push("초기값은 쉼표로 나눈 정수여야 한다".to_owned()),
                Some(v) => {
                    if v.iter().any(|b| *b > 255) {
                        p.push("바이트는 0–255여야 한다".to_owned());
                    }
                    if v.len() as u32 * 8 != self.size {
                        p.push(format!(
                            "초기값 바이트가 {}개인데 길이는 {}바이트다",
                            v.len(),
                            self.size / 8
                        ));
                    }
                }
            }
        } else {
            if !(1..=16).contains(&self.size) {
                p.push("스칼라 길이는 1–16비트여야 한다".to_owned());
            }
            match parse_uint(&self.init) {
                None => p.push("초기값은 정수여야 한다".to_owned()),
                Some(v) if self.size < 32 && u64::from(v) >= 1u64 << self.size => {
                    p.push(format!(
                        "초기값 {v}을(를) {}비트로 표현할 수 없다",
                        self.size
                    ));
                }
                Some(_) => {}
            }
        }
        p
    }

    fn draft(&self) -> Result<SignalDraft, String> {
        let init = if self.array {
            let bytes = parse_list(&self.init).ok_or("초기값은 쉼표로 나눈 정수여야 한다")?;
            InitValue::Array(
                bytes
                    .into_iter()
                    .map(|b| u8::try_from(b).map_err(|_| "바이트는 0–255여야 한다"))
                    .collect::<Result<Vec<_>, _>>()?,
            )
        } else {
            InitValue::Scalar(u64::from(
                parse_uint(&self.init).ok_or("초기값은 정수여야 한다")?,
            ))
        };
        Ok(SignalDraft {
            name: self.name.clone(),
            size_bits: self.size,
            init,
            publisher: self.publisher.clone(),
            subscribers: self.subscribers.clone(),
            encoding: self.encoding.clone(),
        })
    }

    /// 대화상자 OK에 보낼 연산.
    pub fn to_op(&self) -> Result<EditOp, String> {
        let draft = self.draft()?;
        Ok(EditOp::Batch(vec![match &self.orig {
            Some(orig) => EditOp::EditSignal {
                name: orig.clone(),
                draft,
            },
            None => EditOp::AddSignal(draft),
        }]))
    }
}

// ---- 노드 ---------------------------------------------------------------------------------------

/// 슬레이브 노드 속성 입력.
#[derive(Clone, Debug, PartialEq)]
pub struct AttrsUi {
    /// ``LIN_protocol``.
    pub lin_protocol: String,
    /// ``configured_NAD`` 입력.
    pub configured_nad: String,
    /// ``initial_NAD`` 입력(비우면 없음).
    pub initial_nad: String,
    /// 공급자 ID 입력.
    pub supplier: String,
    /// 기능 ID 입력.
    pub function: String,
    /// 변형 ID 입력(비우면 없음).
    pub variant: String,
    /// ``response_error`` 시그널.
    pub response_error: Option<String>,
    /// ``fault_state_signals``.
    pub fault_state: Vec<String>,
    /// ``P2_min`` ms 입력.
    pub p2_min: String,
    /// ``ST_min`` ms 입력.
    pub st_min: String,
    /// ``N_As_timeout`` ms 입력(비우면 없음).
    pub n_as: String,
    /// ``N_Cr_timeout`` ms 입력(비우면 없음).
    pub n_cr: String,
    /// ``configurable_frames``: (프레임, 메시지 ID 입력).
    pub frames: Vec<(String, String)>,
}

fn opt_text<T: ToString>(v: Option<T>) -> String {
    v.map_or_else(String::new, |v| v.to_string())
}

fn ms_text(v: Option<f64>) -> String {
    v.map_or_else(String::new, |v| format!("{v}"))
}

impl AttrsUi {
    /// 기본값을 채운 새 속성(LIN 2.1 형식).
    pub fn blank(m: &LdfModel) -> Self {
        let version = m
            .header
            .protocol_version
            .clone()
            .unwrap_or_else(|| "2.1".to_owned());
        Self {
            lin_protocol: version,
            configured_nad: "1".to_owned(),
            initial_nad: String::new(),
            supplier: "1".to_owned(),
            function: "1".to_owned(),
            variant: "255".to_owned(),
            response_error: None,
            fault_state: Vec::new(),
            p2_min: "50".to_owned(),
            st_min: "0".to_owned(),
            n_as: String::new(),
            n_cr: String::new(),
            frames: Vec::new(),
        }
    }

    fn from_model(m: &LdfModel, node: NodeId) -> Option<Self> {
        let a = m.node_attrs.iter().find(|a| a.node.id() == Some(node))?;
        Some(Self {
            lin_protocol: a.lin_protocol.clone().unwrap_or_default(),
            configured_nad: opt_text(a.configured_nad),
            initial_nad: opt_text(a.initial_nad),
            supplier: a
                .product_id
                .as_ref()
                .map_or_else(String::new, |p| p.supplier.to_string()),
            function: a
                .product_id
                .as_ref()
                .map_or_else(String::new, |p| p.function.to_string()),
            variant: a
                .product_id
                .as_ref()
                .map_or_else(String::new, |p| opt_text(p.variant)),
            response_error: a
                .response_error
                .as_ref()
                .and_then(Ref::id)
                .map(|s| signal_name(m, s)),
            fault_state: a
                .fault_state_signals
                .iter()
                .filter_map(Ref::id)
                .map(|s| signal_name(m, s))
                .collect(),
            p2_min: ms_text(a.p2_min_ms),
            st_min: ms_text(a.st_min_ms),
            n_as: ms_text(a.n_as_timeout_ms),
            n_cr: ms_text(a.n_cr_timeout_ms),
            frames: a
                .configurable_frames
                .iter()
                .map(|c| (frame_name(m, &c.frame), opt_text(c.message_id)))
                .collect(),
        })
    }

    fn to_draft(&self) -> Result<AttrsDraft, String> {
        let uint = |s: &str, what: &str| -> Result<Option<u32>, String> {
            if s.trim().is_empty() {
                Ok(None)
            } else {
                parse_uint(s)
                    .map(Some)
                    .ok_or(format!("{what}은(는) 정수여야 한다"))
            }
        };
        let ms = |s: &str, what: &str| -> Result<Option<f64>, String> {
            if s.trim().is_empty() {
                Ok(None)
            } else {
                s.trim()
                    .parse()
                    .map(Some)
                    .map_err(|_| format!("{what}은(는) 숫자여야 한다"))
            }
        };
        let product_id = match (
            uint(&self.supplier, "공급자 ID")?,
            uint(&self.function, "기능 ID")?,
        ) {
            (Some(supplier), Some(function)) => Some(ProductId {
                supplier,
                function,
                variant: uint(&self.variant, "변형 ID")?,
            }),
            _ => None,
        };
        let mut frames = Vec::new();
        for (f, id) in &self.frames {
            frames.push((f.clone(), uint(id, "메시지 ID")?));
        }
        Ok(AttrsDraft {
            lin_protocol: (!self.lin_protocol.trim().is_empty()).then(|| self.lin_protocol.clone()),
            configured_nad: uint(&self.configured_nad, "configured_NAD")?,
            initial_nad: uint(&self.initial_nad, "initial_NAD")?,
            product_id,
            response_error: self.response_error.clone(),
            fault_state_signals: self.fault_state.clone(),
            p2_min_ms: ms(&self.p2_min, "P2_min")?,
            st_min_ms: ms(&self.st_min, "ST_min")?,
            n_as_timeout_ms: ms(&self.n_as, "N_As_timeout")?,
            n_cr_timeout_ms: ms(&self.n_cr, "N_Cr_timeout")?,
            configurable_frames: frames,
        })
    }
}

/// 노드 대화상자 초안.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeUi {
    /// 편집 대상의 원래 이름(새로 만들면 `None`).
    pub orig: Option<String>,
    /// 이름.
    pub name: String,
    /// 마스터 여부.
    pub master: bool,
    /// 마스터 타임베이스 입력(ms).
    pub timebase: String,
    /// 마스터 지터 입력(ms).
    pub jitter: String,
    /// 슬레이브 속성(편집하지 않으면 `None`).
    pub attrs: Option<AttrsUi>,
}

impl NodeUi {
    /// 새 슬레이브.
    pub fn new_slave(m: &LdfModel) -> Self {
        Self {
            orig: None,
            name: m.unique_name("Slave"),
            master: false,
            timebase: String::new(),
            jitter: String::new(),
            attrs: None,
        }
    }

    /// 기존 노드 편집.
    pub fn from_node(m: &LdfModel, id: NodeId) -> Option<Self> {
        let n = m.nodes.get(id)?;
        let master = n.role == NodeRole::Master;
        Some(Self {
            orig: Some(n.name.clone()),
            name: n.name.clone(),
            master,
            timebase: ms_text(n.timebase_ms),
            jitter: ms_text(n.jitter_ms),
            attrs: if master {
                None
            } else {
                AttrsUi::from_model(m, id)
            },
        })
    }

    /// 대화상자 OK에 보낼 연산.
    pub fn to_op(&self) -> Result<EditOp, String> {
        let mut ops = Vec::new();
        match &self.orig {
            None => ops.push(EditOp::AddSlave(self.name.clone())),
            Some(orig) if *orig != self.name => ops.push(EditOp::RenameNode {
                from: orig.clone(),
                to: self.name.clone(),
            }),
            Some(_) => {}
        }
        if self.master {
            let t: f64 = self
                .timebase
                .trim()
                .parse()
                .map_err(|_| "타임베이스는 숫자여야 한다")?;
            let j: f64 = self
                .jitter
                .trim()
                .parse()
                .map_err(|_| "지터는 숫자여야 한다")?;
            ops.push(EditOp::SetMasterTiming {
                node: self.name.clone(),
                timebase_ms: t,
                jitter_ms: j,
            });
        }
        if let Some(a) = &self.attrs {
            ops.push(EditOp::SetNodeAttrs {
                node: self.name.clone(),
                attrs: a.to_draft()?,
            });
        }
        Ok(EditOp::Batch(ops))
    }
}

// ---- 스케줄 -------------------------------------------------------------------------------------

/// 슬롯 종류(대화상자의 콤보 상자).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    /// 프레임.
    Frame,
    /// `AssignNAD`.
    AssignNad,
    /// `ConditionalChangeNAD`.
    ConditionalChangeNad,
    /// `DataDump`.
    DataDump,
    /// `SaveConfiguration`.
    SaveConfiguration,
    /// `AssignFrameIdRange`.
    AssignFrameIdRange,
    /// `AssignFrameId`.
    AssignFrameId,
    /// `FreeFormat`.
    FreeFormat,
}

impl SlotKind {
    /// 모든 종류(콤보 상자 순서).
    pub const ALL: [SlotKind; 8] = [
        SlotKind::Frame,
        SlotKind::AssignNad,
        SlotKind::ConditionalChangeNad,
        SlotKind::DataDump,
        SlotKind::SaveConfiguration,
        SlotKind::AssignFrameIdRange,
        SlotKind::AssignFrameId,
        SlotKind::FreeFormat,
    ];

    /// 표시 이름.
    pub fn label(self) -> &'static str {
        match self {
            SlotKind::Frame => "프레임",
            SlotKind::AssignNad => "AssignNAD",
            SlotKind::ConditionalChangeNad => "ConditionalChangeNAD",
            SlotKind::DataDump => "DataDump",
            SlotKind::SaveConfiguration => "SaveConfiguration",
            SlotKind::AssignFrameIdRange => "AssignFrameIdRange",
            SlotKind::AssignFrameId => "AssignFrameId",
            SlotKind::FreeFormat => "FreeFormat",
        }
    }

    /// 대상으로 노드를 받는지.
    pub fn takes_node(self) -> bool {
        matches!(
            self,
            SlotKind::AssignNad
                | SlotKind::DataDump
                | SlotKind::SaveConfiguration
                | SlotKind::AssignFrameIdRange
                | SlotKind::AssignFrameId
        )
    }

    /// 대상으로 프레임을 받는지.
    pub fn takes_frame(self) -> bool {
        matches!(self, SlotKind::Frame | SlotKind::AssignFrameId)
    }

    /// 쉼표로 나눈 숫자 인자를 받는지.
    pub fn takes_args(self) -> bool {
        matches!(
            self,
            SlotKind::ConditionalChangeNad
                | SlotKind::DataDump
                | SlotKind::AssignFrameIdRange
                | SlotKind::FreeFormat
        )
    }
}

/// 슬롯 한 줄 입력.
#[derive(Clone, Debug, PartialEq)]
pub struct SlotUi {
    /// 종류.
    pub kind: SlotKind,
    /// 대상 노드.
    pub node: String,
    /// 대상 프레임.
    pub frame: String,
    /// 숫자 인자 입력(`AssignFrameIdRange`는 첫 값이 시작 인덱스).
    pub args: String,
    /// delay(ms) 입력.
    pub delay: String,
}

impl SlotUi {
    /// 프레임 슬롯의 기본값.
    pub fn frame(frame: &str) -> Self {
        Self {
            kind: SlotKind::Frame,
            node: String::new(),
            frame: frame.to_owned(),
            args: String::new(),
            delay: "10".to_owned(),
        }
    }

    fn from_slot(m: &LdfModel, slot: &ldf_model::Slot) -> Self {
        let mut s = Self::frame("");
        s.delay = format!("{}", slot.delay_ms);
        match &slot.entry {
            SlotEntry::Frame(r) => s.frame = frame_name(m, r),
            SlotEntry::AssignNad { node } => {
                s.kind = SlotKind::AssignNad;
                s.node = node_name(m, node);
            }
            SlotEntry::ConditionalChangeNad { args } => {
                s.kind = SlotKind::ConditionalChangeNad;
                s.args = join(args);
            }
            SlotEntry::DataDump { node, data } => {
                s.kind = SlotKind::DataDump;
                s.node = node_name(m, node);
                s.args = join(data);
            }
            SlotEntry::SaveConfiguration { node } => {
                s.kind = SlotKind::SaveConfiguration;
                s.node = node_name(m, node);
            }
            SlotEntry::AssignFrameIdRange {
                node,
                start_index,
                pids,
            } => {
                s.kind = SlotKind::AssignFrameIdRange;
                s.node = node_name(m, node);
                let mut all = vec![*start_index];
                all.extend(pids);
                s.args = join(&all);
            }
            SlotEntry::AssignFrameId { node, frame } => {
                s.kind = SlotKind::AssignFrameId;
                s.node = node_name(m, node);
                s.frame = frame_name(m, frame);
            }
            SlotEntry::FreeFormat { data } => {
                s.kind = SlotKind::FreeFormat;
                s.args = join(data);
            }
            // 모델이 해석하지 못한 슬롯은 대화상자에서 다루지 않고 그대로 둔다(아래 `keep_unknown` 참고)
            SlotEntry::Unknown(_) => {}
        }
        s
    }

    fn draft(&self) -> Result<(SlotDraft, f64), String> {
        let delay: f64 = self
            .delay
            .trim()
            .parse()
            .map_err(|_| "delay는 숫자여야 한다")?;
        let args = || parse_list(&self.args).ok_or("인자는 쉼표로 나눈 정수여야 한다".to_owned());
        let entry = match self.kind {
            SlotKind::Frame => SlotDraft::Frame(self.frame.clone()),
            SlotKind::AssignNad => SlotDraft::AssignNad(self.node.clone()),
            SlotKind::ConditionalChangeNad => SlotDraft::ConditionalChangeNad(args()?),
            SlotKind::DataDump => SlotDraft::DataDump(self.node.clone(), args()?),
            SlotKind::SaveConfiguration => SlotDraft::SaveConfiguration(self.node.clone()),
            SlotKind::AssignFrameIdRange => {
                let all = args()?;
                let (start, pids) = all.split_first().ok_or("시작 인덱스가 필요하다")?;
                SlotDraft::AssignFrameIdRange(self.node.clone(), *start, pids.to_vec())
            }
            SlotKind::AssignFrameId => {
                SlotDraft::AssignFrameId(self.node.clone(), self.frame.clone())
            }
            SlotKind::FreeFormat => SlotDraft::FreeFormat(args()?),
        };
        Ok((entry, delay))
    }
}

fn join(v: &[u32]) -> String {
    v.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
}

/// 스케줄 대화상자 초안.
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduleUi {
    /// 편집 대상의 원래 이름.
    pub orig: Option<String>,
    /// 이름.
    pub name: String,
    /// 슬롯.
    pub slots: Vec<SlotUi>,
    /// 해석하지 못한 슬롯이 있었는지(있으면 슬롯 목록은 읽기 전용 경고만 한다).
    pub has_unknown: bool,
}

impl ScheduleUi {
    /// 새 테이블.
    pub fn new(m: &LdfModel) -> Self {
        Self {
            orig: None,
            name: m.unique_name("Table"),
            slots: Vec::new(),
            has_unknown: false,
        }
    }

    /// 기존 테이블 편집.
    pub fn from_schedule(m: &LdfModel, id: ldf_model::ScheduleId) -> Option<Self> {
        let t = m.schedules.get(id)?;
        Some(Self {
            orig: Some(t.name.clone()),
            name: t.name.clone(),
            has_unknown: t
                .slots
                .iter()
                .any(|s| matches!(s.entry, SlotEntry::Unknown(_))),
            slots: t.slots.iter().map(|s| SlotUi::from_slot(m, s)).collect(),
        })
    }

    /// 대화상자 OK에 보낼 연산. 기존 슬롯을 모두 지우고 입력대로 다시 넣는다(테이블의 ID와 참조는 유지된다).
    pub fn to_op(&self, m: &LdfModel) -> Result<EditOp, String> {
        if self.has_unknown {
            return Err("해석하지 못한 슬롯이 있어 이 테이블은 대화상자로 고칠 수 없다(Checker 창에서 텍스트로 고친다)".to_owned());
        }
        let mut ops = Vec::new();
        let table = match &self.orig {
            None => {
                ops.push(EditOp::AddSchedule(self.name.clone()));
                self.name.clone()
            }
            Some(orig) => {
                if let Some(id) = m.find_schedule(orig)
                    && let Some(t) = m.schedules.get(id)
                {
                    for i in (0..t.slots.len()).rev() {
                        ops.push(EditOp::RemoveSlot {
                            table: orig.clone(),
                            index: i,
                        });
                    }
                }
                if *orig != self.name {
                    ops.push(EditOp::RenameSchedule {
                        from: orig.clone(),
                        to: self.name.clone(),
                    });
                }
                self.name.clone()
            }
        };
        for s in &self.slots {
            let (entry, delay_ms) = s.draft()?;
            ops.push(EditOp::AddSlot {
                table: table.clone(),
                index: None,
                entry,
                delay_ms,
            });
        }
        Ok(EditOp::Batch(ops))
    }

    /// 슬롯별 경고(FR-EDT-05): delay가 타임베이스의 배수가 아니거나 프레임 최대 전송 시간보다 짧다.
    pub fn slot_warnings(&self, m: &LdfModel) -> Vec<Option<String>> {
        let timebase = m
            .nodes
            .iter()
            .find(|(_, n)| n.role == NodeRole::Master)
            .and_then(|(_, n)| n.timebase_ms);
        let speed = m.header.speed_bps.unwrap_or(0);
        self.slots
            .iter()
            .map(|s| {
                let delay: f64 = s.delay.trim().parse().ok()?;
                if let Some(tb) = timebase
                    && tb > 0.0
                {
                    let k = delay / tb;
                    if (k - k.round()).abs() > 1e-6 {
                        return Some(format!(
                            "delay {delay} ms가 타임베이스 {tb} ms의 배수가 아니다"
                        ));
                    }
                }
                if s.kind == SlotKind::Frame
                    && let Some(f) = m.find_frame(&s.frame).and_then(|id| m.frames.get(id))
                    && let Some(size) = f.size_bytes()
                    && let Some(tmax) = ldf_model::frame_time_max_ms(size, speed)
                    && delay + 1e-9 < tmax
                {
                    return Some(format!(
                        "delay {delay} ms가 최대 프레임 시간 {tmax:.2} ms보다 짧다"
                    ));
                }
                None
            })
            .collect()
    }
}

// ---- 인코딩 -------------------------------------------------------------------------------------

/// 인코딩 항목 한 줄 입력.
#[derive(Clone, Debug, PartialEq)]
pub enum EntryUi {
    /// 논리값: 값, 설명.
    Logical(String, String),
    /// 물리값: 최소, 최대, 스케일, 오프셋, 단위.
    Physical(String, String, String, String, String),
    /// BCD.
    Bcd,
    /// ASCII.
    Ascii,
}

/// 인코딩 대화상자 초안.
#[derive(Clone, Debug, PartialEq)]
pub struct EncodingUi {
    /// 편집 대상의 원래 이름.
    pub orig: Option<String>,
    /// 이름.
    pub name: String,
    /// 항목.
    pub entries: Vec<EntryUi>,
}

impl EncodingUi {
    /// 새 인코딩 타입.
    pub fn new(m: &LdfModel) -> Self {
        Self {
            orig: None,
            name: m.unique_name("Encoding"),
            entries: Vec::new(),
        }
    }

    /// 이름으로 기존 인코딩 편집.
    pub fn from_name(m: &LdfModel, name: &str) -> Option<Self> {
        let e = m.encodings.get(m.find_encoding(name)?)?;
        Some(Self {
            orig: Some(e.name.clone()),
            name: e.name.clone(),
            entries: e
                .entries
                .iter()
                .map(|x| match x {
                    EncodingEntry::Logical { value, text } => {
                        EntryUi::Logical(value.to_string(), text.clone())
                    }
                    EncodingEntry::Physical {
                        min,
                        max,
                        scale,
                        offset,
                        unit,
                    } => EntryUi::Physical(
                        min.to_string(),
                        max.to_string(),
                        scale.to_string(),
                        offset.to_string(),
                        unit.clone(),
                    ),
                    EncodingEntry::Bcd => EntryUi::Bcd,
                    EncodingEntry::Ascii => EntryUi::Ascii,
                })
                .collect(),
        })
    }

    /// 대화상자 OK에 보낼 연산.
    pub fn to_op(&self) -> Result<EditOp, String> {
        let mut entries = Vec::new();
        for e in &self.entries {
            entries.push(match e {
                EntryUi::Logical(v, t) => EncodingEntry::Logical {
                    value: v
                        .trim()
                        .parse()
                        .map_err(|_| "논리값은 0 이상의 정수여야 한다")?,
                    text: t.clone(),
                },
                EntryUi::Physical(min, max, scale, offset, unit) => EncodingEntry::Physical {
                    min: min.trim().parse().map_err(|_| "최솟값은 정수여야 한다")?,
                    max: max.trim().parse().map_err(|_| "최댓값은 정수여야 한다")?,
                    scale: scale.trim().parse().map_err(|_| "스케일은 숫자여야 한다")?,
                    offset: offset
                        .trim()
                        .parse()
                        .map_err(|_| "오프셋은 숫자여야 한다")?,
                    unit: unit.clone(),
                },
                EntryUi::Bcd => EncodingEntry::Bcd,
                EntryUi::Ascii => EncodingEntry::Ascii,
            });
        }
        Ok(EditOp::Batch(vec![match &self.orig {
            Some(orig) => EditOp::EditEncoding {
                name: orig.clone(),
                new_name: self.name.clone(),
                entries,
            },
            None => EditOp::AddEncoding {
                name: self.name.clone(),
                entries,
            },
        }]))
    }
}

// ---- 미리 적용과 삭제 영향 --------------------------------------------------------------------

/// 연산을 복사본에 적용해 본 결과.
#[derive(Debug)]
pub struct Preview {
    /// 적용한 모델(실패하면 `None`).
    pub model: Option<LdfModel>,
    /// 적용 실패 사유.
    pub error: Option<String>,
    /// 이 변경으로 새로 생기는 검사 결과.
    pub new_findings: Vec<Diagnostic>,
}

/// `op`를 `m`의 복사본에 적용하고, 이 변경으로 새로 생기는 검사 결과를 모은다.
pub fn preview(m: &LdfModel, op: &EditOp) -> Preview {
    let mut copy = m.clone();
    match op.apply(&mut copy) {
        Err(e) => Preview {
            model: None,
            error: Some(e.to_string()),
            new_findings: Vec::new(),
        },
        Ok(()) => {
            let mut before: BTreeMap<(u16, String), usize> = BTreeMap::new();
            for d in ldf_check::run_all(m) {
                *before.entry((d.code, d.message)).or_insert(0) += 1;
            }
            let mut new_findings = Vec::new();
            for d in ldf_check::run_all(&copy) {
                match before.get_mut(&(d.code, d.message.clone())) {
                    Some(n) if *n > 0 => *n -= 1,
                    _ => new_findings.push(d),
                }
            }
            Preview {
                model: Some(copy),
                error: None,
                new_findings,
            }
        }
    }
}

/// 삭제가 함께 지우거나 정리하는 항목을 사람이 읽을 문장으로 돌려준다(FR-EDT-06).
pub fn deletion_effects(before: &LdfModel, after: &LdfModel) -> Vec<String> {
    let removed = |a: Vec<String>, b: Vec<String>| -> Vec<String> {
        a.into_iter().filter(|x| !b.contains(x)).collect()
    };
    let names = |m: &LdfModel| {
        (
            m.nodes
                .iter()
                .map(|(_, n)| n.name.clone())
                .collect::<Vec<_>>(),
            m.signals
                .iter()
                .map(|(_, n)| n.name.clone())
                .collect::<Vec<_>>(),
            m.frames
                .iter()
                .map(|(_, n)| n.name.clone())
                .collect::<Vec<_>>(),
            m.encodings
                .iter()
                .map(|(_, n)| n.name.clone())
                .collect::<Vec<_>>(),
            m.schedules
                .iter()
                .map(|(_, n)| n.name.clone())
                .collect::<Vec<_>>(),
        )
    };
    let (bn, bs, bf, be, bt) = names(before);
    let (an, as_, af, ae, at) = names(after);
    let mut out = Vec::new();
    for (label, gone) in [
        ("노드", removed(bn, an)),
        ("시그널", removed(bs, as_)),
        ("프레임", removed(bf, af)),
        ("인코딩 타입", removed(be, ae)),
        ("스케줄 테이블", removed(bt, at)),
    ] {
        if !gone.is_empty() {
            out.push(format!(
                "{label} {}개 삭제: {}",
                gone.len(),
                gone.join(", ")
            ));
        }
    }
    let count = |m: &LdfModel| {
        let placements: usize = m.frames.iter().map(|(_, f)| f.signals().len()).sum();
        let slots: usize = m.schedules.iter().map(|(_, t)| t.slots.len()).sum();
        let configurable: usize = m
            .node_attrs
            .iter()
            .map(|a| a.configurable_frames.len())
            .sum();
        let assoc: usize = m
            .frames
            .iter()
            .map(|(_, f)| match &f.kind {
                FrameKind::Sporadic { associated }
                | FrameKind::EventTriggered { associated, .. } => associated.len(),
                _ => 0,
            })
            .sum();
        let attrs = m.node_attrs.len();
        let subs: usize = m.signals.iter().map(|(_, s)| s.subscribers.len()).sum();
        let sigs = m
            .signals
            .iter()
            .filter(|(_, s)| s.kind == SignalKind::Normal)
            .count();
        (placements, slots, configurable, assoc, attrs, subs, sigs)
    };
    let (b, a) = (count(before), count(after));
    for (label, was, now) in [
        ("프레임에 놓인 시그널", b.0, a.0),
        ("스케줄 슬롯", b.1, a.1),
        ("configurable_frames 항목", b.2, a.2),
        ("스포라딕·이벤트 트리거 연관 프레임 항목", b.3, a.3),
        ("노드 속성 블록", b.4, a.4),
    ] {
        if now < was {
            out.push(format!("{label} {}개 정리", was - now));
        }
    }
    out
}

/// 속성 창의 어느 표인지(같은 키 `Name`이 대상에 따라 달라지므로 구분한다).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineScope {
    /// 선택한 객체의 속성 표.
    Object,
    /// 스케줄 테이블·슬롯의 속성 표.
    Schedule,
}

/// 속성 창에서 값을 바로 고칠 수 있는 항목인지(FR-EDT-01).
pub fn inline_editable(scope: InlineScope, sel: &Selection, key: &str) -> bool {
    match (scope, sel) {
        (InlineScope::Object, Selection::Frame(_)) => {
            matches!(key, "Name" | "Frame ID" | "Size [Byte]")
        }
        (InlineScope::Object, Selection::Signal(_)) => {
            matches!(key, "Name" | "Length [Bit]" | "Init Value")
        }
        (InlineScope::Object, Selection::Node(_)) => {
            matches!(key, "Name" | "Timebase [ms]" | "Jitter [ms]")
        }
        (InlineScope::Schedule, Selection::Schedule(_)) => key == "Name",
        (InlineScope::Schedule, Selection::Slot(..)) => key == "Delay [ms]",
        _ => false,
    }
}

/// 속성 창에서 입력한 값을 연산으로 바꾼다. 형식이 틀리거나 범위 밖이거나 모델이 받아들이지 않으면 사유를 돌려준다.
/// 대화상자와 같은 초안·검증을 쓰므로 한 곳에서 고치든 다른 곳에서 고치든 규칙이 같다.
pub fn inline_edit(
    m: &LdfModel,
    scope: InlineScope,
    sel: &Selection,
    key: &str,
    text: &str,
) -> Result<EditOp, String> {
    let text = text.trim();
    let op = match (scope, sel) {
        (InlineScope::Object, Selection::Frame(id)) => {
            let mut ui = FrameUi::from_frame(m, *id).ok_or("이 프레임은 편집할 수 없다")?;
            match (key, &mut ui.kind) {
                ("Name", _) => ui.name = text.to_owned(),
                (
                    "Frame ID",
                    KindUi::Unconditional { id, .. } | KindUi::EventTriggered { id, .. },
                ) => {
                    *id = text.to_owned();
                }
                ("Size [Byte]", KindUi::Unconditional { size, .. }) => {
                    *size = parse_uint(text).ok_or("크기는 정수여야 한다")?;
                }
                _ => return Err("이 항목은 바로 고칠 수 없다".to_owned()),
            }
            if let Some(p) = ui.problems().into_iter().next() {
                return Err(p);
            }
            ui.to_op(m)?
        }
        (InlineScope::Object, Selection::Signal(id)) => {
            let mut ui = SignalUi::from_signal(m, *id).ok_or("시그널을 찾을 수 없다")?;
            match key {
                "Name" => ui.name = text.to_owned(),
                "Length [Bit]" => ui.size = parse_uint(text).ok_or("길이는 정수여야 한다")?,
                "Init Value" => ui.init = text.to_owned(),
                _ => return Err("이 항목은 바로 고칠 수 없다".to_owned()),
            }
            if let Some(p) = ui.problems().into_iter().next() {
                return Err(p);
            }
            ui.to_op()?
        }
        (InlineScope::Object, Selection::Node(id)) => {
            let mut ui = NodeUi::from_node(m, *id).ok_or("노드를 찾을 수 없다")?;
            match key {
                "Name" => ui.name = text.to_owned(),
                "Timebase [ms]" if ui.master => ui.timebase = text.to_owned(),
                "Jitter [ms]" if ui.master => ui.jitter = text.to_owned(),
                _ => return Err("이 항목은 바로 고칠 수 없다".to_owned()),
            }
            ui.to_op()?
        }
        (InlineScope::Schedule, Selection::Schedule(id)) if key == "Name" => {
            EditOp::RenameSchedule {
                from: m
                    .schedules
                    .get(*id)
                    .map(|t| t.name.clone())
                    .ok_or("테이블을 찾을 수 없다")?,
                to: text.to_owned(),
            }
        }
        (InlineScope::Schedule, Selection::Slot(id, index)) if key == "Delay [ms]" => {
            let delay_ms: f64 = text.parse().map_err(|_| "delay는 숫자여야 한다")?;
            if delay_ms < 0.0 {
                return Err("delay는 0 이상이어야 한다".to_owned());
            }
            EditOp::SetSlotDelay {
                table: m
                    .schedules
                    .get(*id)
                    .map(|t| t.name.clone())
                    .ok_or("테이블을 찾을 수 없다")?,
                index: *index,
                delay_ms,
            }
        }
        _ => return Err("이 항목은 바로 고칠 수 없다".to_owned()),
    };
    // 모델이 받아들이는지(이름 중복, 없는 대상 등)는 복사본에 적용해 본다
    match preview(m, &op).error {
        Some(e) => Err(e),
        None => Ok(op),
    }
}

/// 스케줄에 프레임을 새로 넣을 때 제안하는 delay(ms): 최대 프레임 시간 이상이면서 마스터 타임베이스의 배수이고, 최소 10 ms.
pub fn default_slot_delay(m: &LdfModel, frame: &str) -> f64 {
    let timebase = m
        .nodes
        .iter()
        .find(|(_, n)| n.role == NodeRole::Master)
        .and_then(|(_, n)| n.timebase_ms)
        .filter(|t| *t > 0.0);
    let tmax = m
        .find_frame(frame)
        .and_then(|id| m.frames.get(id))
        .and_then(|f| f.size_bytes())
        .and_then(|size| ldf_model::frame_time_max_ms(size, m.header.speed_bps.unwrap_or(0)))
        .unwrap_or(0.0);
    let want = tmax.max(10.0);
    match timebase {
        Some(tb) => (want / tb).ceil() * tb,
        None => want.ceil(),
    }
}

/// 내비게이션에서 프레임을 스케줄 표 위로 끌어다 놓았을 때의 연산. `slot`이 있으면 그 슬롯 바로 뒤에, 없으면(테이블 줄) 끝에 넣는다.
pub fn drop_frame_op(m: &LdfModel, table: &str, slot: Option<usize>, frame: &str) -> EditOp {
    EditOp::AddSlot {
        table: table.to_owned(),
        index: slot.map(|i| i + 1),
        entry: SlotDraft::Frame(frame.to_owned()),
        delay_ms: default_slot_delay(m, frame),
    }
}

/// 같은 테이블 안에서 슬롯을 끌어 `from`에서 `to`로 옮기는 연산. 제자리면 `None`.
pub fn move_slot_op(table: &str, from: usize, to: usize) -> Option<EditOp> {
    (from != to).then(|| EditOp::MoveSlot {
        table: table.to_owned(),
        from,
        to,
    })
}

/// 노드 이름 목록(슬레이브만 또는 전체).
pub fn node_names(m: &LdfModel, slaves_only: bool) -> Vec<String> {
    m.nodes
        .iter()
        .filter(|(_, n)| !slaves_only || n.role == NodeRole::Slave)
        .map(|(_, n)| n.name.clone())
        .collect()
}

/// 프레임 이름 목록. `uncond_only`면 무조건 프레임만.
pub fn frame_names(m: &LdfModel, uncond_only: bool) -> Vec<String> {
    m.frames
        .iter()
        .filter(|(_, f)| !uncond_only || matches!(f.kind, FrameKind::Unconditional { .. }))
        .map(|(_, f)| f.name.clone())
        .collect()
}

#[cfg(test)]
mod tests;
