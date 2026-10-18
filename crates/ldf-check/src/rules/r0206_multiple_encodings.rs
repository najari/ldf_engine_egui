//! 규칙 0206: 한 시그널에 인코딩 타입이 둘 이상 연결되었다.

use crate::Rule;
use crate::util::diag;
use ldf_model::{Diagnostic, LdfModel, Severity};

/// 규칙 0206.
pub(crate) struct R0206;
/// 등록용 인스턴스.
pub(crate) static RULE: R0206 = R0206;

impl Rule for R0206 {
    fn code(&self) -> u16 {
        206
    }

    fn title(&self) -> &'static str {
        "Signal has several encodings"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        for (_, s) in m.signals.iter() {
            if s.encodings.len() > 1 {
                out.push(diag(
                    Severity::Warning,
                    206,
                    s.span,
                    format!(
                        "시그널 `{}`에 인코딩 {}개가 연결되어 있다",
                        s.name,
                        s.encodings.len()
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
    fn two_encodings_on_one_signal() {
        let src = "Nodes { Master: M, 1 ms, 0 ms; Slaves: S; }\nSignals { A: 8, 0, M, S; B: 8, 0, M, S; }\n\
                   Signal_encoding_types { E1 { bcd_value; } E2 { ascii_value; } }\n\
                   Signal_representation { E1: A; E2: A, B; }";
        let mut out = Vec::new();
        RULE.check(&load(src).model, &mut out);
        assert_eq!(out.len(), 1);
    }
}
