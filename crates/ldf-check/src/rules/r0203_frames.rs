//! 규칙 0203: 프레임 정의의 모델링 오류.
//!
//! - 어느 스케줄 테이블에도 들어 있지 않은 프레임.
//! - 프레임 ID 중복(LIN 2.0 `dynamic_frames`에 선언된 ID는 제외).
//! - 시그널이 하나도 없는 무조건 프레임.
//! - 이벤트 트리거 프레임의 연관 프레임이 서로 길이가 다르거나 체크섬 모델(클래식/향상)이 다르다.
//! - 스포라딕 프레임의 연관 프레임이 마스터가 보내는 프레임이 아니다.

use crate::Rule;
use crate::raw::dynamic_frame_ids;
use crate::util::{Proto, attr_proto, diag, frame_label, is_master};
use ldf_model::{Diagnostic, FrameId, FrameKind, LdfModel, Ref, Severity, SlotEntry};
use std::collections::{BTreeMap, BTreeSet};

/// 규칙 0203.
pub(crate) struct R0203;
/// 등록용 인스턴스.
pub(crate) static RULE: R0203 = R0203;

/// 연관 프레임을 발행하는 노드가 클래식 체크섬(LIN 1.3)을 쓰는지.
fn classic(m: &LdfModel, f: FrameId) -> bool {
    let Some(FrameKind::Unconditional { publisher, .. }) = m.frames.get(f).map(|f| &f.kind) else {
        return false;
    };
    m.node_attrs
        .iter()
        .find(|a| a.node == *publisher)
        .is_some_and(|a| attr_proto(m, a.lin_protocol.as_deref()) == Proto::V1_3)
}

impl Rule for R0203 {
    fn code(&self) -> u16 {
        203
    }

    fn title(&self) -> &'static str {
        "Frame definition inconsistent"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        let mut err = |span, msg: String| out.push(diag(Severity::Error, 203, span, msg));

        // 스케줄에 없는 프레임
        let mut scheduled: BTreeSet<FrameId> = BTreeSet::new();
        for (_, t) in m.schedules.iter() {
            for slot in &t.slots {
                if let SlotEntry::Frame(r) = &slot.entry
                    && let Some(fid) = r.id()
                {
                    scheduled.insert(fid);
                    if let Some(
                        FrameKind::Sporadic { associated }
                        | FrameKind::EventTriggered { associated, .. },
                    ) = m.frames.get(fid).map(|f| &f.kind)
                    {
                        scheduled.extend(associated.iter().filter_map(Ref::id));
                    }
                }
            }
        }
        for (fid, f) in m.frames.iter() {
            let relevant = matches!(
                f.kind,
                FrameKind::Unconditional { .. }
                    | FrameKind::Sporadic { .. }
                    | FrameKind::EventTriggered { .. }
            );
            if relevant && !scheduled.contains(&fid) {
                err(
                    f.span,
                    format!("프레임 `{}`이(가) 스케줄되지 않는다", f.name),
                );
            }
        }

        // 프레임 ID 중복
        let dynamic: BTreeSet<u64> = dynamic_frame_ids(m).into_iter().collect();
        let mut by_id: BTreeMap<u32, Vec<FrameId>> = BTreeMap::new();
        for (fid, f) in m.frames.iter() {
            if matches!(
                f.kind,
                FrameKind::Unconditional { .. } | FrameKind::EventTriggered { .. }
            ) && let Some(id) = f.lin_id()
                && !dynamic.contains(&u64::from(id))
            {
                by_id.entry(id).or_default().push(fid);
            }
        }
        for (id, frames) in &by_id {
            if frames.len() > 1 {
                for fid in frames {
                    err(
                        m.frames.get(*fid).and_then(|f| f.span),
                        format!(
                            "프레임 ID {id:#04X}를 프레임 `{}`을(를) 포함해 {}개가 함께 쓴다",
                            frame_label(m, &Ref::Resolved(*fid)),
                            frames.len()
                        ),
                    );
                }
            }
        }

