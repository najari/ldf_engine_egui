//! 규칙 0305: 스케줄 진단 명령의 인자가 범위를 벗어났거나, 이 버전에서는 해석할 수 없는 명령이다.
//!
//! - `FreeFormat`·`DataDump`·`ConditionalChangeNAD`의 바이트는 0–255.
//! - LIN 2.0 형식 `AssignNAD { nad, 새 nad, 공급자, 기능 }`의 NAD는 1–125, 공급자 ≤ 0x7FFF, 기능 ≤ 0xFFFF.
//! - `AssignFrameIdRange`의 PID는 0–255.
//! - 버전에 맞지 않는 명령(0106)은 인자를 해석할 수 없다.

use super::r0106_commands::{allowed, command_name};
use crate::Rule;
use crate::raw::{command_args, parse_int};
use crate::util::{diag, file_proto};
use ldf_model::{Diagnostic, LdfModel, Severity, SlotEntry};

/// 규칙 0305.
pub(crate) struct R0305;
/// 등록용 인스턴스.
pub(crate) static RULE: R0305 = R0305;

fn byte_ok(v: i128) -> bool {
    (0..=255).contains(&v)
}

impl Rule for R0305 {
    fn code(&self) -> u16 {
        305
    }

    fn title(&self) -> &'static str {
        "Schedule command parameter out of range"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        let file = file_proto(m);
        for (_, t) in m.schedules.iter() {
            for slot in &t.slots {
                let mut bad = |msg: String| {
                    out.push(diag(
                        Severity::Error,
                        305,
                        slot.span,
                        format!("스케줄 `{}`: {msg}", t.name),
                    ));
                };
                let Some((name, legacy)) = command_name(&slot.entry) else {
                    continue;
                };
                if !allowed(&name, legacy, file) {
                    bad(format!(
                        "명령 {name}의 인자를 이 LIN 버전에서는 해석할 수 없다"
                    ));
                    continue;
                }
                match &slot.entry {
                    SlotEntry::FreeFormat { data } | SlotEntry::DataDump { data, .. } => {
                        for v in data.iter().filter(|v| **v > 255) {
                            bad(format!("{name}의 바이트 {v}이(가) 0–255를 벗어났다"));
                        }
                    }
                    SlotEntry::ConditionalChangeNad { args } => {
                        for v in args.iter().filter(|v| **v > 255) {
                            bad(format!(
                                "ConditionalChangeNAD의 인자 {v}이(가) 0–255를 벗어났다"
                            ));
                        }
                    }
                    SlotEntry::AssignFrameIdRange { pids, .. } => {
                        for v in pids.iter().filter(|v| **v > 255) {
                            bad(format!(
                                "AssignFrameIdRange의 PID {v}이(가) 0–255를 벗어났다"
                            ));
                        }
                    }
                    SlotEntry::Unknown(raw) => {
                        let nums: Vec<Option<i128>> =
                            command_args(raw).iter().map(|a| parse_int(a)).collect();
                        match name.as_str() {
                            "FreeFormat" | "DataDump" => {
                                for v in nums.iter().flatten().filter(|v| !byte_ok(**v)) {
                                    bad(format!("{name}의 바이트 {v}이(가) 0–255를 벗어났다"));
                                }
                            }
                            // LIN 2.0 형식: { NAD, 새 NAD, 공급자 ID, 기능 ID }
                            "AssignNAD" if legacy => {
                                let get = |i: usize| nums.get(i).copied().flatten();
                                for i in [0, 1] {
                                    if get(i).is_some_and(|v| !(1..=125).contains(&v)) {
                                        bad("AssignNAD의 NAD가 1–125를 벗어났다".to_owned());
                                    }
                                }
                                if get(2).is_some_and(|v| !(0..=0x7FFF).contains(&v)) {
                                    bad("AssignNAD의 공급자 ID가 범위를 벗어났다".to_owned());
                                }
                                if get(3).is_some_and(|v| !(0..=0xFFFF).contains(&v)) {
                                    bad("AssignNAD의 기능 ID가 범위를 벗어났다".to_owned());
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
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
    fn parameter_ranges() {
        assert_eq!(
            n("2.1", "FreeFormat { 1, 2, 3, 4, 5, 6, 7, 8 } delay 10 ms;"),
            0
        );
        assert_eq!(
            n(
                "2.1",
                "FreeFormat { 256, 2, 3, 4, 5, 6, 7, 8 } delay 10 ms;"
            ),
            1
        );
        assert_eq!(
            n("2.1", "FreeFormat { -1, 2, 3, 4, 5, 6, 7, 8 } delay 10 ms;"),
            1
        );
        assert_eq!(n("2.0", "AssignNAD { 0x2, 0x1, 0x1, 0x3 } delay 10 ms;"), 0);
        assert_eq!(
            n("2.0", "AssignNAD { 0xFF, 0x1, 0x1, 0x3 } delay 10 ms;"),
            1
        );
        assert_eq!(
            n("2.0", "AssignNAD { 0x2, 0xFFFF, 0x1, 0x3 } delay 10 ms;"),
            1
        );
    }

    #[test]
    fn wrong_version_commands_cannot_be_parsed() {
        assert_eq!(n("2.1", "AssignNAD { 1, 2, 3, 4 } delay 10 ms;"), 1);
        assert_eq!(n("2.1", "UnassignFrameId { S, F } delay 10 ms;"), 1);
    }
}
