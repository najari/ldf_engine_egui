//! LIN 노드 기능 파일(NCF, Node Capability File)을 읽는다. LIN 2.2A 8장(Node Capability Language)이 문법이다.
//!
//! NCF는 슬레이브 노드 **하나**의 기능을 적은 파일이다(LDF는 클러스터 전체). 이 크레이트는 읽기 전용이다:
//! 파일을 구조체로 풀어 보여 주고, 규격에 없거나 읽을 수 없는 항목은 잃지 않도록 원문 문자열(`Item`)로 보관한다.
//! 구문 계층은 `ldf-syntax`를 그대로 쓴다(NCF도 `이름 = 값;`과 `{ }` 블록으로 이루어져 있다).
//!
//! ```text
//! node_capability_file;
//! LIN_language_version = "2.1";
//! node Slave1 {
//!   general { LIN_protocol_version = "2.1"; supplier = 0x1e; function = 1; variant = 0; bitrate = 19.2 kbps; }
//!   diagnostic { NAD = 1; P2_min = 50 ms; }
//!   frames { publish F { length = 2; signals { S { init_value = 0; size = 16; offset = 0; } } } }
//!   encoding { E { logical_value, 0, "off"; } }
//!   status_management { response_error = S; }
//! }
//! ```

mod describe;
mod parse;

pub use describe::{describe_entry, describe_init, format_bps, format_ms, hex};
pub use parse::{Parsed, is_ncf, parse};

use ldf_model::{EncodingEntry, InitValue};
use ldf_syntax::Span;
use serde::Serialize;

/// NCF가 내는 진단 코드(9201–9205). 구문 계층의 9001–9012와 함께 쓴다.
pub mod codes {
    /// `node_capability_file;`로 시작하지 않는다.
    pub const NOT_NCF: u16 = 9201;
    /// 알 수 없는 항목(원문으로 보관).
    pub const UNKNOWN_ITEM: u16 = 9202;
    /// 값을 읽을 수 없다.
    pub const BAD_VALUE: u16 = 9203;
    /// 문장 모양이 맞지 않는다.
    pub const BAD_STRUCTURE: u16 = 9204;
    /// `node` 블록이 없다.
    pub const NO_NODE: u16 = 9205;
}

/// 해석하지 않은 항목: 키와 값의 원문(공백은 하나로 줄인다).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Item {
    /// 키(`이름 = 값;`의 이름이나 `이름 { }`의 이름).
    pub key: String,
    /// 값 원문.
    pub value: String,
    /// 원문 위치.
    #[serde(skip)]
    pub span: Option<Span>,
}

/// NCF 파일 하나.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct NcfFile {
    /// `LIN_language_version`.
    pub language_version: Option<String>,
    /// `node` 블록들(보통 하나).
    pub nodes: Vec<NcfNode>,
}

/// 노드 하나의 기능.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct NcfNode {
    /// 노드 이름.
    pub name: String,
    /// `general`: 제품 식별과 비트레이트.
    pub general: General,
    /// `diagnostic`: 진단 능력.
    pub diagnostic: DiagnosticInfo,
    /// `frames`: 발행·구독하는 프레임.
    pub frames: Vec<NcfFrame>,
    /// `encoding`: 시그널 인코딩.
    pub encodings: Vec<NcfEncoding>,
    /// `status_management`: 항목 그대로(`response_error = 시그널;` 등).
    pub status_management: Vec<Item>,
    /// `free_text`.
    pub free_text: Option<String>,
    /// 해석하지 않은 노드 수준 항목.
    pub extra: Vec<Item>,
    /// 원문 위치.
    #[serde(skip)]
    pub span: Option<Span>,
}

/// `general` 섹션.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct General {
    /// `LIN_protocol_version`.
    pub protocol_version: Option<String>,
    /// 공급자 ID.
    pub supplier: Option<u32>,
    /// 기능 ID.
    pub function: Option<u32>,
    /// 변형.
    pub variant: Option<u32>,
    /// 비트레이트.
    pub bitrate: Option<Bitrate>,
    /// `sends_wake_up_signal`(`"yes"`/`"no"`).
    pub sends_wake_up_signal: Option<String>,
    /// 해석하지 않은 항목(`response_tolerance` 등).
    pub extra: Vec<Item>,
}

/// 노드가 지원하는 비트레이트(bit/s).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum Bitrate {
    /// 하나로 고정.
    Fixed {
        /// bit/s.
        bps: f64,
    },
    /// 자동 감지, 선택적으로 범위.
    Automatic {
        /// 아래 한계(bit/s).
        min_bps: Option<f64>,
        /// 위 한계(bit/s).
        max_bps: Option<f64>,
    },
    /// 고른 값들 중 하나.
    Select {
        /// 후보(bit/s).
        bps: Vec<f64>,
    },
}

/// 노드 주소(NAD).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Nad {
    /// 나열한 값(보통 하나).
    List(Vec<u32>),
    /// `from to to` 범위.
    Range {
        /// 시작.
        from: u32,
        /// 끝.
        to: u32,
    },
}

