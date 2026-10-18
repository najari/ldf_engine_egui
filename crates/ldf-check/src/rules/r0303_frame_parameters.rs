//! 규칙 0303: 프레임 ID(무조건·이벤트 트리거 0–59)나 프레임 크기(1–8 byte)가 범위를 벗어났다.
//! 진단 프레임은 값이 정해져 있다: `MasterReq`는 ID 60(0x3C), `SlaveResp`는 61(0x3D)이고, 시그널은
//! `MasterReqB0`…`B7`(`SlaveRespB0`…`B7`)이 오프셋 0, 8, …, 56에 차례로 놓인다(LIN 2.2A §9.2.4.4).

use crate::Rule;
use crate::util::diag;
use crate::util::signal_label;
use ldf_model::{Diagnostic, Frame, FrameKind, LdfModel, Severity};

/// 규칙 0303.
pub(crate) struct R0303;
/// 등록용 인스턴스.
pub(crate) static RULE: R0303 = R0303;

impl Rule for R0303 {
    fn code(&self) -> u16 {
        303
    }

    fn title(&self) -> &'static str {
        "Frame parameter out of range"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        for (_, f) in m.frames.iter() {
            match &f.kind {
                FrameKind::Unconditional { id, size, .. } => {
                    if *id > 59 {
                        out.push(diag(
                            Severity::Error,
                            303,
                            f.span,
                            format!("프레임 `{}`의 ID {id}이(가) 0–59를 벗어났다", f.name),
                        ));
                    }
                    if !(1..=8).contains(size) {
                        out.push(diag(
                            Severity::Error,
                            303,
                            f.span,
                            format!("프레임 `{}`의 크기 {size}바이트가 1–8을 벗어났다", f.name),
                        ));
                    }
                }
                FrameKind::EventTriggered { id, .. } if *id > 59 => out.push(diag(
                    Severity::Error,
                    303,
                    f.span,
                    format!(
                        "이벤트 트리거 프레임 `{}`의 ID {id}이(가) 0–59를 벗어났다",
                        f.name
                    ),
                )),
                FrameKind::Diagnostic { id, signals } => {
                    let want = match f.name.as_str() {
                        "MasterReq" => 0x3C,
                        "SlaveResp" => 0x3D,
                        _ => continue,
                    };
                    if *id != want {
                        out.push(diag(
                            Severity::Error,
                            303,
                            f.span,
                            format!(
                                "진단 프레임 `{}`의 ID가 {id}이(가) 아니라 {want}(0x{want:02X})여야 한다",
                                f.name
                            ),
                        ));
                    }
                    if !standard_layout(m, f, signals) {
                        out.push(diag(
                            Severity::Error,
                            303,
                            f.span,
                            format!(
                                "진단 프레임 `{0}`의 시그널이 `{0}B0`…`{0}B7`을 오프셋 0, 8, …, 56에 놓는 규격 배치와 다르다",
                                f.name
                            ),
                        ));
                    }
                }
                _ => {}
            }
        }
    }
}

/// 진단 프레임의 시그널이 규격 배치(`<이름>B0`…`B7`을 8비트 간격으로)인지.
fn standard_layout(m: &LdfModel, f: &Frame, signals: &[ldf_model::FrameSignal]) -> bool {
    signals.len() == 8
        && signals.iter().enumerate().all(|(i, s)| {
            s.offset == 8 * i as u32 && signal_label(m, &s.signal) == format!("{}B{i}", f.name)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::load;

    #[test]
    fn id_and_size() {
        let n = |frames: &str| {
            let src = format!(
                "Nodes {{ Master: M, 1 ms, 0 ms; Slaves: S; }}\nSignals {{ A: 8, 0, M, S; }}\nFrames {{ {frames} }}"
            );
            let mut out = Vec::new();
            RULE.check(&load(&src).model, &mut out);
            out.len()
        };
        assert_eq!(n("F: 59, M, 8 { A, 0; }"), 0);
        assert_eq!(n("F: 60, M, 8 { A, 0; }"), 1);
        assert_eq!(n("F: 1, M, 9 { A, 0; }"), 1);
        assert_eq!(n("F: 1, M, 0 { }"), 1);
    }

    #[test]
    fn diagnostic_frames_have_fixed_ids_and_layout() {
        let sig = |p: &str| {
            (0..8)
                .map(|i| format!("{p}B{i}: 8, 0;"))
                .collect::<String>()
        };
        let lay = |p: &str| {
            (0..8)
                .map(|i| format!("{p}B{i}, {};", 8 * i))
                .collect::<String>()
        };
        let n = |frames: &str| {
            let src = format!(
                "Nodes {{ Master: M, 1 ms, 0 ms; Slaves: S; }}\nDiagnostic_signals {{ {}{} }}\nDiagnostic_frames {{ {frames} }}",
                sig("MasterReq"),
                sig("SlaveResp")
            );
            let mut out = Vec::new();
            RULE.check(&load(&src).model, &mut out);
            out.len()
        };
        let ok = format!(
            "MasterReq: 0x3c {{ {} }} SlaveResp: 0x3d {{ {} }}",
            lay("MasterReq"),
            lay("SlaveResp")
        );
        assert_eq!(n(&ok), 0);
        assert_eq!(n(&ok.replace("0x3c", "0x10")), 1);
        assert_eq!(n(&ok.replace("0x3d", "0x3c")), 1);
        // 시그널 순서나 오프셋이 다르다
        assert_eq!(n(&ok.replace("MasterReqB0, 0;", "MasterReqB0, 8;")), 1);
        assert_eq!(
            n(&ok.replace(
                "MasterReqB1, 8;MasterReqB2, 16;",
                "MasterReqB2, 8;MasterReqB1, 16;"
            )),
            1
        );
        assert_eq!(n(&ok.replace("SlaveRespB7, 56;", "")), 1);
    }
}
