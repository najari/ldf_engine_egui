//! 규칙들이 함께 쓰는 도우미: LIN 버전 판별, 이름 표시, 진단 생성, 비트 배치 계산.

use ldf_model::{
    Diagnostic, EncodingId, Frame, FrameId, FrameKind, FrameSignal, LdfModel, NodeId, NodeRole,
    Ref, Severity, SignalId, Span,
};

/// LDF(또는 슬레이브 노드)가 따르는 LIN 계열.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Proto {
    V1_3,
    V2_0,
    V2_1,
    V2_2,
    J2602,
    Iso,
    /// `OEM variant` 같은 제조사 확장.
    Oem,
    /// 인식하지 못한 문자열.
    Unknown,
}

impl Proto {
    /// `LIN_protocol_version`·노드의 `LIN_protocol` 문자열을 해석한다.
    pub(crate) fn parse_protocol(s: &str) -> Proto {
        let t = s.trim();
        match t {
            "1.3" => Proto::V1_3,
            "2.0" => Proto::V2_0,
            "2.1" => Proto::V2_1,
            "2.2" => Proto::V2_2,
            "J2602_1_1.0" => Proto::J2602,
            "ISO17987:2015" => Proto::Iso,
            _ if t.to_ascii_uppercase().starts_with("OEM") => Proto::Oem,
            _ => Proto::Unknown,
        }
    }

    /// `LIN_language_version` 문자열을 해석한다.
    pub(crate) fn parse_language(s: &str) -> Proto {
        match s.trim() {
            "J2602_3_1.0" => Proto::J2602,
            t if t.starts_with("J2602") => Proto::Unknown,
            t => Self::parse_protocol(t),
        }
    }

    /// LIN 2.1 이상 방식(`initial_NAD`, 메시지 ID 없는 `configurable_frames` 등)을 쓰는지.
    pub(crate) fn is_2_1_style(self) -> bool {
        matches!(self, Proto::V2_1 | Proto::V2_2 | Proto::Iso)
    }

    /// 2.x 방식의 노드 속성이 필요한지(1.3·OEM은 불필요).
    pub(crate) fn has_node_attributes(self) -> bool {
        matches!(
            self,
            Proto::V2_0 | Proto::V2_1 | Proto::V2_2 | Proto::J2602 | Proto::Iso
        )
    }

    /// LIN 2.x 계열의 정렬 번호(비교용). 2.x가 아니면 `None`.
    pub(crate) fn lin2_rank(self) -> Option<u8> {
        match self {
            Proto::V2_0 => Some(0),
            Proto::V2_1 => Some(1),
            Proto::V2_2 => Some(2),
            Proto::Iso => Some(3),
            _ => None,
        }
    }
}

/// 헤더의 `LIN_protocol_version` 계열. 없으면 `Unknown`.
pub(crate) fn file_proto(m: &LdfModel) -> Proto {
    m.header
        .protocol_version
        .as_deref()
        .map_or(Proto::Unknown, Proto::parse_protocol)
}

/// 노드 속성 블록의 `LIN_protocol`이 없거나 알 수 없으면 파일의 버전을 따른다.
pub(crate) fn attr_proto(m: &LdfModel, protocol: Option<&str>) -> Proto {
    match protocol.map(Proto::parse_protocol) {
        Some(p) if p != Proto::Unknown => p,
        _ => file_proto(m),
    }
}

/// 큰 값에도 안전한 비트 수 상한(적대적 입력 방어).
pub(crate) const MAX_BITS: u32 = 4096;

/// 위치가 없을 수 있는 진단 생성.
pub(crate) fn diag(
    severity: Severity,
    code: u16,
    span: Option<Span>,
    message: impl Into<String>,
) -> Diagnostic {
    Diagnostic {
        code,
        severity,
        span,
        message: message.into(),
    }
}

pub(crate) fn node_label(m: &LdfModel, r: &Ref<NodeId>) -> String {
    match r {
        Ref::Resolved(id) => m
            .nodes
            .get(*id)
            .map_or_else(|| "?".to_owned(), |n| n.name.clone()),
        Ref::Unresolved(n) => n.clone(),
    }
}

pub(crate) fn signal_label(m: &LdfModel, r: &Ref<SignalId>) -> String {
    match r {
        Ref::Resolved(id) => m
            .signals
            .get(*id)
            .map_or_else(|| "?".to_owned(), |s| s.name.clone()),
        Ref::Unresolved(n) => n.clone(),
    }
}

pub(crate) fn frame_label(m: &LdfModel, r: &Ref<FrameId>) -> String {
    match r {
        Ref::Resolved(id) => m
            .frames
            .get(*id)
            .map_or_else(|| "?".to_owned(), |f| f.name.clone()),
        Ref::Unresolved(n) => n.clone(),
    }
}

pub(crate) fn encoding_label(m: &LdfModel, r: &Ref<EncodingId>) -> String {
    match r {
        Ref::Resolved(id) => m
            .encodings
            .get(*id)
            .map_or_else(|| "?".to_owned(), |e| e.name.clone()),
        Ref::Unresolved(n) => n.clone(),
    }
}

/// 노드 이름으로 노드를 찾는다(같은 이름이 여럿이면 첫 번째).
pub(crate) fn find_node<'a>(m: &'a LdfModel, name: &str) -> Option<(NodeId, &'a ldf_model::Node)> {
    m.nodes.iter().find(|(_, n)| n.name == name)
}

