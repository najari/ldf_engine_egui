//! 규칙 0401: 어느 프레임에도 배치되지 않은 시그널.

use crate::Rule;
use crate::util::diag;
use ldf_model::{Diagnostic, FrameKind, LdfModel, Severity, SignalId, SignalKind};
use std::collections::BTreeSet;

/// 규칙 0401.
pub(crate) struct R0401;
/// 등록용 인스턴스.
pub(crate) static RULE: R0401 = R0401;

/// 어떤 프레임에든 배치된 시그널.
pub(crate) fn mapped(m: &LdfModel) -> BTreeSet<SignalId> {
    m.frames
        .iter()
        .flat_map(|(_, f)| match &f.kind {
            FrameKind::Unconditional { signals, .. } | FrameKind::Diagnostic { signals, .. } => {
                signals
                    .iter()
                    .filter_map(|s| s.signal.id())
                    .collect::<Vec<_>>()
            }
            _ => Vec::new(),
        })
        .collect()
}

impl Rule for R0401 {
    fn code(&self) -> u16 {
        401
    }

    fn title(&self) -> &'static str {
        "Signal not mapped to a frame"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        let placed = mapped(m);
        for (sid, s) in m.signals.iter() {
            if s.kind == SignalKind::Normal && !placed.contains(&sid) {
                out.push(diag(
                    Severity::Warning,
                    401,
                    s.span,
                    format!("시그널 `{}`이(가) 어느 프레임에도 배치되지 않았다", s.name),
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
    fn unplaced_signals() {
        let src = "Nodes { Master: M, 1 ms, 0 ms; Slaves: S; }\nSignals { A: 8, 0, M, S; B: 8, 0, M, S; }\n\
                   Frames { F: 1, M, 1 { A, 0; } }";
        let mut out = Vec::new();
        RULE.check(&load(src).model, &mut out);
        assert_eq!(out.len(), 1);
        assert!(out[0].message.contains('B'));
    }
}
