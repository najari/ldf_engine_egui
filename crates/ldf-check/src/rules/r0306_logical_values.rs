//! 규칙 0306: 인코딩의 논리값이나 물리값 범위가 16비트(0–65535)를 벗어났다.

use crate::Rule;
use crate::util::diag;
use ldf_model::{Diagnostic, EncodingEntry, LdfModel, Severity};

/// 규칙 0306.
pub(crate) struct R0306;
/// 등록용 인스턴스.
pub(crate) static RULE: R0306 = R0306;

impl Rule for R0306 {
    fn code(&self) -> u16 {
        306
    }

    fn title(&self) -> &'static str {
        "Encoding value out of range"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        for (_, e) in m.encodings.iter() {
            for x in &e.entries {
                let bad = match x {
                    EncodingEntry::Logical { value, .. } => *value > 0xFFFF,
                    EncodingEntry::Physical { min, max, .. } => {
                        !(0..=0xFFFF).contains(min) || !(0..=0xFFFF).contains(max)
                    }
                    _ => false,
                };
                if bad {
                    out.push(diag(
                        Severity::Error,
                        306,
                        e.span,
                        format!("인코딩 `{}`의 값이 0–65535를 벗어났다", e.name),
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
    fn sixteen_bit_limit() {
        let n = |body: &str| {
            let mut out = Vec::new();
            RULE.check(
                &load(&format!("Signal_encoding_types {{ E {{ {body} }} }}")).model,
                &mut out,
            );
            out.len()
        };
        assert_eq!(n("logical_value, 65535, \"a\";"), 0);
        assert_eq!(n("logical_value, 65536, \"a\";"), 1);
        assert_eq!(n("physical_value, 0, 65536, 1, 0, \"u\";"), 1);
    }
}
