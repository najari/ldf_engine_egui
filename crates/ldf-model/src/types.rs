//! 도메인 타입. 참조는 `Ref`로 표현하며, 못 찾은 참조는 `Unresolved(이름)`으로 남는다.

use crate::arena::{Arena, EncodingId, FrameId, NodeId, ScheduleId, SignalId};
use ldf_syntax::Span;
use serde::{Deserialize, Serialize};

/// 다른 객체에 대한 참조.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Ref<T> {
    /// 해석된 참조.
    Resolved(T),
    /// 이름을 못 찾은 참조(원래 이름을 보관해 왕복 보존한다).
    Unresolved(String),
}

impl<T: Copy> Ref<T> {
    /// 해석된 ID.
    pub fn id(&self) -> Option<T> {
        match self {
            Ref::Resolved(id) => Some(*id),
            Ref::Unresolved(_) => None,
        }
    }
}

impl<T> Ref<T> {
    /// 해석되지 않은 참조의 이름.
    pub fn unresolved_name(&self) -> Option<&str> {
        match self {
            Ref::Unresolved(n) => Some(n),
            Ref::Resolved(_) => None,
        }
    }
}

/// LDF 헤더.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Header {
    /// `LIN_description_file;` 선언이 있었는지.
    pub has_description_marker: bool,
    /// `LIN_protocol_version` (`"2.2"`, `"J2602_1_1.0"`, `"ISO17987:2015"` 등).
    pub protocol_version: Option<String>,
    /// `LIN_language_version`.
    pub language_version: Option<String>,
    /// `LIN_speed`를 bit/s로 환산한 값(19.2 kbps → 19200).
    pub speed_bps: Option<u32>,
    /// `Channel_name` (LIN 2.1+).
    pub channel_name: Option<String>,
    /// `LDF_file_revision` (ISO 17987).
    pub ldf_file_revision: Option<String>,
    /// `LIN_sig_byte_order_big_endian;` 선언이 있었는지 (ISO 17987 Motorola 바이트 순서).
    pub sig_byte_order_big_endian: bool,
    /// 원본 문자 인코딩 이름(`utf-8`, `windows-1252`). 엔진이 채운다.
    pub encoding: String,
    /// 첫 문장 앞의 주석 블록(파일 머리말). 줄바꿈은 `\n`이고 앞뒤 빈 줄은 없다. 출력할 때 맨 앞에 되살린다.
    #[serde(default)]
    pub leading_comment: String,
}

/// 노드 역할.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeRole {
    /// 마스터.
    Master,
    /// 슬레이브.
    Slave,
}

/// 노드.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// 이름.
    pub name: String,
    /// 역할.
    pub role: NodeRole,
    /// 마스터의 타임베이스(ms).
    pub timebase_ms: Option<f64>,
    /// 마스터의 지터(ms).
    pub jitter_ms: Option<f64>,
    /// SAE J2602 마스터의 최대 헤더 길이(bit).
    pub max_header_bits: Option<u32>,
    /// SAE J2602 마스터의 응답 허용 오차(%).
    pub response_tolerance_pct: Option<f64>,
    /// 원문 위치.
    pub span: Option<Span>,
}

/// 구성 가능한 프레임(`configurable_frames`) 항목.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConfigurableFrame {
    /// 프레임.
    pub frame: Ref<FrameId>,
    /// LIN 2.0의 `= 0x0001` 메시지 ID.
    pub message_id: Option<u32>,
}

/// 제품 ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductId {
    /// 공급자 ID.
    pub supplier: u32,
    /// 기능 ID.
    pub function: u32,
    /// 변형 ID(생략 가능).
    pub variant: Option<u32>,
}

/// 모델이 해석하지 않는 속성(왕복 보존용).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawAttr {
    /// 키.
    pub key: String,
    /// 원문 텍스트.
    pub text: String,
}

