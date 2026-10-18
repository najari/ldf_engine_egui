//! 규칙 0404: 노드의 `fault_state_signals`가 그 노드가 보내는 프레임에 들어 있지 않다.

use super::r0402_publisher_mapping::sent_by;
use crate::Rule;
use crate::util::{diag, node_label, signal_label};
use ldf_model::{Diagnostic, LdfModel, Severity};

/// 규칙 0404.
pub(crate) struct R0404;
/// 등록용 인스턴스.
pub(crate) static RULE: R0404 = R0404;

impl Rule for R0404 {
    fn code(&self) -> u16 {
        404
    }

    fn title(&self) -> &'static str {
        "Fault state signal not sent by the node"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        for a in &m.node_attrs {
            let Some(node) = a.node.id() else { continue };
            for r in &a.fault_state_signals {
                if let Some(sid) = r.id()
                    && !sent_by(m, node, sid)
                {
                    out.push(diag(
                        Severity::Warning,
                        404,
                        a.span,
                        format!(
                            "노드 `{}`의 fault_state_signals `{}`을(를) 그 노드가 보내지 않는다",
                            node_label(m, &a.node),
                            signal_label(m, r)
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

    #[test]
    fn fault_state_must_be_sent_by_the_node() {
        let base = "Nodes { Master: M, 1 ms, 0 ms; Slaves: S, T; }\n\
                    Signals { E: 1, 0, S, M; F: 1, 0, T, M; }\n\
                    Frames { FE: 2, S, 1 { E, 0; } FF: 3, T, 1 { F, 0; } }\n";
        let n = |sig: &str| {
            let mut out = Vec::new();
            RULE.check(
                &load(&format!(
                    "{base}Node_attributes {{ S {{ fault_state_signals = {sig}; }} }}"
                ))
                .model,
                &mut out,
            );
            out.len()
        };
        assert_eq!(n("E"), 0);
        assert_eq!(n("F"), 1);
    }
}
