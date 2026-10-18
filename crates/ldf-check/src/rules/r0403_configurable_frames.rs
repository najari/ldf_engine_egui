//! 규칙 0403: 슬레이브의 `configurable_frames`가 그 노드가 실제로 쓰는 프레임과 맞지 않는다
//! (쓰는 프레임이 빠졌거나, 쓰지 않는 프레임이 들어 있다).
//!
//! 노드가 쓰는 프레임 = 직접 발행하는 무조건 프레임, 구독하는 시그널이 들어 있는 무조건 프레임,
//! 그런 프레임을 연관 프레임으로 가진 이벤트 트리거 프레임.

use crate::Rule;
use crate::util::{diag, frame_label, node_label};
use ldf_model::{Diagnostic, FrameId, FrameKind, LdfModel, NodeId, NodeRole, Ref, Severity};
use std::collections::BTreeSet;

/// 규칙 0403.
pub(crate) struct R0403;
/// 등록용 인스턴스.
pub(crate) static RULE: R0403 = R0403;

/// `node`가 발행하거나 구독하는 무조건 프레임, 그리고 그것을 포함하는 이벤트 트리거 프레임.
pub(crate) fn frames_used_by(m: &LdfModel, node: NodeId) -> BTreeSet<FrameId> {
    let mut used = BTreeSet::new();
    for (fid, f) in m.frames.iter() {
        let FrameKind::Unconditional {
            publisher, signals, ..
        } = &f.kind
        else {
            continue;
        };
        let publishes = publisher.id() == Some(node);
        let subscribes = signals.iter().any(|fs| {
            fs.signal
                .id()
                .and_then(|sid| m.signals.get(sid))
                .is_some_and(|s| s.subscribers.iter().any(|r| r.id() == Some(node)))
        });
        if publishes || subscribes {
            used.insert(fid);
        }
    }
    for (fid, f) in m.frames.iter() {
        if let FrameKind::EventTriggered { associated, .. } = &f.kind
            && associated
                .iter()
                .any(|r| r.id().is_some_and(|a| used.contains(&a)))
        {
            used.insert(fid);
        }
    }
    used
}

impl Rule for R0403 {
    fn code(&self) -> u16 {
        403
    }

    fn title(&self) -> &'static str {
        "Configurable frames do not match the frames the node uses"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        for a in &m.node_attrs {
            let Some(nid) = a.node.id() else { continue };
            let is_slave = m.nodes.get(nid).is_some_and(|n| n.role == NodeRole::Slave);
            // 목록이 아예 없는 노드는 0104가 다룬다
            if !is_slave || a.configurable_frames.is_empty() {
                continue;
            }
            let used = frames_used_by(m, nid);
            let listed: BTreeSet<FrameId> = a
                .configurable_frames
                .iter()
                .filter_map(|c| c.frame.id())
                .collect();
            let name = node_label(m, &a.node);
            for fid in used.difference(&listed) {
                out.push(diag(
                    Severity::Warning,
                    403,
                    a.span,
                    format!(
                        "노드 `{name}`이(가) 쓰는 프레임 `{}`이(가) configurable_frames에 없다",
                        frame_label(m, &Ref::Resolved(*fid))
                    ),
                ));
            }
            for c in &a.configurable_frames {
                let Some(fid) = c.frame.id() else { continue };
                // 스포라딕·진단 프레임은 구성 대상이 아니므로 판단하지 않는다
                let relevant = m.frames.get(fid).is_some_and(|f| {
                    matches!(
                        f.kind,
                        FrameKind::Unconditional { .. } | FrameKind::EventTriggered { .. }
                    )
                });
                if relevant && !used.contains(&fid) {
                    out.push(diag(
                        Severity::Warning,
                        403,
                        a.span,
                        format!(
                            "노드 `{name}`이(가) 쓰지 않는 프레임 `{}`이(가) configurable_frames에 있다",
                            frame_label(m, &c.frame)
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

    fn count(cf: &str) -> usize {
        let src = format!(
            "LIN_protocol_version = \"2.2\";\n\
             Nodes {{ Master: M, 1 ms, 0 ms; Slaves: S, T; }}\n\
             Signals {{ A: 8, 0, M, S; B: 8, 0, S, M; C: 8, 0, T, M; }}\n\
             Frames {{ F1: 1, M, 1 {{ A, 0; }} F2: 2, S, 1 {{ B, 0; }} F3: 3, T, 1 {{ C, 0; }} }}\n\
             Node_attributes {{ S {{ LIN_protocol = \"2.2\"; configurable_frames {{ {cf} }} }} }}"
        );
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        out.len()
    }

    #[test]
    fn missing_and_extra_frames_are_found() {
        assert_eq!(count("F1; F2;"), 0);
        assert_eq!(count("F1;"), 1); // F2 누락
        assert_eq!(count("F1; F2; F3;"), 1); // F3 불필요
        assert_eq!(count(""), 0); // 빈 목록은 0104의 몫
    }
}
