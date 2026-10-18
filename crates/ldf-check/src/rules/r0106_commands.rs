//! 규칙 0106: 스케줄 테이블의 진단 명령 형식이 LIN 버전과 맞지 않는다.

use crate::Rule;
use crate::util::{Proto, diag, file_proto};
use ldf_model::{Diagnostic, LdfModel, Severity, SlotEntry};

/// 규칙 0106.
pub(crate) struct R0106;
/// 등록용 인스턴스.
pub(crate) static RULE: R0106 = R0106;

/// 슬롯 명령의 이름과 LIN 2.0 방식의 인자(숫자 4개의 `AssignNAD`)인지.
pub(crate) fn command_name(e: &SlotEntry) -> Option<(String, bool)> {
    match e {
        SlotEntry::AssignNad { .. } => Some(("AssignNAD".to_owned(), false)),
        SlotEntry::ConditionalChangeNad { .. } => Some(("ConditionalChangeNAD".to_owned(), false)),
        SlotEntry::DataDump { .. } => Some(("DataDump".to_owned(), false)),
        SlotEntry::SaveConfiguration { .. } => Some(("SaveConfiguration".to_owned(), false)),
        SlotEntry::AssignFrameIdRange { .. } => Some(("AssignFrameIdRange".to_owned(), false)),
        SlotEntry::AssignFrameId { .. } => Some(("AssignFrameId".to_owned(), false)),
        SlotEntry::FreeFormat { .. } => Some(("FreeFormat".to_owned(), false)),
        SlotEntry::Unknown(raw) => {
            let name: String = raw
                .trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let legacy = name == "AssignNAD";
            (!name.is_empty()).then_some((name, legacy))
        }
        SlotEntry::Frame(_) => None,
    }
}

/// 이 명령이 `p` 버전에서 허용되는지. 모르는 명령은 허용으로 본다.
pub(crate) fn allowed(name: &str, legacy_assign_nad: bool, p: Proto) -> bool {
    let is_2_0 = p == Proto::V2_0;
    match name {
        // 2.0은 숫자 4개 형식, 2.1 이상은 노드 이름 형식
        "AssignNAD" => match p {
            Proto::V2_0 | Proto::V1_3 => legacy_assign_nad,
            Proto::V2_1 | Proto::V2_2 | Proto::Iso => !legacy_assign_nad,
            _ => true,
        },
        "UnassignFrameId" => is_2_0,
        "ConditionalChangeNAD" => matches!(p, Proto::V2_1 | Proto::V2_2),
        "DataDump" | "SaveConfiguration" | "AssignFrameIdRange" => !is_2_0 && p != Proto::V1_3,
        _ => true,
    }
}

impl Rule for R0106 {
    fn code(&self) -> u16 {
        106
    }

    fn title(&self) -> &'static str {
        "Schedule command not allowed for LIN version"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        let p = file_proto(m);
        if matches!(p, Proto::Unknown | Proto::Oem) {
            return;
        }
        let ver = m.header.protocol_version.as_deref().unwrap_or("?");
        for (_, t) in m.schedules.iter() {
            for slot in &t.slots {
                let Some((name, legacy)) = command_name(&slot.entry) else {
                    continue;
                };
                if !allowed(&name, legacy, p) {
                    let form = if legacy { " (LIN 2.0 형식)" } else { "" };
                    out.push(diag(
                        Severity::Error,
                        106,
                        slot.span,
                        format!(
                            "스케줄 `{}`의 명령 {name}{form}은(는) LIN {ver}에서 허용되지 않는다",
                            t.name
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

    fn n(version: &str, slot: &str) -> usize {
        let src = format!(
            "LIN_protocol_version = \"{version}\";\nNodes {{ Master: M, 1 ms, 0 ms; Slaves: S; }}\nSchedule_tables {{ T {{ {slot} }} }}"
        );
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        out.len()
    }

    #[test]
    fn commands_by_version() {
        assert_eq!(n("2.1", "AssignNAD { S } delay 10 ms;"), 0);
        assert_eq!(n("2.0", "AssignNAD { S } delay 10 ms;"), 1);
        assert_eq!(n("2.0", "AssignNAD { 1, 2, 3, 4 } delay 10 ms;"), 0);
        assert_eq!(n("2.1", "AssignNAD { 1, 2, 3, 4 } delay 10 ms;"), 1);
        assert_eq!(n("2.1", "UnassignFrameId { S, F } delay 10 ms;"), 1);
        assert_eq!(n("2.0", "UnassignFrameId { S, F } delay 10 ms;"), 0);
        assert_eq!(n("2.0", "SaveConfiguration { S } delay 10 ms;"), 1);
        assert_eq!(
            n(
                "ISO17987:2015",
                "ConditionalChangeNAD { 1, 0, 0, 0, 0, 0 } delay 10 ms;"
            ),
            1
        );
    }
}
