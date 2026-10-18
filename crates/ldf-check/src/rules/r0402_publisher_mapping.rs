//! 규칙 0402: 시그널을 발행하는 노드가 그 시그널을 담은 프레임을 보내지 않는다.
//! 노드의 `response_error` 시그널도 그 노드가 보내는 프레임에 들어 있어야 한다.

use crate::Rule;
use crate::util::{diag, node_label, signal_label};
use ldf_model::{Diagnostic, FrameKind, LdfModel, NodeId, Ref, Severity, SignalId, SignalKind};

/// 규칙 0402.
pub(crate) struct R0402;
/// 등록용 인스턴스.
pub(crate) static RULE: R0402 = R0402;

/// `node`가 발행하는 무조건 프레임 중 `signal`을 담은 것이 있는지.
pub(crate) fn sent_by(m: &LdfModel, node: NodeId, signal: SignalId) -> bool {
    m.frames.iter().any(|(_, f)| match &f.kind {
        FrameKind::Unconditional {
            publisher, signals, ..
        } => publisher.id() == Some(node) && signals.iter().any(|s| s.signal.id() == Some(signal)),
        _ => false,
    })
}

impl Rule for R0402 {
    fn code(&self) -> u16 {
        402
    }

    fn title(&self) -> &'static str {
        "Signal not sent by its publisher"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        if super::attribute_format_cascade(m).is_some() {
            out.push(super::cascade_diag(402, Severity::Warning));
        }
        for (sid, s) in m.signals.iter() {
            if s.kind != SignalKind::Normal {
                continue;
            }
            let Some(p) = s.publisher.as_ref().and_then(Ref::id) else {
                continue;
            };
            if !sent_by(m, p, sid) {
                out.push(diag(
                    Severity::Warning,
                    402,
                    s.span,
                    format!(
                        "시그널 `{}`을(를) 발행하는 `{}`이(가) 그 시그널을 담은 프레임을 보내지 않는다",
                        s.name,
                        node_label(m, s.publisher.as_ref().unwrap_or(&Ref::Unresolved(String::new())))
                    ),
                ));
            }
        }
        for a in &m.node_attrs {
            let (Some(node), Some(r)) = (a.node.id(), a.response_error.as_ref()) else {
                continue;
            };
            if let Some(sid) = r.id()
                && !sent_by(m, node, sid)
            {
                out.push(diag(
                    Severity::Warning,
                    402,
                    a.span,
                    format!(
                        "노드 `{}`의 response_error 시그널 `{}`을(를) 그 노드가 보내지 않는다",
                        node_label(m, &a.node),
                        signal_label(m, r)
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

    #[test]
    fn publisher_and_response_error() {
        let base = "Nodes { Master: M, 1 ms, 0 ms; Slaves: S, T; }\n\
                    Signals { A: 8, 0, M, S; E: 1, 0, S, M; F: 1, 0, T, M; }\n\
                    Frames { FA: 1, M, 1 { A, 0; } FE: 2, S, 1 { E, 0; } FF: 3, T, 1 { F, 0; } }\n";
        let n = |attrs: &str| {
            let mut out = Vec::new();
            RULE.check(&load(&format!("{base}{attrs}")).model, &mut out);
            out.len()
        };
        assert_eq!(n("Node_attributes { S { response_error = E; } }"), 0);
        assert_eq!(n("Node_attributes { S { response_error = F; } }"), 1);
        // 시그널을 빼면 발행자가 보내지 않는 시그널이 된다
        let mut out = Vec::new();
        RULE.check(
            &load("Nodes { Master: M, 1 ms, 0 ms; Slaves: S; }\nSignals { A: 8, 0, M, S; }\nFrames { FA: 1, M, 1 { } }").model,
            &mut out,
        );
        assert_eq!(out.len(), 1);
    }
}
