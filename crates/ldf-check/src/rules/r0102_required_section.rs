//! 규칙 0102: LIN 2.x 파일에 슬레이브가 있는데 `Node_attributes` 섹션이 없다.

use crate::Rule;
use crate::util::{diag, file_proto};
use ldf_model::{Diagnostic, LdfModel, NodeRole, Severity};

/// 규칙 0102.
pub(crate) struct R0102;
/// 등록용 인스턴스.
pub(crate) static RULE: R0102 = R0102;

impl Rule for R0102 {
    fn code(&self) -> u16 {
        102
    }

    fn title(&self) -> &'static str {
        "Required section missing"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        let p = file_proto(m);
        let has_slave = m.nodes.iter().any(|(_, n)| n.role == NodeRole::Slave);
        let has_section = m.section_order.iter().any(|s| s == "Node_attributes");
        if p.has_node_attributes() && has_slave && !has_section {
            out.push(diag(
                Severity::Error,
                102,
                None,
                format!(
                    "LIN {} 파일에 슬레이브가 있지만 Node_attributes 섹션이 없다",
                    m.header.protocol_version.as_deref().unwrap_or("2.x")
                ),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::load;

    #[test]
    fn missing_node_attributes_section() {
        let base = "LIN_protocol_version = \"2.2\";\nNodes { Master: M, 1 ms, 0 ms; Slaves: S; }\n";
        let mut out = Vec::new();
        RULE.check(&load(base).model, &mut out);
        assert_eq!(out.len(), 1);
        out.clear();
        let with = format!("{base}Node_attributes {{ }}");
        RULE.check(&load(&with).model, &mut out);
        assert!(out.is_empty());
    }
}
