//! 규칙 0020: LDF의 기본 구조 오류(필수 선언 누락, 이름 중복, 정의되지 않은 대상 참조, 종류가 맞지 않는 참조).
//!
//! 이 규칙이 하나라도 결과를 내면 모델을 믿고 돌릴 수 없으므로 나머지 규칙은 실행되지 않는다
//! (`gate` 모듈: Vector도 이때 모든 검사를 "수행하지 못함"으로 보고한다).

use crate::Rule;
use crate::util::{diag, frame_label, node_label, signal_label};
use ldf_model::{
    Diagnostic, FrameKind, LdfModel, NodeRole, Ref, Severity, SignalKind, SlotEntry, Span,
};
use std::collections::HashSet;

/// 규칙 0020.
pub(crate) struct R0020;
/// 등록용 인스턴스.
pub(crate) static RULE: R0020 = R0020;

fn err(out: &mut Vec<Diagnostic>, span: Option<Span>, msg: String) {
    out.push(diag(Severity::Error, 20, span, msg));
}

fn dup_names<'a>(
    out: &mut Vec<Diagnostic>,
    kind: &str,
    items: impl Iterator<Item = (&'a str, Option<Span>)>,
) {
    let mut seen = HashSet::new();
    for (name, span) in items {
        if !seen.insert(name) {
            err(out, span, format!("{kind} 이름 `{name}`이 중복된다"));
        }
    }
}

impl Rule for R0020 {
    fn code(&self) -> u16 {
        20
    }

    fn title(&self) -> &'static str {
        "Invalid LDF structure"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        if !m.header.has_description_marker {
            err(out, None, "`LIN_description_file;` 선언이 없다".to_owned());
        }

        dup_names(
            out,
            "노드",
            m.nodes.iter().map(|(_, n)| (n.name.as_str(), n.span)),
        );
        dup_names(
            out,
            "시그널",
            m.signals.iter().map(|(_, s)| (s.name.as_str(), s.span)),
        );
        dup_names(
            out,
            "프레임",
            m.frames.iter().map(|(_, f)| (f.name.as_str(), f.span)),
        );
        dup_names(
            out,
            "인코딩 타입",
            m.encodings.iter().map(|(_, e)| (e.name.as_str(), e.span)),
        );
        dup_names(
            out,
            "스케줄 테이블",
            m.schedules.iter().map(|(_, s)| (s.name.as_str(), s.span)),
        );

        for (_, n) in m.nodes.iter() {
            if n.role == NodeRole::Master && (n.timebase_ms.is_none() || n.jitter_ms.is_none()) {
                err(
                    out,
                    n.span,
                    format!(
                        "마스터 `{}`의 선언이 `이름, 타임베이스 ms, 지터 ms` 형식이 아니다",
                        n.name
                    ),
                );
            }
        }

        for (_, s) in m.signals.iter() {
            if s.kind == SignalKind::Normal {
                match &s.publisher {
                    None => err(out, s.span, format!("시그널 `{}`의 발행자가 없다", s.name)),
                    Some(r @ Ref::Unresolved(_)) => err(
                        out,
                        s.span,
                        format!(
                            "시그널 `{}`의 발행자 `{}`이(가) 정의되지 않았다",
                            s.name,
                            node_label(m, r)
                        ),
                    ),
                    Some(Ref::Resolved(_)) => {}
                }
                for r in s.subscribers.iter().filter(|r| r.id().is_none()) {
                    err(
                        out,
                        s.span,
                        format!(
                            "시그널 `{}`의 구독자 `{}`이(가) 정의되지 않았다",
                            s.name,
                            node_label(m, r)
                        ),
                    );
                }
            }
        }

        let is_unconditional = |r: &Ref<ldf_model::FrameId>| {
            r.id()
                .and_then(|id| m.frames.get(id))
                .is_some_and(|f| matches!(f.kind, FrameKind::Unconditional { .. }))
        };

