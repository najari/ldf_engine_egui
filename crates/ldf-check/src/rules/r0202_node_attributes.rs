//! 규칙 0202: 노드 속성의 모델링 오류.
//!
//! - LIN 2.0 `configurable_frames`의 메시지 ID가 한 노드 안의 서로 다른 프레임에서 겹친다.
//! - `configured_NAD`·`initial_NAD`가 슬레이브끼리 겹친다.
//! - `response_error` 시그널이 허용 길이(LIN 1비트, J2602 3비트)보다 길다.
//! - 2.x 파일에서 슬레이브에 노드 속성 블록이 없다.

use crate::Rule;
use crate::util::{Proto, attr_proto, diag, file_proto, frame_label, node_label};
use ldf_model::{Diagnostic, FrameId, LdfModel, NodeRole, Ref, Severity};
use std::collections::BTreeMap;

/// 규칙 0202.
pub(crate) struct R0202;
/// 등록용 인스턴스.
pub(crate) static RULE: R0202 = R0202;

impl Rule for R0202 {
    fn code(&self) -> u16 {
        202
    }

    fn title(&self) -> &'static str {
        "Node attribute inconsistent"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        if super::attribute_format_cascade(m) == Some(true) {
            out.push(super::cascade_diag(202, Severity::Error));
        }
        let file = file_proto(m);

        // 메시지 ID는 노드 안에서 프레임마다 달라야 한다(같은 프레임의 ID가 노드마다 다른 것은 허용)
        for a in &m.node_attrs {
            let mut by_id: BTreeMap<u32, Vec<FrameId>> = BTreeMap::new();
            for c in &a.configurable_frames {
                if let (Some(id), Some(fid)) = (c.message_id, c.frame.id()) {
                    let frames = by_id.entry(id).or_default();
                    if !frames.contains(&fid) {
                        frames.push(fid);
                    }
                }
            }
            for (id, frames) in &by_id {
                if frames.len() > 1 {
                    for fid in frames {
                        out.push(diag(
                            Severity::Error,
                            202,
                            a.span,
                            format!(
                                "노드 `{}`에서 메시지 ID {id:#06X}를 프레임 `{}`을(를) 포함해 {}개 프레임이 함께 쓴다",
                                node_label(m, &a.node),
                                frame_label(m, &Ref::Resolved(*fid)),
                                frames.len()
                            ),
                        ));
                    }
                }
            }
        }
        // NAD 중복
        let dup =
            |values: Vec<(u32, Option<ldf_model::Span>)>, what: &str, out: &mut Vec<Diagnostic>| {
                let mut seen: BTreeMap<u32, usize> = BTreeMap::new();
                for (v, _) in &values {
                    *seen.entry(*v).or_insert(0) += 1;
                }
                for (v, n) in seen {
                    if n > 1 {
                        let span = values.iter().find(|(x, _)| *x == v).and_then(|(_, s)| *s);
                        out.push(diag(
                            Severity::Error,
                            202,
                            span,
                            format!("{what} {v:#04X}를 슬레이브 {n}개가 함께 쓴다"),
                        ));
                    }
                }
            };
        dup(
            m.node_attrs
                .iter()
                .filter_map(|a| a.configured_nad.map(|v| (v, a.span)))
                .collect(),
            "configured_NAD",
            out,
        );
        dup(
            m.node_attrs
                .iter()
                .filter_map(|a| a.initial_nad.map(|v| (v, a.span)))
                .collect(),
            "initial_NAD",
            out,
        );

        // response_error 시그널 길이
        for a in &m.node_attrs {
            let Some(sig) = a
                .response_error
                .as_ref()
                .and_then(Ref::id)
                .and_then(|id| m.signals.get(id))
            else {
                continue;
            };
            let limit = if attr_proto(m, a.lin_protocol.as_deref()) == Proto::J2602 {
                3
            } else {
                1
            };
            if sig.size_bits > limit {
                out.push(diag(
                    Severity::Error,
                    202,
                    a.span,
                    format!(
                        "노드 `{}`의 response_error 시그널 `{}`이(가) {}비트로 허용 길이 {limit}비트보다 길다",
                        node_label(m, &a.node),
                        sig.name,
                        sig.size_bits
                    ),
                ));
            }
        }

        // 속성 블록이 없는 슬레이브
        if file.has_node_attributes() && m.section_order.iter().any(|s| s == "Node_attributes") {
            for (nid, n) in m.nodes.iter() {
                if n.role == NodeRole::Slave
                    && !m.node_attrs.iter().any(|a| a.node.id() == Some(nid))
                {
                    out.push(diag(
                        Severity::Error,
                        202,
                        n.span,
                        format!("슬레이브 `{}`에 노드 속성 블록이 없다", n.name),
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::load;

    fn n(attrs: &str) -> usize {
        let src = format!(
            "LIN_protocol_version = \"2.2\";\n\
             Nodes {{ Master: M, 1 ms, 0 ms; Slaves: S, T; }}\n\
             Signals {{ E: 1, 0, S, M; F: 2, 0, T, M; }}\n\
             Frames {{ F1: 1, S, 1 {{ E, 0; }} F2: 2, T, 1 {{ F, 0; }} }}\n\
             Node_attributes {{ {attrs} }}"
        );
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        out.len()
    }

    #[test]
    fn nad_response_error_and_missing_blocks() {
        let s = "S { LIN_protocol = \"2.2\"; configured_NAD = 1; response_error = E; }";
        let t = |nad: u32, re: &str| {
            format!(
                "T {{ LIN_protocol = \"2.2\"; configured_NAD = {nad}; response_error = {re}; }}"
            )
        };
        assert_eq!(n(&format!("{s} {}", t(2, "F"))), 1); // F가 2비트
        assert_eq!(n(&format!("{s} {}", t(1, "E"))), 1); // NAD 중복
        assert_eq!(n(s), 1); // T의 블록 없음
    }
}