/// 슬레이브 노드 속성(`Node_attributes`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeAttrs {
    /// 대상 노드.
    pub node: Ref<NodeId>,
    /// `LIN_protocol`.
    pub lin_protocol: Option<String>,
    /// `configured_NAD`.
    pub configured_nad: Option<u32>,
    /// `initial_NAD`.
    pub initial_nad: Option<u32>,
    /// `product_id`.
    pub product_id: Option<ProductId>,
    /// `response_error` 시그널.
    pub response_error: Option<Ref<SignalId>>,
    /// `fault_state_signals`.
    pub fault_state_signals: Vec<Ref<SignalId>>,
    /// `P2_min` (ms).
    pub p2_min_ms: Option<f64>,
    /// `ST_min` (ms).
    pub st_min_ms: Option<f64>,
    /// `N_As_timeout` (ms).
    pub n_as_timeout_ms: Option<f64>,
    /// `N_Cr_timeout` (ms).
    pub n_cr_timeout_ms: Option<f64>,
    /// `configurable_frames`.
    pub configurable_frames: Vec<ConfigurableFrame>,
    /// 해석하지 않은 속성.
    pub extra: Vec<RawAttr>,
    /// 원문 위치.
    pub span: Option<Span>,
}

/// 시그널 초기값.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InitValue {
    /// 스칼라.
    Scalar(u64),
    /// 바이트 배열(`{0, 0, 0, 0}`).
    Array(Vec<u8>),
}

/// 시그널 종류.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalKind {
    /// 일반 시그널.
    Normal,
    /// 진단 시그널(`Diagnostic_signals`).
    Diagnostic,
}

/// 시그널.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    /// 이름.
    pub name: String,
    /// 길이(bit). 범위 검사는 `ldf-check`가 한다.
    pub size_bits: u32,
    /// 초기값.
    pub init: InitValue,
    /// 발행자(진단 시그널은 없음).
    pub publisher: Option<Ref<NodeId>>,
    /// 구독자.
    pub subscribers: Vec<Ref<NodeId>>,
    /// `Signal_representation`으로 연결된 인코딩. 둘 이상이면 모델링 오류(0206).
    pub encodings: Vec<Ref<EncodingId>>,
    /// 종류.
    pub kind: SignalKind,
    /// 원문 위치. 편집으로 생긴 객체는 `None`.
    pub span: Option<Span>,
}

impl Signal {
    /// 대표 인코딩(첫 번째).
    pub fn encoding(&self) -> Option<&Ref<EncodingId>> {
        self.encodings.first()
    }
}

/// 프레임에 배치된 시그널.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameSignal {
    /// 시그널.
    pub signal: Ref<SignalId>,
    /// 시작 비트.
    pub offset: u32,
}

/// 프레임 종류별 내용.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FrameKind {
    /// 무조건 프레임.
    Unconditional {
        /// LIN 프레임 ID.
        id: u32,
        /// 발행 노드.
        publisher: Ref<NodeId>,
        /// 크기(byte).
        size: u32,
        /// 배치된 시그널.
        signals: Vec<FrameSignal>,
    },
    /// 스포라딕 프레임.
    Sporadic {
        /// 연관 프레임(우선순위 순).
        associated: Vec<Ref<FrameId>>,
    },
    /// 이벤트 트리거 프레임.
    EventTriggered {
        /// LIN 프레임 ID.
        id: u32,
        /// 충돌 해결 스케줄 테이블.
        collision_table: Option<Ref<ScheduleId>>,
        /// 연관 프레임.
        associated: Vec<Ref<FrameId>>,
    },
    /// 진단 프레임(`MasterReq` 0x3C, `SlaveResp` 0x3D).
    Diagnostic {
        /// LIN 프레임 ID.
        id: u32,
        /// 배치된 시그널.
        signals: Vec<FrameSignal>,
    },
}

/// 프레임.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// 이름.
    pub name: String,
    /// 종류와 내용.
    pub kind: FrameKind,
    /// 원문 위치.
    pub span: Option<Span>,
}

impl Frame {
    /// LIN 프레임 ID(스포라딕은 없음).
    pub fn lin_id(&self) -> Option<u32> {
        match &self.kind {
            FrameKind::Unconditional { id, .. }
            | FrameKind::EventTriggered { id, .. }
            | FrameKind::Diagnostic { id, .. } => Some(*id),
            FrameKind::Sporadic { .. } => None,
        }
    }

    /// 프레임 크기(byte). 무조건 프레임과 진단 프레임(8)만 갖는다.
    pub fn size_bytes(&self) -> Option<u32> {
        match &self.kind {
            FrameKind::Unconditional { size, .. } => Some(*size),
            FrameKind::Diagnostic { .. } => Some(8),
            _ => None,
        }
    }

