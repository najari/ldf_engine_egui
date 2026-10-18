//! 규칙 0104: 슬레이브의 노드 속성 블록에서 필수 속성이 빠졌다.

use crate::Rule;
use crate::util::{Proto, attr_proto, diag, node_label};
use ldf_model::{Diagnostic, LdfModel, NodeAttrs, NodeRole, Severity};

/// 규칙 0104.
pub(crate) struct R0104;
/// 등록용 인스턴스.
pub(crate) static RULE: R0104 = R0104;

/// 노드 버전 `p`에서 `a`에 빠진 필수 속성. 1.3·OEM 노드는 요구하지 않는다.
pub(crate) fn missing(a: &NodeAttrs, p: Proto) -> Vec<&'static str> {
    if !p.has_node_attributes() {
        return Vec::new();
    }
    let mut v = Vec::new();
    if a.configured_nad.is_none() {
        v.push("configured_NAD");
    }
    if a.product_id.is_none() {
        v.push("product_id");
    }
    if a.response_error.is_none() {
        v.push("response_error");
    }
    // J2602 노드는 P2_min·ST_min을 요구하지 않는다
    if p != Proto::J2602 {
        if a.p2_min_ms.is_none() {
            v.push("P2_min");
        }
        if a.st_min_ms.is_none() {
            v.push("ST_min");
        }
    }
    if a.configurable_frames.is_empty() {
        v.push("configurable_frames");
    }
    v
}

impl Rule for R0104 {
    fn code(&self) -> u16 {
        104
    }

    fn title(&self) -> &'static str {
        "Node attribute missing"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        if super::attribute_format_cascade(m).is_some() {
            out.push(super::cascade_diag(104, Severity::Error));
        }
        for a in &m.node_attrs {
            let is_slave = a
                .node
                .id()
                .and_then(|id| m.nodes.get(id))
                .is_some_and(|n| n.role == NodeRole::Slave);
            if !is_slave {
                continue;
            }
            let miss = missing(a, attr_proto(m, a.lin_protocol.as_deref()));
            if !miss.is_empty() {
                out.push(diag(
                    Severity::Error,
                    104,
                    a.span,
                    format!(
                        "노드 `{}`에 필수 속성이 없다: {}",
                        node_label(m, &a.node),
                        miss.join(", ")
                    ),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::load;

    fn n(version: &str, attrs: &str) -> usize {
        let src = format!(
            "LIN_protocol_version = \"{version}\";\nNodes {{ Master: M, 1 ms, 0 ms; Slaves: S; }}\nFrames {{ F: 1, M, 1 {{ }} }}\nNode_attributes {{ S {{ LIN_protocol = \"{version}\"; {attrs} }} }}"
        );
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        out.len()
    }

    const FULL: &str = "configured_NAD = 1; product_id = 1, 2; response_error = E; P2_min = 50 ms; ST_min = 0 ms; configurable_frames { F; }";

    #[test]
    fn complete_and_incomplete_blocks() {
        assert_eq!(n("2.2", FULL), 0);
        assert_eq!(n("2.2", &FULL.replace("product_id = 1, 2;", "")), 1);
        assert_eq!(n("2.2", &FULL.replace("P2_min = 50 ms;", "")), 1);
        assert_eq!(n("1.3", ""), 0);
        // J2602는 메시지 ID 표기(`= ID`)를 쓴다
        let j2602 = FULL
            .replace("P2_min = 50 ms; ST_min = 0 ms;", "")
            .replace("{ F; }", "{ F = 1; }");
        assert_eq!(n("J2602_1_1.0", &j2602), 0);
    }
}
