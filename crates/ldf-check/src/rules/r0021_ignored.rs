//! 규칙 0021: 읽을 수는 있지만 의미가 없어 무시된 선언.
//!
//! - 없는 시그널을 가리키는 `Signal_representation` 항목.
//! - 이 버전에는 없는 섹션(`Signal_groups`·`Diagnostic_addresses`·`composite`, LIN 1.3의 `dynamic_frames`).
//! - 마스터 노드에 둔 노드 속성.
//! - 프레임에 들어 있지만 그 프레임의 발행자가 발행하지 않는 시그널.
//! - 쓸 수 없는 값(공급자·기능 ID)을 가진, 또는 ISO 17987에서 쓸 수 없는 LIN 2.0 형식의 `AssignNAD`.

use super::r0106_commands::{allowed, command_name};
use crate::Rule;
use crate::raw::{command_args, parse_int};
use crate::util::{Proto, diag, file_proto, is_master, node_label, same_node};
use ldf_model::{Diagnostic, FrameKind, LdfModel, NoteKind, Severity};

/// 규칙 0021.
pub(crate) struct R0021;
/// 등록용 인스턴스.
pub(crate) static RULE: R0021 = R0021;

fn ignored_section(name: &str, p: Proto) -> bool {
    match name {
        "Signal_groups" => p != Proto::V1_3,
        "Diagnostic_addresses" => p != Proto::Iso,
        "composite" => p != Proto::V2_0,
        "dynamic_frames" | "Dynamic_frames" => p == Proto::V1_3,
        _ => false,
    }
}

impl Rule for R0021 {
    fn code(&self) -> u16 {
        21
    }

    fn title(&self) -> &'static str {
        "Ignored LDF declaration"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        let file = file_proto(m);
        let mut warn = |span, msg: String| out.push(diag(Severity::Warning, 21, span, msg));

        for n in &m.notes {
            if n.kind == NoteKind::UnknownRepresentationSignal {
                warn(
                    n.span,
                    format!(
                        "Signal_representation `{}`이(가) 정의되지 않은 시그널 `{}`을(를) 가리켜 무시했다",
                        n.name, n.detail
                    ),
                );
            }
        }

        if !matches!(file, Proto::Unknown | Proto::Oem) {
            for s in &m.unknown_sections {
                if ignored_section(&s.name, file) {
                    warn(
                        s.span,
                        format!("`{}` 섹션은 이 LIN 버전에서 무시된다", s.name),
                    );
                }
            }
        }

        for a in &m.node_attrs {
            if is_master(m, &a.node) {
                warn(
                    a.span,
                    format!("마스터 `{}`의 노드 속성은 무시된다", node_label(m, &a.node)),
                );
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
                    warn(
                        f.span,
                        format!(
                            "프레임 `{}`의 시그널 `{}`은(는) 발행자가 달라 무시된다",
                            f.name, sig.name
                        ),
                    );
                }
            }
        }

        for (_, t) in m.schedules.iter() {
            for slot in &t.slots {
                let Some((name, legacy)) = command_name(&slot.entry) else {
                    continue;
                };
                // 이 버전에서 쓸 수 없는 명령(0106)은 무시된다
                if !allowed(&name, legacy, file) {
                    warn(
                        slot.span,
                        format!(
                            "스케줄 `{}`의 명령 {name}은(는) 이 LIN 버전에서 무시된다",
                            t.name
                        ),
                    );
                    continue;
                }
                if name != "AssignNAD" || !legacy {
                    continue;
                }
                if let ldf_model::SlotEntry::Unknown(raw) = &slot.entry {
                    let nums: Vec<Option<i128>> =
                        command_args(raw).iter().map(|a| parse_int(a)).collect();
                    let supplier_bad = nums
                        .get(2)
                        .copied()
                        .flatten()
                        .is_some_and(|v| !(0..=0x7FFF).contains(&v));
                    let function_bad = nums
                        .get(3)
                        .copied()
                        .flatten()
                        .is_some_and(|v| !(0..=0xFFFF).contains(&v));
                    if supplier_bad || function_bad {
                        // 공급자 ID와 기능 ID를 함께 읽지 못해 둘 다 무시된다
                        for what in ["공급자 ID", "기능 ID"] {
                            warn(
                                slot.span,
                                format!(
                                    "스케줄 `{}`의 AssignNAD {what}를 읽을 수 없어 무시했다",
                                    t.name
                                ),
                            );
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

    fn n(src: &str) -> usize {
        let mut out = Vec::new();
        RULE.check(&load(src).model, &mut out);
        out.len()
    }

    #[test]
    fn representation_of_missing_signal_is_reported() {
        let src = "Signals { A: 8, 0, M, S; }\nSignal_encoding_types { E { logical_value, 0, \"x\"; } }\nSignal_representation { E: A, Ghost; }";
        let mut out = Vec::new();
        RULE.check(&load(src).model, &mut out);
        assert_eq!(out.len(), 1);
        assert!(out[0].message.contains("Ghost"));
    }

    #[test]
    fn sections_that_the_version_does_not_know() {
        let head = "LIN_protocol_version = \"2.2\";\n";
        assert_eq!(n(&format!("{head}Diagnostic_addresses {{ }}")), 1);
        assert_eq!(n(&format!("{head}Signal_groups {{ }}")), 1);
        assert_eq!(n(&format!("{head}dynamic_frames {{ 1 }}")), 0);
        assert_eq!(
            n("LIN_protocol_version = \"1.3\";\ndynamic_frames { 1 }"),
            1
        );
    }

    #[test]
    fn master_attributes_and_foreign_signals() {
        let nodes =
            "LIN_protocol_version = \"2.2\";\nNodes { Master: M, 1 ms, 0 ms; Slaves: S; }\n";
        assert_eq!(
            n(&format!(
                "{nodes}Node_attributes {{ M {{ configured_NAD = 1; }} }}"
            )),
            1
        );
        let src =
            format!("{nodes}Signals {{ A: 8, 0, S, M; }}\nFrames {{ F: 1, M, 1 {{ A, 0; }} }}");
        assert_eq!(n(&src), 1);
    }

    #[test]
    fn legacy_assign_nad_with_bad_ids() {
        let n20 = |args: &str| {
            n(&format!(
                "LIN_protocol_version = \"2.0\";\nNodes {{ Master: M, 1 ms, 0 ms; Slaves: S; }}\nSchedule_tables {{ T {{ AssignNAD {{ {args} }} delay 10 ms; }} }}"
            ))
        };
        assert_eq!(n20("0x2, 0x1, 0x1, 0x1"), 0);
        assert_eq!(n20("0x2, 0x1, 0xFFFF, 0x1"), 2);
        assert_eq!(n20("0x2, 0x1, 0x1, 0x1FFFF"), 2);
    }
}