    /// 프레임에 배치된 시그널.
    pub fn signals(&self) -> &[FrameSignal] {
        match &self.kind {
            FrameKind::Unconditional { signals, .. } | FrameKind::Diagnostic { signals, .. } => {
                signals
            }
            _ => &[],
        }
    }
}

/// 인코딩 항목.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EncodingEntry {
    /// 논리값: 값 -> 텍스트.
    Logical {
        /// 값.
        value: u64,
        /// 설명 텍스트.
        text: String,
    },
    /// 물리값 범위.
    Physical {
        /// 최소 raw 값.
        min: i64,
        /// 최대 raw 값.
        max: i64,
        /// 스케일.
        scale: f64,
        /// 오프셋.
        offset: f64,
        /// 단위.
        unit: String,
    },
    /// BCD 값.
    Bcd,
    /// ASCII 값.
    Ascii,
}

/// 인코딩 타입.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EncodingType {
    /// 이름.
    pub name: String,
    /// 항목(원문 순서).
    pub entries: Vec<EncodingEntry>,
    /// 원문 위치.
    pub span: Option<Span>,
}

/// 스케줄 슬롯의 내용.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SlotEntry {
    /// 프레임(`MasterReq`/`SlaveResp` 포함).
    Frame(Ref<FrameId>),
    /// `AssignNAD`.
    AssignNad {
        /// 대상 노드.
        node: Ref<NodeId>,
    },
    /// `ConditionalChangeNAD`.
    ConditionalChangeNad {
        /// 인자 6개: nad, id, byte, mask, inv, new_nad.
        args: Vec<u32>,
    },
    /// `DataDump`.
    DataDump {
        /// 대상 노드.
        node: Ref<NodeId>,
        /// 데이터 5바이트.
        data: Vec<u32>,
    },
    /// `SaveConfiguration`.
    SaveConfiguration {
        /// 대상 노드.
        node: Ref<NodeId>,
    },
    /// `AssignFrameIdRange`.
    AssignFrameIdRange {
        /// 대상 노드.
        node: Ref<NodeId>,
        /// 시작 인덱스.
        start_index: u32,
        /// PID 0–4개.
        pids: Vec<u32>,
    },
    /// `AssignFrameId`.
    AssignFrameId {
        /// 대상 노드.
        node: Ref<NodeId>,
        /// 대상 프레임.
        frame: Ref<FrameId>,
    },
    /// `FreeFormat`.
    FreeFormat {
        /// 데이터 8바이트.
        data: Vec<u32>,
    },
    /// 인식하지 못한 슬롯(원문 보관).
    Unknown(String),
}

/// 스케줄 슬롯.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    /// 내용.
    pub entry: SlotEntry,
    /// delay(ms).
    pub delay_ms: f64,
    /// 원문 위치.
    pub span: Option<Span>,
}

/// 스케줄 테이블.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScheduleTable {
    /// 이름.
    pub name: String,
    /// 슬롯(실행 순서).
    pub slots: Vec<Slot>,
    /// 원문 위치.
    pub span: Option<Span>,
}

/// 합성 노드(composite_node): 물리 슬레이브 하나를 이루는 논리 노드들.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositeNode {
    /// 합성 노드 이름.
    pub name: String,
    /// 논리 노드 이름(노드 속성에 있어야 한다).
    pub logical: Vec<String>,
}

/// 노드 구성(`configuration`): 클러스터를 이루는 합성 노드 모음 하나.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Configuration {
    /// 구성 이름.
    pub name: String,
    /// 합성 노드들.
    pub composites: Vec<CompositeNode>,
}

/// 노드 구성 섹션(`composite { configuration … }`, LIN 2.2A §9.2.2.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Composition {
    /// 섹션 이름: `composite` 또는 `Node_composition`.
    pub keyword: String,
    /// 구성들.
    pub configurations: Vec<Configuration>,
    /// 합성 노드를 `이름: a, b;`로 쓴 원문이었는지(출력할 때 같은 모양을 따른다).
    #[serde(default)]
    pub colon_form: bool,
    /// 원문 위치. 편집으로 생겼으면 `None`.
    pub span: Option<Span>,
}