/// `diagnostic` 섹션.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct DiagnosticInfo {
    /// 노드 주소.
    pub nad: Option<Nad>,
    /// 진단 클래스.
    pub diagnostic_class: Option<u32>,
    /// `P2_min`(ms).
    pub p2_min_ms: Option<f64>,
    /// `ST_min`(ms).
    pub st_min_ms: Option<f64>,
    /// `N_As_timeout`(ms).
    pub n_as_timeout_ms: Option<f64>,
    /// `N_Cr_timeout`(ms).
    pub n_cr_timeout_ms: Option<f64>,
    /// 지원하는 서비스 ID.
    pub support_sid: Vec<u32>,
    /// 최대 메시지 길이(바이트).
    pub max_message_length: Option<u32>,
    /// 해석하지 않은 항목.
    pub extra: Vec<Item>,
}

/// 프레임 방향(노드 기준).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Direction {
    /// 노드가 보낸다.
    Publish,
    /// 노드가 받는다.
    Subscribe,
}

/// 이벤트 트리거 프레임과의 연결.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum EventTriggered {
    /// 프레임 이름으로(LIN 2.1 이후): `event_triggered_frame = 이름;`
    Frame(String),
    /// 메시지 ID로(LIN 2.0): `event_triggered_message_ID = 0x4567;`
    MessageId(u32),
}

/// `frames`의 프레임 하나.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NcfFrame {
    /// 발행/구독.
    pub direction: Direction,
    /// 프레임 이름.
    pub name: String,
    /// 길이(바이트).
    pub length: Option<u32>,
    /// 메시지 ID(LIN 2.0 NCF).
    pub message_id: Option<u32>,
    /// 최소 주기(ms).
    pub min_period_ms: Option<f64>,
    /// 최대 주기(ms).
    pub max_period_ms: Option<f64>,
    /// 이벤트 트리거 프레임과의 연결.
    pub event_triggered: Option<EventTriggered>,
    /// 시그널.
    pub signals: Vec<NcfSignal>,
    /// 해석하지 않은 항목.
    pub extra: Vec<Item>,
    /// 원문 위치.
    #[serde(skip)]
    pub span: Option<Span>,
}

/// 시그널의 인코딩: 이름으로 가리키거나 그 자리에서 정의한다.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum EncodingRef {
    /// `encoding` 섹션의 인코딩 이름.
    Named(String),
    /// 시그널 안에서 정의한 인코딩(`encoding 이름 { ... };`).
    Inline(NcfEncoding),
}

/// 프레임 안의 시그널.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NcfSignal {
    /// 이름.
    pub name: String,
    /// 초기값.
    pub init: Option<InitValue>,
    /// 크기(비트).
    pub size: Option<u32>,
    /// 프레임 안의 시작 비트.
    pub offset: Option<u32>,
    /// 인코딩.
    pub encoding: Option<EncodingRef>,
    /// 해석하지 않은 항목.
    pub extra: Vec<Item>,
    /// 원문 위치.
    #[serde(skip)]
    pub span: Option<Span>,
}

/// 인코딩 타입.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NcfEncoding {
    /// 이름.
    pub name: String,
    /// 항목들(LDF와 같은 `logical_value`/`physical_value`/`bcd_value`/`ascii_value`).
    pub entries: Vec<EncodingEntry>,
    /// 원문 위치.
    #[serde(skip)]
    pub span: Option<Span>,
}

impl NcfNode {
    /// ncoding 섹션의 인코딩과 시그널 안에서 정의한 인코딩 전부(이 순서로).
    pub fn all_encodings(&self) -> Vec<&NcfEncoding> {
        let inline =
            self.frames
                .iter()
                .flat_map(|f| &f.signals)
                .filter_map(|s| match &s.encoding {
                    Some(EncodingRef::Inline(e)) => Some(e),
                    _ => None,
                });
        self.encodings.iter().chain(inline).collect()
    }

    /// 이름으로 인코딩을 찾는다(시그널 안에서 정의한 것도 본다).
    pub fn find_encoding(&self, name: &str) -> Option<&NcfEncoding> {
        self.encodings.iter().find(|e| e.name == name).or_else(|| {
            self.frames
                .iter()
                .flat_map(|f| &f.signals)
                .find_map(|s| match &s.encoding {
                    Some(EncodingRef::Inline(e)) if e.name == name => Some(e),
                    _ => None,
                })
        })
    }

    /// 프레임 비트 중 시그널이 차지하는 범위가 프레임 길이를 넘는 시그널의 이름. 넘치지 않으면 빈 목록이다.
    pub fn overflowing_signals(&self) -> Vec<(&str, &str)> {
        let mut out = Vec::new();
        for f in &self.frames {
            let Some(len) = f.length else { continue };
            for s in &f.signals {
                if let (Some(size), Some(off)) = (s.size, s.offset)
                    && off.saturating_add(size) > len.saturating_mul(8)
                {
                    out.push((f.name.as_str(), s.name.as_str()));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
