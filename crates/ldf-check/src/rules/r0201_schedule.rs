//! 규칙 0201: 스케줄 테이블의 모델링 오류.
//!
//! - 스케줄 테이블이 없거나, 슬롯이 없는 테이블이 있다.
//! - 어느 테이블에도 들어 있지 않은 프레임(스포라딕·이벤트 트리거 프레임에 연관된 프레임은 거기에 포함된 것으로 본다).
//! - 스포라딕·이벤트 트리거 프레임과 그 연관 프레임이 한 테이블에 함께 있다.
//! - 슬롯 delay가 프레임 최대 전송 시간보다 짧다.
//! - 이벤트 트리거 프레임의 충돌 해결 테이블이 연관 프레임을 모두 담지 않는다.
//! - `AssignFrameIdRange`의 인덱스·PID가 노드의 `configurable_frames`와 맞지 않는다.
//! - `AssignFrameId`가 노드의 `configurable_frames`에 없는 프레임을 가리킨다.
//! - LIN 2.0 노드에 보내면 안 되는 명령(`AssignFrameIdRange`, `SaveConfiguration`).
//! - 버전과 맞지 않아(0106) 해석할 수 없는 명령.

use super::r0106_commands::command_name;
use crate::Rule;
use crate::util::{Proto, attr_proto, diag, file_proto, frame_label, node_label};
use ldf_model::{
    Diagnostic, FrameId, FrameKind, LdfModel, NodeId, Ref, ScheduleId, Severity, SlotEntry,
    frame_time_max_ms, pid_is_valid, protected_id,
};
use std::collections::{BTreeMap, BTreeSet};

/// 규칙 0201.
pub(crate) struct R0201;
/// 등록용 인스턴스.
pub(crate) static RULE: R0201 = R0201;

fn err(out: &mut Vec<Diagnostic>, span: Option<ldf_model::Span>, msg: String) {
    out.push(diag(Severity::Error, 201, span, msg));
}

/// 노드의 `configurable_frames`(원문 순서)에 있는 해석된 프레임들.
fn configurable(m: &LdfModel, node: NodeId) -> Option<Vec<FrameId>> {
    let a = m.node_attrs.iter().find(|a| a.node.id() == Some(node))?;
    if a.configurable_frames.is_empty() {
        return None;
    }
    Some(
        a.configurable_frames
            .iter()
            .map(|c| c.frame.id())
            .collect::<Option<Vec<_>>>()?,
    )
}

fn associated_of(m: &LdfModel, f: FrameId) -> Vec<FrameId> {
    match m.frames.get(f).map(|f| &f.kind) {
        Some(FrameKind::Sporadic { associated } | FrameKind::EventTriggered { associated, .. }) => {
            associated.iter().filter_map(Ref::id).collect()
        }
        _ => Vec::new(),
    }
}

impl Rule for R0201 {
    fn code(&self) -> u16 {
        201
    }

    fn title(&self) -> &'static str {
        "Schedule table inconsistent"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        let file = file_proto(m);
        let has_frames = m.frames.iter().any(|(_, f)| {
            matches!(
                f.kind,
                FrameKind::Unconditional { .. }
                    | FrameKind::Sporadic { .. }
                    | FrameKind::EventTriggered { .. }
            )
        });
        if m.schedules.is_empty() {
            if has_frames {
                err(out, None, "스케줄 테이블이 하나도 없다".to_owned());
            }
            return;
        }

        // 테이블마다 들어 있는 프레임
        let mut in_table: BTreeMap<ScheduleId, BTreeSet<FrameId>> = BTreeMap::new();
        let mut scheduled: BTreeSet<FrameId> = BTreeSet::new();
        for (sid, t) in m.schedules.iter() {
            if t.slots.is_empty() {
                err(
                    out,
                    t.span,
                    format!("스케줄 테이블 `{}`에 슬롯이 없다", t.name),
                );
            }
            let set = in_table.entry(sid).or_default();
            for slot in &t.slots {
                if let SlotEntry::Frame(r) = &slot.entry
                    && let Some(fid) = r.id()
                {
                    set.insert(fid);
                    scheduled.insert(fid);
                    scheduled.extend(associated_of(m, fid));
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
                    out,
                    f.span,
                    format!("프레임 `{}`이(가) 어느 스케줄 테이블에도 없다", f.name),
                );
            }
        }