/// 동적 프레임 섹션(`dynamic_frames { 1, 2 }`). LIN 2.2A 명세에는 없고 LIN 2.0 파일에 쓰이는 형식이다.
///
/// 이 ID들은 프레임 ID가 겹쳐도 되는 것으로 본다(`ldf-check` 0203·0406).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DynamicFrames {
    /// 섹션 이름: `dynamic_frames` 또는 `Dynamic_frames`.
    pub keyword: String,
    /// 동적 프레임 ID들(원문 순서).
    pub ids: Vec<u32>,
    /// 원문 위치. 편집으로 생겼으면 `None`.
    pub span: Option<Span>,
}

/// 시그널 그룹에 든 시그널 하나: 시그널 이름과 그룹 안의 시작 비트.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupSignal {
    /// 시그널 이름(`Signals`에 있어야 한다).
    pub signal: String,
    /// 그룹 안의 시작 비트(`group_offset`).
    pub offset: u32,
}

/// 시그널 그룹: 시그널 여럿을 묶어 한 덩어리로 다룬다(LIN 1.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalGroup {
    /// 그룹 이름.
    pub name: String,
    /// 그룹 크기(bit).
    pub size_bits: u32,
    /// 든 시그널들.
    pub signals: Vec<GroupSignal>,
}

/// `Signal_groups` 섹션(`그룹: 크기 { 시그널, 오프셋; … }`). LIN 1.3의 문법이고 2.x에서는 쓰지 않는다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalGroups {
    /// 그룹들. 비어 있어도 섹션은 있다(`Signal_groups { }`).
    pub groups: Vec<SignalGroup>,
    /// 원문 위치. 편집으로 생겼으면 `None`.
    pub span: Option<Span>,
}

/// 모델이 해석하지 않는 섹션(왕복 보존용).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawSection {
    /// 섹션 이름.
    pub name: String,
    /// 원문 텍스트(`이름 { ... }` 전체).
    pub text: String,
    /// 원문 위치.
    pub span: Option<Span>,
}

/// 변환(lowering) 중 모델에 담을 수 없었지만 검사 규칙이 필요로 하는 사실의 종류.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoteKind {
    /// `Signal_representation`이 존재하지 않는 시그널을 가리켜 그 항목을 버렸다.
    /// `name`은 인코딩 타입 이름, `detail`은 없는 시그널 이름.
    UnknownRepresentationSignal,
    /// 바이트 배열 초기값에 0–255를 벗어난 값이 있어 그 값을 버렸다. `name`은 시그널 이름.
    ByteInitOutOfRange,
}

/// 변환 중 버려진 정보의 기록. 모델에서 사라졌지만 `ldf-check`가 진단해야 하는 항목이다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LowerNote {
    /// 종류.
    pub kind: NoteKind,
    /// 주 대상 이름(종류별로 정의).
    pub name: String,
    /// 보조 정보(종류별로 정의).
    pub detail: String,
    /// 원문 위치.
    pub span: Option<Span>,
}

/// LDF 모델 전체.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LdfModel {
    /// 헤더.
    pub header: Header,
    /// 노드.
    pub nodes: Arena<NodeId, Node>,
    /// 노드 속성.
    pub node_attrs: Vec<NodeAttrs>,
    /// 시그널(일반 + 진단).
    pub signals: Arena<SignalId, Signal>,
    /// 프레임.
    pub frames: Arena<FrameId, Frame>,
    /// 인코딩 타입.
    pub encodings: Arena<EncodingId, EncodingType>,
    /// 스케줄 테이블.
    pub schedules: Arena<ScheduleId, ScheduleTable>,
    /// 노드 구성(`composite`/`Node_composition`). 모양이 맞지 않는 섹션은 `unknown_sections`에 원문으로 남는다.
    #[serde(default)]
    pub composition: Option<Composition>,
    /// 동적 프레임(`dynamic_frames`). ID 목록이 아닌 섹션은 `unknown_sections`에 원문으로 남는다.
    #[serde(default)]
    pub dynamic_frames: Option<DynamicFrames>,
    /// 시그널 그룹(`Signal_groups`). 모양이 맞지 않는 섹션은 `unknown_sections`에 원문으로 남는다.
    #[serde(default)]
    pub signal_groups: Option<SignalGroups>,
    /// 해석하지 않은 섹션.
    pub unknown_sections: Vec<RawSection>,
    /// 원문의 최상위 문장 순서(왕복 출력용).
    pub section_order: Vec<String>,
    /// 변환 중 모델에 담지 못한 사실(검사용). 이전 버전으로 저장된 모델에는 없다.
    #[serde(default)]
    pub notes: Vec<LowerNote>,
}