        for (_, f) in m.frames.iter() {
            match &f.kind {
                FrameKind::Unconditional {
                    publisher, signals, ..
                } => {
                    if publisher.id().is_none() {
                        err(
                            out,
                            f.span,
                            format!(
                                "프레임 `{}`의 발행자 `{}`이(가) 정의되지 않았다",
                                f.name,
                                node_label(m, publisher)
                            ),
                        );
                    }
                    for fs in signals.iter().filter(|fs| fs.signal.id().is_none()) {
                        err(
                            out,
                            f.span,
                            format!(
                                "프레임 `{}`이(가) 정의되지 않은 시그널 `{}`을(를) 배치한다",
                                f.name,
                                signal_label(m, &fs.signal)
                            ),
                        );
                    }
                }
                FrameKind::Diagnostic { signals, .. } => {
                    for fs in signals.iter().filter(|fs| fs.signal.id().is_none()) {
                        err(
                            out,
                            f.span,
                            format!(
                                "진단 프레임 `{}`이(가) 정의되지 않은 시그널 `{}`을(를) 배치한다",
                                f.name,
                                signal_label(m, &fs.signal)
                            ),
                        );
                    }
                }
                FrameKind::Sporadic { associated } => {
                    for r in associated {
                        if r.id().is_none() {
                            err(
                                out,
                                f.span,
                                format!(
                                    "스포라딕 프레임 `{}`의 연관 프레임 `{}`이(가) 정의되지 않았다",
                                    f.name,
                                    frame_label(m, r)
                                ),
                            );
                        } else if !is_unconditional(r) {
                            err(
                                out,
                                f.span,
                                format!(
                                    "스포라딕 프레임 `{}`의 연관 프레임 `{}`이(가) 무조건 프레임이 아니다",
                                    f.name,
                                    frame_label(m, r)
                                ),
                            );
                        }
                    }
                }
                FrameKind::EventTriggered {
                    collision_table,
                    associated,
                    ..
                } => {
                    if let Some(t @ Ref::Unresolved(name)) = collision_table {
                        let _ = t;
                        err(
                            out,
                            f.span,
                            format!(
                                "이벤트 트리거 프레임 `{}`의 충돌 해결 테이블 `{name}`이(가) 정의되지 않았다",
                                f.name
                            ),
                        );
                    }
                    for r in associated {
                        if r.id().is_none() {
                            err(
                                out,
                                f.span,
                                format!(
                                    "이벤트 트리거 프레임 `{}`의 연관 프레임 `{}`이(가) 정의되지 않았다",
                                    f.name,
                                    frame_label(m, r)
                                ),
                            );
                        } else if !is_unconditional(r) {
                            err(
                                out,
                                f.span,
                                format!(
                                    "이벤트 트리거 프레임 `{}`의 연관 프레임 `{}`이(가) 무조건 프레임이 아니다",
                                    f.name,
                                    frame_label(m, r)
                                ),
                            );
                        }
                    }
                }
            }
        }

        for a in &m.node_attrs {
            if a.node.id().is_none() {
                err(
                    out,
                    a.span,
                    format!(
                        "노드 속성의 대상 노드 `{}`이(가) 정의되지 않았다",
                        node_label(m, &a.node)
                    ),
                );
            }
            if let Some(r) = a.response_error.as_ref().filter(|r| r.id().is_none()) {
                err(
                    out,
                    a.span,
                    format!(
                        "response_error 시그널 `{}`이(가) 정의되지 않았다",
                        signal_label(m, r)
                    ),
                );
            }
            for r in a.fault_state_signals.iter().filter(|r| r.id().is_none()) {
                err(
                    out,
                    a.span,
                    format!(
                        "fault_state_signals의 시그널 `{}`이(가) 정의되지 않았다",
                        signal_label(m, r)
                    ),
                );
            }
            for c in a
                .configurable_frames
                .iter()
                .filter(|c| c.frame.id().is_none())
            {
                err(
                    out,
                    a.span,
                    format!(
                        "configurable_frames의 프레임 `{}`이(가) 정의되지 않았다",
                        frame_label(m, &c.frame)
                    ),
                );
            }
        }

