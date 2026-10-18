//! 규칙 0103: 노드의 LIN 버전에서 쓸 수 없는 노드 속성, 마스터에 둔 노드 속성,
//! `configurable_frames`의 메시지 ID 표기가 버전과 맞지 않는 경우.

use crate::Rule;
use crate::util::{Proto, attr_proto, diag, node_label};
use ldf_model::{Diagnostic, LdfModel, NodeAttrs, NodeRole, Severity};

/// 규칙 0103.
pub(crate) struct R0103;
/// 등록용 인스턴스.
pub(crate) static RULE: R0103 = R0103;

/// 노드 버전 `p`에서 쓸 수 없는 속성 이름들.
fn disallowed(a: &NodeAttrs, p: Proto) -> Vec<&'static str> {
    let mut v = Vec::new();
    let mut flag = |present: bool, name: &'static str| {
        if present {
            v.push(name);
        }
    };
    match p {
        Proto::V1_3 => {
            flag(a.initial_nad.is_some(), "initial_NAD");
            flag(a.product_id.is_some(), "product_id");
            flag(a.response_error.is_some(), "response_error");
            flag(!a.fault_state_signals.is_empty(), "fault_state_signals");
            flag(a.p2_min_ms.is_some(), "P2_min");
            flag(a.st_min_ms.is_some(), "ST_min");
            flag(a.n_as_timeout_ms.is_some(), "N_As_timeout");
            flag(a.n_cr_timeout_ms.is_some(), "N_Cr_timeout");
            flag(!a.configurable_frames.is_empty(), "configurable_frames");
        }
        Proto::V2_0 | Proto::J2602 => {
            flag(a.initial_nad.is_some(), "initial_NAD");
            flag(!a.fault_state_signals.is_empty(), "fault_state_signals");
            flag(a.n_as_timeout_ms.is_some(), "N_As_timeout");
            flag(a.n_cr_timeout_ms.is_some(), "N_Cr_timeout");
        }
        _ => {}
    }
    v
}

impl Rule for R0103 {
    fn code(&self) -> u16 {
        103
    }

    fn title(&self) -> &'static str {
        "Node attribute not allowed for LIN version"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        for a in &m.node_attrs {
            let name = node_label(m, &a.node);
            let is_master = a
                .node
                .id()
                .and_then(|id| m.nodes.get(id))
                .is_some_and(|n| n.role == NodeRole::Master);
            if is_master {
                out.push(diag(
                    Severity::Error,
                    103,
                    a.span,
                    format!("마스터 노드 `{name}`에는 노드 속성을 둘 수 없다"),
                ));
                continue;
            }
            let p = attr_proto(m, a.lin_protocol.as_deref());
            for attr in disallowed(a, p) {
                out.push(diag(
                    Severity::Error,
                    103,
                    a.span,
                    format!(
                        "노드 `{name}`(LIN {})에서 속성 {attr}을(를) 쓸 수 없다",
                        a.lin_protocol.as_deref().unwrap_or("?")
                    ),
                ));
            }
            // 메시지 ID는 LIN 2.0·J2602에서만 쓴다
            let needs_ids = matches!(p, Proto::V2_0 | Proto::J2602);
            let uses_ids = matches!(p, Proto::V2_1 | Proto::V2_2 | Proto::Iso);
            for c in &a.configurable_frames {
                if needs_ids && c.message_id.is_none() {
                    out.push(diag(
                        Severity::Error,
                        103,
                        a.span,
                        format!(
                            "노드 `{name}`의 configurable_frames 항목 `{}`에 메시지 ID가 없다(LIN {}는 `= ID`가 필요하다)",
                            crate::util::frame_label(m, &c.frame),
                            a.lin_protocol.as_deref().unwrap_or("?")
                        ),
                    ));
                } else if uses_ids && c.message_id.is_some() {
                    out.push(diag(
                        Severity::Error,
                        103,
                        a.span,
                        format!(
                            "노드 `{name}`의 configurable_frames 항목 `{}`에 메시지 ID를 쓸 수 없다(LIN {})",
                            crate::util::frame_label(m, &c.frame),
                            a.lin_protocol.as_deref().unwrap_or("?")
                        ),
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

    fn n(file: &str, attrs: &str) -> usize {
        let src = format!(
            "LIN_protocol_version = \"{file}\";\nNodes {{ Master: M, 1 ms, 0 ms; Slaves: S; }}\nFrames {{ F: 1, M, 1 {{ }} }}\nNode_attributes {{ S {{ {attrs} }} }}"
        );
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        out.len()
    }

    #[test]
    fn attributes_by_node_version() {
        assert_eq!(n("2.0", "LIN_protocol = \"2.0\"; configured_NAD = 1;"), 0);
        assert_eq!(n("2.0", "LIN_protocol = \"2.0\"; initial_NAD = 1;"), 1);
        assert_eq!(
            n(
                "2.0",
                "LIN_protocol = \"2.0\"; initial_NAD = 1; N_As_timeout = 5 ms;"
            ),
            2
        );
        assert_eq!(n("2.2", "LIN_protocol = \"2.2\"; initial_NAD = 1;"), 0);
        assert_eq!(n("2.2", "LIN_protocol = \"1.3\"; P2_min = 50 ms;"), 1);
    }

    #[test]
    fn message_id_notation_follows_version() {
        assert_eq!(
            n("2.0", "LIN_protocol = \"2.0\"; configurable_frames { F; }"),
            1
        );
        assert_eq!(
            n(
                "2.0",
                "LIN_protocol = \"2.0\"; configurable_frames { F = 0x10; }"
            ),
            0
        );
        assert_eq!(
            n(
                "2.1",
                "LIN_protocol = \"2.1\"; configurable_frames { F = 0x10; }"
            ),
            1
        );
    }
}
