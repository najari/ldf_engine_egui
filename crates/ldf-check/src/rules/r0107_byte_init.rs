//! 규칙 0107: 바이트 배열 시그널의 초기값이 형식에 맞지 않는다(바이트가 0–255를 벗어났거나 개수가 길이와 다르다).

use crate::Rule;
use crate::util::diag;
use ldf_model::{Diagnostic, InitValue, LdfModel, NoteKind, Severity};

/// 규칙 0107.
pub(crate) struct R0107;
/// 등록용 인스턴스.
pub(crate) static RULE: R0107 = R0107;

impl Rule for R0107 {
    fn code(&self) -> u16 {
        107
    }

    fn title(&self) -> &'static str {
        "Byte array initial value malformed"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        for n in m
            .notes
            .iter()
            .filter(|n| n.kind == NoteKind::ByteInitOutOfRange)
        {
            out.push(diag(
                Severity::Error,
                107,
                n.span,
                format!(
                    "시그널 `{}`의 바이트 배열 초기값에 0–255를 벗어난 값이 있다",
                    n.name
                ),
            ));
        }
        for (_, s) in m.signals.iter() {
            if let InitValue::Array(bytes) = &s.init {
                let expected = u64::from(s.size_bits).div_ceil(8);
                if bytes.len() as u64 != expected {
                    out.push(diag(
                        Severity::Error,
                        107,
                        s.span,
                        format!(
                            "시그널 `{}`({}비트)의 초기값 바이트가 {}개인데 {expected}개여야 한다",
                            s.name,
                            s.size_bits,
                            bytes.len()
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

    fn n(sig: &str) -> usize {
        let src = format!("Nodes {{ Master: M, 1 ms, 0 ms; Slaves: S; }}\nSignals {{ {sig} }}");
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        out.len()
    }

    #[test]
    fn byte_array_initial_values() {
        assert_eq!(n("A: 16, {1, 2}, M, S;"), 0);
        assert_eq!(n("A: 16, {1}, M, S;"), 1);
        assert_eq!(n("A: 8, {256, 2}, M, S;"), 1);
        assert_eq!(n("A: 8, 5, M, S;"), 0);
    }
}