/// 노드가 마스터인지.
pub(crate) fn is_master(m: &LdfModel, r: &Ref<NodeId>) -> bool {
    r.id()
        .and_then(|id| m.nodes.get(id))
        .is_some_and(|n| n.role == NodeRole::Master)
}

/// 노드 ID 참조가 같은 노드를 가리키는지(해석되지 않았으면 이름 비교).
pub(crate) fn same_node(a: &Ref<NodeId>, b: &Ref<NodeId>) -> bool {
    match (a, b) {
        (Ref::Resolved(x), Ref::Resolved(y)) => x == y,
        (Ref::Unresolved(x), Ref::Unresolved(y)) => x == y,
        _ => false,
    }
}

/// 무조건 프레임의 구성 요소.
pub(crate) struct Uncond<'a> {
    pub id: FrameId,
    pub frame: &'a Frame,
    pub lin_id: u32,
    pub publisher: &'a Ref<NodeId>,
    pub size: u32,
    pub signals: &'a [FrameSignal],
}

/// 무조건 프레임 목록.
pub(crate) fn unconditional(m: &LdfModel) -> Vec<Uncond<'_>> {
    m.frames
        .iter()
        .filter_map(|(id, f)| match &f.kind {
            FrameKind::Unconditional {
                id: lin_id,
                publisher,
                size,
                signals,
            } => Some(Uncond {
                id,
                frame: f,
                lin_id: *lin_id,
                publisher,
                size: *size,
                signals,
            }),
            _ => None,
        })
        .collect()
}

/// 이벤트 트리거 프레임에 연관된 무조건 프레임의 ID 집합.
pub(crate) fn etf_associated(m: &LdfModel) -> Vec<FrameId> {
    let mut out = Vec::new();
    for (_, f) in m.frames.iter() {
        if let FrameKind::EventTriggered { associated, .. } = &f.kind {
            out.extend(associated.iter().filter_map(|r| r.id()));
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// 시그널이 차지하는 프레임 비트 구간 `[lo, hi]`(양끝 포함) 목록.
///
/// - 리틀 엔디언: 시작 비트부터 위로 `size`비트.
/// - 빅 엔디언(Motorola): 시작 비트는 MSB 위치이며 바이트 안에서 아래로 내려가고,
///   바이트가 끝나면 다음 바이트의 비트 7부터 이어진다.
///
/// 비정상적으로 큰 `size`는 [`MAX_BITS`]로 자른다.
pub(crate) fn bit_spans(offset: u32, size: u32, big_endian: bool) -> Vec<(u64, u64)> {
    let size = u64::from(size.min(MAX_BITS));
    let offset = u64::from(offset);
    if size == 0 {
        return Vec::new();
    }
    if !big_endian {
        return vec![(offset, offset + size - 1)];
    }
    let mut out = Vec::new();
    let mut remaining = size;
    let mut byte = offset / 8;
    let mut k = offset % 8; // 현재 바이트에서 쓸 수 있는 가장 높은 비트
    while remaining > 0 {
        let take = remaining.min(k + 1);
        let hi = byte * 8 + k;
        out.push((hi + 1 - take, hi));
        remaining -= take;
        byte += 1;
        k = 7;
    }
    out
}

/// 비트 구간이 닿는 바이트 번호의 개수.
pub(crate) fn bytes_touched(spans: &[(u64, u64)]) -> usize {
    let mut bytes: Vec<u64> = Vec::new();
    for &(lo, hi) in spans {
        let mut b = lo / 8;
        while b <= hi / 8 && bytes.len() <= 64 {
            bytes.push(b);
            b += 1;
        }
    }
    bytes.sort_unstable();
    bytes.dedup();
    bytes.len()
}

/// 헤더가 빅 엔디언 비트 순서를 쓰는지(`LIN_sig_byte_order_big_endian;` 또는 OEM 변형).
pub(crate) fn big_endian(m: &LdfModel) -> bool {
    m.header.sig_byte_order_big_endian || file_proto(m) == Proto::Oem
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_strings() {
        assert_eq!(Proto::parse_protocol("2.2"), Proto::V2_2);
        assert_eq!(Proto::parse_protocol("OEM variant"), Proto::Oem);
        assert_eq!(Proto::parse_protocol("2.3"), Proto::Unknown);
        assert_eq!(Proto::parse_language("J2602_3_1.0"), Proto::J2602);
        assert_eq!(Proto::parse_language("J2602_3_1.1"), Proto::Unknown);
    }

    #[test]
    fn little_endian_spans() {
        assert_eq!(bit_spans(8, 8, false), vec![(8, 15)]);
        assert!(bit_spans(0, 0, false).is_empty());
    }

    #[test]
    fn big_endian_spans() {
        // 시작 비트 7, 8비트 -> 바이트 0 전체
        assert_eq!(bit_spans(7, 8, true), vec![(0, 7)]);
        // 시작 비트 14, 8비트 -> 바이트 1의 비트 6..0(7비트) + 바이트 2의 비트 7
        assert_eq!(bit_spans(14, 8, true), vec![(8, 14), (23, 23)]);
    }

    #[test]
    fn huge_sizes_are_bounded() {
        let s = bit_spans(0, u32::MAX, true);
        assert!(s.len() < 1000);
        assert!(bytes_touched(&bit_spans(u32::MAX, u32::MAX, false)) <= 65);
    }
}