        let speed = m.header.speed_bps.unwrap_or(0);
        for (sid, t) in m.schedules.iter() {
            let here = in_table.get(&sid);
            for slot in &t.slots {
                match &slot.entry {
                    SlotEntry::Frame(r) => {
                        let Some(fid) = r.id() else { continue };
                        // 연관 프레임이 자기를 묶는 스포라딕·이벤트 트리거 프레임과 같은 테이블에 있는지
                        for (gid, g) in m.frames.iter() {
                            let group = matches!(
                                g.kind,
                                FrameKind::Sporadic { .. } | FrameKind::EventTriggered { .. }
                            );
                            if group
                                && gid != fid
                                && associated_of(m, gid).contains(&fid)
                                && here.is_some_and(|h| h.contains(&gid))
                            {
                                err(
                                    out,
                                    slot.span,
                                    format!(
                                        "스케줄 `{}`에 프레임 `{}`이(가) 자기를 묶는 `{}`와 함께 있다",
                                        t.name,
                                        frame_label(m, r),
                                        g.name
                                    ),
                                );
                            }
                        }
                        // delay가 최대 프레임 시간보다 짧다
                        let size = m.frames.get(fid).and_then(|f| f.size_bytes());
                        if let Some(size) = size
                            && let Some(tmax) = frame_time_max_ms(size, speed)
                            && slot.delay_ms + 1e-9 < tmax
                        {
                            err(
                                out,
                                slot.span,
                                format!(
                                    "스케줄 `{}`의 프레임 `{}` delay {} ms가 최대 프레임 시간 {tmax:.3} ms보다 짧다",
                                    t.name,
                                    frame_label(m, r),
                                    slot.delay_ms
                                ),
                            );
                        }
                    }
                    SlotEntry::AssignFrameIdRange {
                        node,
                        start_index,
                        pids,
                    } => {
                        if let Some(nid) = node.id() {
                            self.frame_id_range(
                                m,
                                out,
                                t.name.as_str(),
                                slot.span,
                                nid,
                                *start_index,
                                pids,
                            );
                        }
                    }
                    SlotEntry::AssignFrameId { node, frame } => {
                        if let (Some(nid), Some(fid)) = (node.id(), frame.id())
                            && let Some(list) = configurable(m, nid)
                            && !list.contains(&fid)
                        {
                            err(
                                out,
                                slot.span,
                                format!(
                                    "스케줄 `{}`의 AssignFrameId 대상 `{}`이(가) 노드 `{}`의 configurable_frames에 없다",
                                    t.name,
                                    frame_label(m, frame),
                                    node_label(m, node)
                                ),
                            );
                        }
                    }
                    _ => {}
                }
                // LIN 2.0 노드에 2.1 이후 명령을 보낸다
                let target = match &slot.entry {
                    SlotEntry::AssignFrameIdRange { node, .. }
                    | SlotEntry::SaveConfiguration { node } => Some(node),
                    _ => None,
                };
                if let Some(node) = target
                    && let Some(a) = m.node_attrs.iter().find(|a| a.node == *node)
                    && attr_proto(m, a.lin_protocol.as_deref()) == Proto::V2_0
                {
                    err(
                        out,
                        slot.span,
                        format!(
                            "스케줄 `{}`이(가) LIN 2.0 노드 `{}`에 2.1 이후 명령을 보낸다",
                            t.name,
                            node_label(m, node)
                        ),
                    );
                }
                // 이 버전에서 쓸 수 없는 명령은 해석되지 않는다(0106)
                if let Some((name, legacy)) = command_name(&slot.entry)
                    && !super::r0106_commands::allowed(&name, legacy, file)
                {
                    err(
                        out,
                        slot.span,
                        format!("스케줄 `{}`의 명령 {name}을(를) 해석할 수 없다", t.name),
                    );
                }
            }
        }

        // 충돌 해결 테이블은 연관 프레임을 모두 담아야 한다
        for (_, f) in m.frames.iter() {
            let FrameKind::EventTriggered {
                collision_table: Some(ct),
                associated,
                ..
            } = &f.kind
            else {
                continue;
            };
            let Some(set) = ct.id().and_then(|id| in_table.get(&id)) else {
                continue;
            };
            for a in associated
                .iter()
                .filter(|a| a.id().is_some_and(|id| !set.contains(&id)))
            {
                err(
                    out,
                    f.span,
                    format!(
                        "이벤트 트리거 프레임 `{}`의 충돌 해결 테이블에 연관 프레임 `{}`이(가) 없다",
                        f.name,
                        frame_label(m, a)
                    ),
                );
            }
        }
    }
}