        for (_, f) in m.frames.iter() {
            match &f.kind {
                FrameKind::Unconditional { signals, .. } if signals.is_empty() => {
                    err(
                        f.span,
                        format!("무조건 프레임 `{}`에 시그널이 없다", f.name),
                    );
                }
                FrameKind::EventTriggered { associated, .. } => {
                    let ids: Vec<FrameId> = associated.iter().filter_map(Ref::id).collect();
                    let sizes: BTreeSet<u32> = ids
                        .iter()
                        .filter_map(|id| m.frames.get(*id).and_then(|f| f.size_bytes()))
                        .collect();
                    if sizes.len() > 1 {
                        err(
                            f.span,
                            format!(
                                "이벤트 트리거 프레임 `{}`의 연관 프레임 길이가 서로 다르다",
                                f.name
                            ),
                        );
                    }
                    let kinds: BTreeSet<bool> = ids.iter().map(|id| classic(m, *id)).collect();
                    if kinds.len() > 1 {
                        err(
                            f.span,
                            format!(
                                "이벤트 트리거 프레임 `{}`의 연관 프레임이 서로 다른 체크섬 모델을 쓴다",
                                f.name
                            ),
                        );
                    }
                }
                FrameKind::Sporadic { associated } => {
                    for r in associated {
                        let from_master =
                            r.id()
                                .and_then(|id| m.frames.get(id))
                                .is_some_and(|a| match &a.kind {
                                    FrameKind::Unconditional { publisher, .. } => {
                                        is_master(m, publisher)
                                    }
                                    _ => false,
                                });
                        if r.id().is_some() && !from_master {
                            err(
                                f.span,
                                format!(
                                    "스포라딕 프레임 `{}`의 연관 프레임 `{}`을(를) 마스터가 보내지 않는다",
                                    f.name,
                                    frame_label(m, r)
                                ),
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::load;

    fn n(frames: &str, schedule: &str) -> usize {
        let src = format!(
            "LIN_protocol_version = \"2.2\";\n\
             Nodes {{ Master: M, 1 ms, 0 ms; Slaves: S; }}\n\
             Signals {{ A: 8, 0, M, S; B: 8, 0, M, S; C: 8, 0, S, M; }}\n\
             Frames {{ {frames} }}\nSchedule_tables {{ T {{ {schedule} }} }}"
        );
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        out.len()
    }

    #[test]
    fn duplicate_ids_empty_frames_and_unscheduled() {
        let ok = "F1: 1, M, 1 { A, 0; } F2: 2, M, 1 { B, 0; }";
        let both = "F1 delay 10 ms; F2 delay 10 ms;";
        assert_eq!(n(ok, both), 0);
        assert_eq!(n("F1: 1, M, 1 { A, 0; } F2: 1, M, 1 { B, 0; }", both), 2);
        assert_eq!(n("F1: 1, M, 1 { A, 0; } F2: 2, M, 1 { }", both), 1);
        assert_eq!(n(ok, "F1 delay 10 ms;"), 1);
    }

    #[test]
    fn duplicate_ids_are_allowed_when_declared_in_dynamic_frames() {
        let src = |dynamic: &str| {
            format!(
                "LIN_protocol_version = \"2.0\";\n\
                 Nodes {{ Master: M, 1 ms, 0 ms; Slaves: S; }}\n\
                 Signals {{ A: 8, 0, M, S; B: 8, 0, M, S; }}\n{dynamic}\n\
                 Frames {{ F1: 1, M, 1 {{ A, 0; }} F2: 1, M, 1 {{ B, 0; }} }}\n\
                 Schedule_tables {{ T {{ F1 delay 10 ms; F2 delay 10 ms; }} }}"
            )
        };
        let count = |text: &str| {
            let mut out = Vec::new();
            RULE.check(&load(text).model, &mut out);
            out.len()
        };
        assert_eq!(count(&src("")), 2);
        assert_eq!(count(&src("dynamic_frames { 1 }")), 0);
        // ID 목록이 아니라 원문으로 남은 섹션도 같다
        assert_eq!(count(&src("dynamic_frames { 1, Dyn }")), 0);
        assert_eq!(count(&src("dynamic_frames { 2 }")), 2);
    }

    #[test]
    fn sporadic_must_come_from_the_master() {
        let src = "LIN_protocol_version = \"2.2\";\nNodes { Master: M, 1 ms, 0 ms; Slaves: S; }\n\
                   Signals { A: 8, 0, M, S; C: 8, 0, S, M; }\n\
                   Frames { F1: 1, M, 1 { A, 0; } F2: 2, S, 1 { C, 0; } }\n\
                   Sporadic_frames { SP: F1, F2; }\nSchedule_tables { T { SP delay 10 ms; } }";
        let mut out = Vec::new();
        RULE.check(&load(src).model, &mut out);
        assert_eq!(out.len(), 1, "{out:?}");
    }
}
