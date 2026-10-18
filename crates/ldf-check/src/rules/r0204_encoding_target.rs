//! 규칙 0204: 바이트 배열 시그널에 숫자 인코딩(논리값·물리값)을 연결했다. 배열에는 ASCII·BCD만 의미가 있다.

use crate::Rule;
use crate::util::diag;
use ldf_model::{Diagnostic, EncodingEntry, InitValue, LdfModel, Ref, Severity};

/// 규칙 0204.
pub(crate) struct R0204;
/// 등록용 인스턴스.
pub(crate) static RULE: R0204 = R0204;

impl Rule for R0204 {
    fn code(&self) -> u16 {
        204
    }

    fn title(&self) -> &'static str {
        "Encoding does not fit the signal type"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        for (_, s) in m.signals.iter() {
            if !matches!(s.init, InitValue::Array(_)) {
                continue;
            }
            for r in &s.encodings {
                let Some(enc) = r.id().and_then(|id| m.encodings.get(id)) else {
                    continue;
                };
                let numeric = enc.entries.iter().any(|e| {
                    matches!(
                        e,
                        EncodingEntry::Logical { .. } | EncodingEntry::Physical { .. }
                    )
                });
                if numeric {
                    out.push(diag(
                        Severity::Error,
                        204,
                        s.span,
                        format!(
                            "바이트 배열 시그널 `{}`에 숫자 인코딩 `{}`이(가) 연결되어 있다",
                            s.name,
                            match r {
                                Ref::Resolved(_) => enc.name.as_str(),
                                Ref::Unresolved(n) => n.as_str(),
                            }
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

    #[test]
    fn numeric_encodings_on_byte_arrays() {
        let src = |enc: &str| {
            format!(
                "Nodes {{ Master: M, 1 ms, 0 ms; Slaves: S; }}\n\
                 Signals {{ A: 16, {{1, 2}}, M, S; }}\n\
                 Signal_encoding_types {{ E {{ {enc} }} }}\nSignal_representation {{ E: A; }}"
            )
        };
        let mut out = Vec::new();
        RULE.check(&load(&src("logical_value, 0, \"x\";")).model, &mut out);
        assert_eq!(out.len(), 1);
        out.clear();
        RULE.check(&load(&src("ascii_value;")).model, &mut out);
        assert!(out.is_empty());
    }
}
