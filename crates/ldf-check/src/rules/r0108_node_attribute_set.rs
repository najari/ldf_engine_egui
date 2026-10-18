//! 규칙 0108: 슬레이브 노드 속성 블록이 완전하지 않다.
//!
//! 0104가 빠진 속성을 속성 이름과 함께 알리는 반면, 이 규칙은 블록 하나당 한 번 "이 노드는 정의가 불완전하다"를 알린다
//! (Vector는 두 결과를 함께 낸다).

use super::r0104_missing_attr::missing;
use crate::Rule;
use crate::util::{attr_proto, diag, node_label};
use ldf_model::{Diagnostic, LdfModel, NodeRole, Severity};

/// 규칙 0108.
pub(crate) struct R0108;
/// 등록용 인스턴스.
pub(crate) static RULE: R0108 = R0108;

impl Rule for R0108 {
    fn code(&self) -> u16 {
        108
    }

    fn title(&self) -> &'static str {
        "Incomplete slave node definition"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        for a in &m.node_attrs {
            let is_slave = a
                .node
                .id()
                .and_then(|id| m.nodes.get(id))
                .is_some_and(|n| n.role == NodeRole::Slave);
            if is_slave && !missing(a, attr_proto(m, a.lin_protocol.as_deref())).is_empty() {
                out.push(diag(
                    Severity::Error,
                    108,
                    a.span,
                    format!("슬레이브 `{}`의 정의가 불완전하다", node_label(m, &a.node)),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::load;

    #[test]
    fn one_report_per_incomplete_block() {
        let src = "LIN_protocol_version = \"2.2\";\nNodes { Master: M, 1 ms, 0 ms; Slaves: S; }\n\
                   Node_attributes { S { LIN_protocol = \"2.2\"; } }";
        let mut out = Vec::new();
        RULE.check(&load(src).model, &mut out);
        assert_eq!(out.len(), 1);
    }
}
