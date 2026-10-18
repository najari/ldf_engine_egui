//! 규칙 0405: 시그널의 발행자·구독자가 모순이다.
//!
//! - 구독자가 없다.
//! - 발행자와 구독자가 같다.
//! - 바이트 배열 시그널을 LIN 1.3 노드가 발행하거나 구독한다.
//! - 프레임에 다른 노드가 발행하는 시그널이 들어 있다.

use crate::Rule;
use crate::util::{Proto, attr_proto, diag, node_label, same_node};
use ldf_model::{Diagnostic, FrameKind, InitValue, LdfModel, Ref, Severity, SignalKind};

/// 규칙 0405.
pub(crate) struct R0405;
/// 등록용 인스턴스.
pub(crate) static RULE: R0405 = R0405;

fn is_1_3(m: &LdfModel, node: &Ref<ldf_model::NodeId>) -> bool {
    m.node_attrs
        .iter()
        .find(|a| a.node == *node)
        .is_some_and(|a| {
            a.lin_protocol.is_some() && attr_proto(m, a.lin_protocol.as_deref()) == Proto::V1_3
        })
}

impl Rule for R0405 {
    fn code(&self) -> u16 {
        405
    }

    fn title(&self) -> &'static str {
        "Signal publisher/subscriber inconsistent"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        for (_, s) in m
            .signals
            .iter()
            .filter(|(_, s)| s.kind == SignalKind::Normal)
        {
            let mut warn = |msg: String| out.push(diag(Severity::Warning, 405, s.span, msg));
            if s.subscribers.is_empty() {
                warn(format!("시그널 `{}`에 구독자가 없다", s.name));
            }
            if let Some(p) = &s.publisher {
                if s.subscribers.iter().any(|r| same_node(p, r)) {
                    warn(format!(
                        "시그널 `{}`의 발행자 `{}`이(가) 구독자이기도 하다",
                        s.name,
                        node_label(m, p)
                    ));
                }
                if matches!(s.init, InitValue::Array(_))
                    && std::iter::once(p)
                        .chain(&s.subscribers)
                        .any(|r| is_1_3(m, r))
                {
                    warn(format!(
                        "바이트 배열 시그널 `{}`을(를) LIN 1.3 노드가 발행하거나 구독한다",
                        s.name
                    ));
                }
            }
        }
        for (_, f) in m.frames.iter() {
            let FrameKind::Unconditional {
                publisher, signals, ..
            } = &f.kind
            else {
                continue;
            };
            for fs in signals {
                let Some(sig) = fs.signal.id().and_then(|id| m.signals.get(id)) else {
                    continue;
                };
                if let Some(p) = &sig.publisher
                    && p.id().is_some()
                    && publisher.id().is_some()
                    && !same_node(p, publisher)
                {
                    out.push(diag(
                        Severity::Warning,
                        405,
                        f.span,
                        format!(
                            "프레임 `{}`({})에 `{}`이(가) 발행하는 시그널 `{}`이(가) 들어 있다",
                            f.name,
                            node_label(m, publisher),
                            node_label(m, p),
                            sig.name
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

    fn n(sigs: &str, frames: &str) -> usize {
        let src = format!(
            "Nodes {{ Master: M, 1 ms, 0 ms; Slaves: S, T; }}\nSignals {{ {sigs} }}\nFrames {{ {frames} }}"
        );
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        out.len()
    }

    #[test]
    fn publisher_subscriber_rules() {
        let f = "F: 1, M, 1 { A, 0; }";
        assert_eq!(n("A: 8, 0, M, S;", f), 0);
        assert_eq!(n("A: 8, 0, M;", f), 1);
        assert_eq!(n("A: 8, 0, M, M;", f), 1);
        // S가 발행하는 시그널을 마스터 프레임에 넣었다
        assert_eq!(n("A: 8, 0, S, M;", f), 1);
    }
}