        let iso = crate::util::file_proto(m) == crate::util::Proto::Iso;
        let mut diag_scheduled = false;
        for (_, t) in m.schedules.iter() {
            for slot in &t.slots {
                match &slot.entry {
                    SlotEntry::Frame(r) => {
                        match r.id().and_then(|id| m.frames.get(id)) {
                            // ISO 17987은 진단 프레임을 선언하지 않아도 이름으로 스케줄에 넣을 수 있다
                            None if iso
                                && matches!(
                                    frame_label(m, r).as_str(),
                                    "MasterReq" | "SlaveResp"
                                ) =>
                            {
                                diag_scheduled = true;
                            }
                            None => err(
                                out,
                                slot.span,
                                format!(
                                    "스케줄 `{}`의 프레임 `{}`이(가) 정의되지 않았다",
                                    t.name,
                                    frame_label(m, r)
                                ),
                            ),
                            Some(f) => {
                                diag_scheduled |= matches!(f.kind, FrameKind::Diagnostic { .. });
                            }
                        }
                    }
                    SlotEntry::AssignNad { node }
                    | SlotEntry::DataDump { node, .. }
                    | SlotEntry::SaveConfiguration { node }
                    | SlotEntry::AssignFrameIdRange { node, .. } => {
                        if node.id().is_none() {
                            err(
                                out,
                                slot.span,
                                format!(
                                    "스케줄 `{}`의 명령이 정의되지 않은 노드 `{}`을(를) 가리킨다",
                                    t.name,
                                    node_label(m, node)
                                ),
                            );
                        }
                    }
                    SlotEntry::AssignFrameId { node, frame } => {
                        if node.id().is_none() {
                            err(
                                out,
                                slot.span,
                                format!(
                                    "스케줄 `{}`의 AssignFrameId가 정의되지 않은 노드 `{}`을(를) 가리킨다",
                                    t.name,
                                    node_label(m, node)
                                ),
                            );
                        }
                        match frame.id().and_then(|id| m.frames.get(id)) {
                            None => err(
                                out,
                                slot.span,
                                format!(
                                    "스케줄 `{}`의 AssignFrameId가 정의되지 않은 프레임 `{}`을(를) 가리킨다",
                                    t.name,
                                    frame_label(m, frame)
                                ),
                            ),
                            Some(f) if matches!(f.kind, FrameKind::Sporadic { .. }) => err(
                                out,
                                slot.span,
                                format!(
                                    "스케줄 `{}`의 AssignFrameId 대상 `{}`은(는) 프레임 ID가 없는 스포라딕 프레임이다",
                                    t.name, f.name
                                ),
                            ),
                            Some(_) => {}
                        }
                    }
                    SlotEntry::ConditionalChangeNad { .. }
                    | SlotEntry::FreeFormat { .. }
                    | SlotEntry::Unknown(_) => {}
                }
            }
        }

        // 진단 프레임(MasterReq/SlaveResp)이 스케줄에 있는데 일반 프레임이 그 ID(0x3C, 0x3D)를 쓰면 모호하다
        if diag_scheduled {
            for (_, f) in m.frames.iter() {
                if let FrameKind::Unconditional { id, .. } = &f.kind
                    && matches!(*id, 0x3c | 0x3d)
                {
                    err(
                        out,
                        f.span,
                        format!(
                            "프레임 `{}`이(가) 진단 프레임 ID 0x{id:02X}를 쓰는데 진단 프레임이 스케줄에 있다",
                            f.name
                        ),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::load;

    fn run(src: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        RULE.check(&load(src).model, &mut out);
        out
    }

    const BASE: &str = r#"
LIN_description_file;
LIN_protocol_version = "2.1";
LIN_language_version = "2.1";
LIN_speed = 19.2 kbps;
Nodes { Master: M, 1 ms, 0 ms; Slaves: S; }
Signals { A: 8, 0, M, S; }
Frames { F: 1, M, 1 { A, 0; } }
Schedule_tables { T { F delay 10 ms; } }
"#;

    #[test]
    fn consistent_file_is_clean() {
        assert!(run(BASE).is_empty());
    }

    #[test]
    fn duplicate_and_unresolved_are_found() {
        let dup = BASE.replace("A: 8, 0, M, S;", "A: 8, 0, M, S; A: 8, 0, M, S;");
        assert_eq!(run(&dup).len(), 1);
        let ghost = BASE.replace("A, 0;", "Ghost, 0;");
        assert_eq!(run(&ghost).len(), 1);
        let no_marker = BASE.replace("LIN_description_file;", "");
        assert_eq!(run(&no_marker).len(), 1);
        let slot = BASE.replace("F delay", "Nope delay");
        assert_eq!(run(&slot).len(), 1);
    }

    #[test]
    fn master_without_jitter_is_malformed() {
        let src = BASE.replace("1 ms, 0 ms", "1 ms, x ms");
        assert_eq!(run(&src).len(), 1);
    }
}
