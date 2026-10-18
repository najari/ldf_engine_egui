//! 규칙 0010: `LIN_protocol_version`·`LIN_language_version`이 유효하고 서로 맞는지 본다.

use crate::Rule;
use crate::util::{Proto, diag};
use ldf_model::{Diagnostic, LdfModel, Severity};

/// 규칙 0010.
pub(crate) struct R0010;
/// 등록용 인스턴스.
pub(crate) static RULE: R0010 = R0010;

impl Rule for R0010 {
    fn code(&self) -> u16 {
        10
    }

    fn title(&self) -> &'static str {
        "Invalid LIN protocol or language version"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        let h = &m.header;
        let proto = match h.protocol_version.as_deref() {
            None => {
                out.push(diag(
                    Severity::Error,
                    10,
                    None,
                    "LIN_protocol_version이 선언되지 않았다",
                ));
                None
            }
            Some(s) => {
                let p = Proto::parse_protocol(s);
                if p == Proto::Unknown {
                    out.push(diag(
                        Severity::Error,
                        10,
                        None,
                        format!("LIN_protocol_version \"{s}\"은(는) 지원하는 버전이 아니다"),
                    ));
                }
                Some((s, p))
            }
        };
        let language = match h.language_version.as_deref() {
            None => {
                out.push(diag(
                    Severity::Error,
                    10,
                    None,
                    "LIN_language_version이 선언되지 않았다",
                ));
                None
            }
            Some(s) => {
                let p = Proto::parse_language(s);
                if p == Proto::Unknown {
                    out.push(diag(
                        Severity::Error,
                        10,
                        None,
                        format!("LIN_language_version \"{s}\"은(는) 지원하는 버전이 아니다"),
                    ));
                }
                Some((s, p))
            }
        };
        if let (Some((ps, pp)), Some((ls, lp))) = (proto, language) {
            // OEM 변형은 어떤 언어 버전과도 쓸 수 있다
            let compatible = pp == Proto::Oem || (pp != Proto::Unknown && pp == lp);
            if !compatible {
                out.push(diag(
                    Severity::Error,
                    10,
                    None,
                    format!(
                        "LIN_protocol_version \"{ps}\"와 LIN_language_version \"{ls}\"가 서로 맞지 않는다"
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

    fn count(src: &str) -> usize {
        let mut out = Vec::new();
        RULE.check(&load(src).model, &mut out);
        out.len()
    }

    #[test]
    fn valid_pairs_pass() {
        for (p, l) in [
            ("1.3", "1.3"),
            ("2.0", "2.0"),
            ("2.1", "2.1"),
            ("2.2", "2.2"),
            ("J2602_1_1.0", "J2602_3_1.0"),
            ("ISO17987:2015", "ISO17987:2015"),
            ("OEM variant", "1.3"),
        ] {
            let src = format!(
                "LIN_description_file;\nLIN_protocol_version = \"{p}\";\nLIN_language_version = \"{l}\";\n"
            );
            assert_eq!(count(&src), 0, "{p} / {l}");
        }
    }

    #[test]
    fn invalid_version_and_mismatch_are_reported() {
        let both = "LIN_protocol_version = \"2.3\";\nLIN_language_version = \"2.2\";\n";
        assert_eq!(count(both), 2); // 잘못된 값 + 불일치
        let diff = "LIN_protocol_version = \"2.2\";\nLIN_language_version = \"2.1\";\n";
        assert_eq!(count(diff), 1);
        assert_eq!(count(""), 2); // 둘 다 없음
    }
}