impl R0201 {
    #[allow(clippy::too_many_arguments)]
    fn frame_id_range(
        &self,
        m: &LdfModel,
        out: &mut Vec<Diagnostic>,
        table: &str,
        span: Option<ldf_model::Span>,
        node: NodeId,
        start: u32,
        pids: &[u32],
    ) {
        let Some(list) = configurable(m, node) else {
            return;
        };
        if pids.is_empty() && start as usize >= list.len() {
            err(
                out,
                span,
                format!(
                    "스케줄 `{table}`의 AssignFrameIdRange 시작 인덱스 {start}이(가) 범위를 벗어난다"
                ),
            );
        }
        for (i, pid) in pids.iter().enumerate() {
            let idx = start as usize + i;
            let Some(&fid) = list.get(idx) else {
                err(
                    out,
                    span,
                    format!(
                        "스케줄 `{table}`의 AssignFrameIdRange 인덱스 {idx}이(가) configurable_frames 길이 {}를 넘는다",
                        list.len()
                    ),
                );
                continue;
            };
            // 0과 0xFF는 "지정 안 함"으로 쓰인다
            if *pid == 0 || *pid == 0xFF {
                continue;
            }
            let Ok(pid8) = u8::try_from(*pid) else {
                err(
                    out,
                    span,
                    format!("스케줄 `{table}`의 PID {pid:#X}이(가) 한 바이트를 넘는다"),
                );
                continue;
            };
            if !pid_is_valid(pid8) {
                err(
                    out,
                    span,
                    format!("스케줄 `{table}`의 PID {pid:#04X}의 패리티가 틀렸다"),
                );
                continue;
            }
            let want = m
                .frames
                .get(fid)
                .and_then(|f| f.lin_id())
                .and_then(|id| u8::try_from(id).ok())
                .map(protected_id);
            if want.is_some_and(|w| w != pid8) {
                err(
                    out,
                    span,
                    format!(
                        "스케줄 `{table}`의 PID {pid:#04X}이(가) 프레임 `{}`의 ID와 맞지 않는다",
                        m.frames.get(fid).map_or("?", |f| f.name.as_str())
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::load;

    const BASE: &str = "LIN_protocol_version = \"2.2\";\nLIN_speed = 19.2 kbps;\n\
        Nodes { Master: M, 1 ms, 0 ms; Slaves: S; }\n\
        Signals { A: 8, 0, M, S; B: 8, 0, S, M; }\n\
        Frames { F1: 1, M, 1 { A, 0; } F2: 2, S, 1 { B, 0; } }\n\
        Node_attributes { S { LIN_protocol = \"2.2\"; configurable_frames { F1; F2; } } }\n";

    fn n(schedule: &str) -> usize {
        let src = format!("{BASE}Schedule_tables {{ {schedule} }}");
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        out.len()
    }

    #[test]
    fn complete_schedule_is_clean() {
        assert_eq!(n("T { F1 delay 10 ms; F2 delay 10 ms; }"), 0);
    }

    #[test]
    fn empty_missing_and_short_slots() {
        assert_eq!(n("T { F1 delay 10 ms; F2 delay 10 ms; } E { }"), 1);
        assert_eq!(n("T { F1 delay 10 ms; }"), 1); // F2 누락
        assert_eq!(n("T { F1 delay 1 ms; F2 delay 10 ms; }"), 1); // 너무 짧다
    }

    #[test]
    fn frame_id_range_checks_pids() {
        // F1 id1 -> 0xC1, F2 id2 -> 0x42
        let ok = "T { F1 delay 10 ms; F2 delay 10 ms; AssignFrameIdRange { S, 0, 0xC1, 0x42 } delay 10 ms; }";
        assert_eq!(n(ok), 0);
        let bad = "T { F1 delay 10 ms; F2 delay 10 ms; AssignFrameIdRange { S, 0, 0xC1, 0x43 } delay 10 ms; }";
        assert_eq!(n(bad), 1);
    }
}
