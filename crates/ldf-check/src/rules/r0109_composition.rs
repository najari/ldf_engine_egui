//! 규칙 0109: 노드 구성(`composite { configuration … }`, ISO·2.1 이후 이름은 `Node_composition`)이 규격과 맞지 않는다.
//!
//! LIN 2.2A §9.2.2.3: 논리 노드(logical_node)는 노드 속성(`Node_attributes`)에 있어야 하고, 구성 안에서 합성 노드(composite_node)와
//! 논리 노드의 이름은 겹치지 않아야 한다. 이 섹션은 모델이 해석하지 않고 원문으로 보관하므로 여기서 원문을 읽는다.
//! (이 번호는 Vector의 검사에 없는 이 프로젝트의 것이다.)

use super::r0101_sections::section_allowed;
use crate::Rule;
use crate::raw::section;
use crate::util::{diag, file_proto, node_label};
use ldf_model::{Diagnostic, LdfModel, Severity};
use ldf_syntax::{Elem, Stmt, TokenKind, parse};
use std::collections::BTreeSet;

/// 규칙 0109.
pub(crate) struct R0109;
/// 등록용 인스턴스.
pub(crate) static RULE: R0109 = R0109;

/// 문장 안(블록 속 문장 포함)의 식별자들.
fn idents(src: &str, stmt: &Stmt, out: &mut Vec<String>) {
    for e in &stmt.elems {
        match e {
            Elem::Token(t) if t.kind == TokenKind::Ident => {
                out.push(t.span.text(src).to_owned());
            }
            Elem::Token(_) => {}
            Elem::Block(b) => {
                for s in &b.stmts {
                    idents(src, s, out);
                }
            }
        }
    }
}

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
        let described: BTreeSet<String> = m
            .node_attrs
            .iter()
            .map(|a| node_label(m, &a.node))
            .collect();
        let proto = file_proto(m);
        for sec in section(m, &["composite", "Node_composition"]) {
            // 이 버전에서 쓸 수 없는 섹션은 0101이 보고하고 여기서는 내용을 따지지 않는다(Vector `Format/0201`)
            if !section_allowed(&sec.name, proto) {
                continue;
            }
            let parsed = parse(&sec.text);
            let src = sec.text.as_str();
            let Some(Elem::Block(top)) = parsed.file.stmts.first().and_then(|s| s.elems.get(1))
            else {
                continue;
            };
            for cfg in &top.stmts {
                // `configuration <이름> { <합성 노드> { <논리 노드>, …; } … }`
                let cfg_name = match cfg.elems.get(1) {
                    Some(Elem::Token(t)) if t.kind == TokenKind::Ident => t.span.text(src),
                    _ => continue,
                };
                let Some(Elem::Block(body)) = cfg.elems.last() else {
                    continue;
                };
                let mut seen: BTreeSet<String> = BTreeSet::new();
                for comp in &body.stmts {
                    let mut names = Vec::new();
                    idents(src, comp, &mut names);
                    // 첫 식별자가 합성 노드, 나머지가 논리 노드
                    for (i, name) in names.iter().enumerate() {
                        if !seen.insert(name.clone()) {
                            out.push(diag(
                                Severity::Error,
                                109,
                                sec.span,
                                format!(
                                    "구성 `{cfg_name}`에서 노드 이름 `{name}`이(가) 두 번 나온다"
                                ),
                            ));
                        }
                        if i > 0 && !described.contains(name) {
                            out.push(diag(
                                Severity::Error,
                                109,
                                sec.span,
                                format!(
                                    "구성 `{cfg_name}`의 논리 노드 `{name}`이(가) 노드 속성에 없다"
                                ),
                            ));
                        }
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
