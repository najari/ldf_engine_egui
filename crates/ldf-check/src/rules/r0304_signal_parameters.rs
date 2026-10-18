//! 규칙 0304: 시그널 길이나 초기값이 범위를 벗어났다.
//!
//! - 스칼라는 1–16비트, 바이트 배열은 8의 배수로 8–64비트.
//! - 스칼라 초기값은 길이로 표현할 수 있어야 한다.
//! - 인코딩 타입에 논리값·물리값이 있으면 초기값이 그중 하나에 속해야 한다.

use crate::Rule;
use crate::util::diag;
use ldf_model::{Diagnostic, EncodingEntry, InitValue, LdfModel, Severity, Signal};

/// 규칙 0304.
pub(crate) struct R0304;
/// 등록용 인스턴스.
pub(crate) static RULE: R0304 = R0304;

/// 초기값이 인코딩들 중 하나(숫자 항목이 있는 것)에 속하는지. 판단할 인코딩이 없으면 `true`.
fn fits_encoding(m: &LdfModel, s: &Signal, v: u64) -> bool {
    let mut judged = false;
    for r in &s.encodings {
        let Some(e) = r.id().and_then(|id| m.encodings.get(id)) else {
            continue;
        };
        let numeric = e.entries.iter().any(|x| {
            matches!(
                x,
                EncodingEntry::Logical { .. } | EncodingEntry::Physical { .. }
            )
        });
        if !numeric {
            continue;
        }
        judged = true;
        let ok = e.entries.iter().any(|x| match x {
            EncodingEntry::Logical { value, .. } => *value == v,
            EncodingEntry::Physical { min, max, .. } => {
                i64::try_from(v).is_ok_and(|v| (*min..=*max).contains(&v))
            }
            _ => false,
        });
        if ok {
            return true;
        }
    }
    !judged
}

impl Rule for R0304 {
    fn code(&self) -> u16 {
        304
    }

    fn title(&self) -> &'static str {
        "Signal parameter out of range"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        for (_, s) in m.signals.iter() {
            let mut bad = |msg: String| out.push(diag(Severity::Error, 304, s.span, msg));
            match &s.init {
                InitValue::Scalar(v) => {
                    if !(1..=16).contains(&s.size_bits) {
                        bad(format!(
                            "스칼라 시그널 `{}`의 길이 {}비트가 1–16을 벗어났다",
                            s.name, s.size_bits
                        ));
                        continue;
                    }
                    if *v >= 1u64 << s.size_bits {
                        bad(format!(
                            "시그널 `{}`의 초기값 {v}을(를) {}비트로 표현할 수 없다",
                            s.name, s.size_bits
                        ));
                    } else if !fits_encoding(m, s, *v) {
                        bad(format!(
                            "시그널 `{}`의 초기값 {v}이(가) 인코딩 타입의 값에 속하지 않는다",
                            s.name
                        ));
                    }
                }
                InitValue::Array(_) => {
                    if s.size_bits == 0 || s.size_bits > 64 || s.size_bits % 8 != 0 {
                        bad(format!(
                            "바이트 배열 시그널 `{}`의 길이 {}비트가 8의 배수로 8–64를 벗어났다",
                            s.name, s.size_bits
                        ));
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

    fn n(sigs: &str, enc: &str) -> usize {
        let src =
            format!("Nodes {{ Master: M, 1 ms, 0 ms; Slaves: S; }}\nSignals {{ {sigs} }}\n{enc}");
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        out.len()
    }

    #[test]
    fn length_and_init() {
        assert_eq!(n("A: 16, 65535, M, S; B: 8, {1}, M, S;", ""), 0);
        assert_eq!(n("A: 0, 0, M, S;", ""), 1);
        assert_eq!(n("A: 17, 0, M, S;", ""), 1);
        assert_eq!(n("A: 2, 5, M, S;", ""), 1);
        assert_eq!(n("A: 9, {0}, M, S;", ""), 1);
        assert_eq!(n("A: 72, {0}, M, S;", ""), 1);
    }

    #[test]
    fn init_must_belong_to_the_encoding() {
        let enc = "Signal_encoding_types { E { logical_value, 0, \"a\"; logical_value, 2, \"b\"; } }\n\
                   Signal_representation { E: A; }";
        assert_eq!(n("A: 8, 2, M, S;", enc), 0);
        assert_eq!(n("A: 8, 1, M, S;", enc), 1);
        let phys = "Signal_encoding_types { E { physical_value, 0, 1, 1, 0, \"u\"; } }\nSignal_representation { E: A; }";
        assert_eq!(n("A: 8, 1, M, S;", phys), 0);
        assert_eq!(n("A: 8, 2, M, S;", phys), 1);
    }
}
