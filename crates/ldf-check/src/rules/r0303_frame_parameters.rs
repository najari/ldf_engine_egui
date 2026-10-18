//! 규칙 0303: 프레임 ID(무조건·이벤트 트리거 0–59)나 프레임 크기(1–8 byte)가 범위를 벗어났다.

use crate::Rule;
use crate::util::diag;
use ldf_model::{Diagnostic, FrameKind, LdfModel, Severity};

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
                _ => {}
            }
        }
    }
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
}
