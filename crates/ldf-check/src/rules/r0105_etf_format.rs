//! 규칙 0105: 이벤트 트리거 프레임의 충돌 해결 테이블 표기가 LIN 버전과 맞지 않는다
//! (2.0은 테이블을 쓰지 않고, 2.1 이상은 반드시 쓴다).

use crate::Rule;
use crate::util::{Proto, diag, file_proto};
use ldf_model::{Diagnostic, FrameKind, LdfModel, Severity};

/// 규칙 0105.
pub(crate) struct R0105;
/// 등록용 인스턴스.
pub(crate) static RULE: R0105 = R0105;

impl Rule for R0105 {
    fn code(&self) -> u16 {
        105
    }

    fn title(&self) -> &'static str {
        "Event triggered frame format inconsistent with LIN version"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        let p = file_proto(m);
        let ver = m.header.protocol_version.as_deref().unwrap_or("?");
        for (_, f) in m.frames.iter() {
            let FrameKind::EventTriggered {
                collision_table, ..
            } = &f.kind
            else {
                continue;
            };
            match (p, collision_table.is_some()) {
                (Proto::V2_0, true) => out.push(diag(
                    Severity::Error,
                    105,
                    f.span,
                    format!(
                        "이벤트 트리거 프레임 `{}`에 충돌 해결 테이블을 쓸 수 없다(LIN {ver})",
                        f.name
                    ),
                )),
                (Proto::V2_1 | Proto::V2_2 | Proto::Iso, false) => out.push(diag(
                    Severity::Error,
                    105,
                    f.span,
                    format!(
                        "이벤트 트리거 프레임 `{}`에 충돌 해결 테이블이 없다(LIN {ver}는 필수)",
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

    fn n(version: &str, etf: &str) -> usize {
        let src =
            format!("LIN_protocol_version = \"{version}\";\nEvent_triggered_frames {{ {etf} }}");
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        out.len()
    }

    #[test]
    fn collision_table_by_version() {
        assert_eq!(n("2.0", "E: 5, A, B;"), 0);
        assert_eq!(n("2.0", "E: T, 5, A, B;"), 1);
        assert_eq!(n("2.2", "E: T, 5, A, B;"), 0);
        assert_eq!(n("2.2", "E: 5, A, B;"), 1);
    }
}
