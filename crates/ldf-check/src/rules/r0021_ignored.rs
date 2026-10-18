//! 규칙 0021: 읽을 수는 있지만 의미가 없어 무시된 선언(없는 시그널을 가리키는 `Signal_representation` 항목 등).

use crate::Rule;
use crate::util::diag;
use ldf_model::{Diagnostic, LdfModel, NoteKind, Severity};

/// 규칙 0021.
pub(crate) struct R0021;
/// 등록용 인스턴스.
pub(crate) static RULE: R0021 = R0021;

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
        for n in &m.notes {
            if n.kind == NoteKind::UnknownRepresentationSignal {
                out.push(diag(
                    Severity::Warning,
                    21,
                    n.span,
                    format!(
                        "Signal_representation `{}`이(가) 정의되지 않은 시그널 `{}`을(를) 가리켜 무시했다",
                        n.name, n.detail
                    ),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::load;

    #[test]
    fn representation_of_missing_signal_is_reported() {
        let src = "Signals { A: 8, 0, M, S; }\nSignal_encoding_types { E { logical_value, 0, \"x\"; } }\nSignal_representation { E: A, Ghost; }";
        let mut out = Vec::new();
        RULE.check(&load(src).model, &mut out);
        assert_eq!(out.len(), 1);
        assert!(out[0].message.contains("Ghost"));
    }
}
