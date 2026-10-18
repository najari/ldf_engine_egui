//! 규칙 0109: 노드 구성(`composite { configuration … }`, ISO·2.1 이후 이름은 `Node_composition`)이 규격과 맞지 않는다.
//!
//! LIN 2.2A §9.2.2.3: 논리 노드(logical_node)는 노드 속성(`Node_attributes`)에 있어야 하고, 구성 안에서 합성 노드(composite_node)와
//! 논리 노드의 이름은 겹치지 않아야 한다.
//! (이 번호는 Vector의 검사에 없는 이 프로젝트의 것이다.)

use super::r0101_sections::section_allowed;
use crate::Rule;
use crate::util::{diag, file_proto, node_label};
use ldf_model::{Diagnostic, LdfModel, Severity};
use std::collections::BTreeSet;

/// 규칙 0109.
pub(crate) struct R0109;
/// 등록용 인스턴스.
pub(crate) static RULE: R0109 = R0109;

impl Rule for R0109 {
    fn code(&self) -> u16 {
        109
    }

    fn title(&self) -> &'static str {
        "Node composition inconsistent"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        let Some(c) = &m.composition else { return };
        // 이 버전에서 쓸 수 없는 섹션은 0101이 보고하고 여기서는 내용을 따지지 않는다(Vector `Format/0201`)
        if !section_allowed(&c.keyword, file_proto(m)) {
            return;
        }
        let described: BTreeSet<String> = m
            .node_attrs
            .iter()
            .map(|a| node_label(m, &a.node))
            .collect();
        for cfg in &c.configurations {
            let mut seen: BTreeSet<&str> = BTreeSet::new();
            for node in &cfg.composites {
                // 합성 노드 이름이 먼저, 그 뒤에 논리 노드들
                for (i, name) in std::iter::once(&node.name).chain(&node.logical).enumerate() {
                    if !seen.insert(name) {
                        out.push(diag(
                            Severity::Error,
                            109,
                            c.span,
                            format!(
                                "구성 `{}`에서 노드 이름 `{name}`이(가) 두 번 나온다",
                                cfg.name
                            ),
                        ));
                    }
                    if i > 0 && !described.contains(name.as_str()) {
                        out.push(diag(
                            Severity::Error,
                            109,
                            c.span,
                            format!(
                                "구성 `{}`의 논리 노드 `{name}`이(가) 노드 속성에 없다",
                                cfg.name
                            ),
                        ));
                    }
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::load;

    fn n(section: &str) -> usize {
        let src = format!(
            "LIN_protocol_version = \"2.2\";\nNodes {{ Master: M, 1 ms, 0 ms; Slaves: A, B; }}\n\
             {section}\nNode_attributes {{ A {{ LIN_protocol = \"2.2\"; }} B {{ LIN_protocol = \"2.2\"; }} }}"
        );
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        out.len()
    }

    #[test]
    fn spec_form_and_colon_form() {
        // LIN 2.2A §9.2.2.3
        assert_eq!(n("composite { configuration c1 { Phys { A, B; } } }"), 0);
        assert_eq!(
            n("Node_composition { configuration c1 { Phys: A, B; } }"),
            0
        );
        assert_eq!(n("Signals { }"), 0);
        // ISO 17987의 `composite`는 허용되지 않는 섹션(0101)이라 내용을 따지지 않는다
        let iso = "composite { configuration c { P { Ghost; } } }";
        let src = format!(
            "LIN_protocol_version = \"ISO17987:2015\";\nNodes {{ Master: M, 1 ms, 0 ms; Slaves: A; }}\n{iso}"
        );
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn unknown_and_repeated_nodes() {
        // 노드 속성에 없는 논리 노드
        assert_eq!(
            n("composite { configuration c1 { Phys { A, Ghost; } } }"),
            1
        );
        // 같은 구성에서 논리 노드가 두 합성 노드에 들어 있다
        assert_eq!(
            n("composite { configuration c1 { P1 { A; } P2 { A, B; } } }"),
            1
        );
        // 다른 구성이면 같은 노드가 다시 나와도 된다
        assert_eq!(
            n("composite { configuration c1 { P1 { A; } } configuration c2 { P1 { A; } } }"),
            0
        );
    }
}
