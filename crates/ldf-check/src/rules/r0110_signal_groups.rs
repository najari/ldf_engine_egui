//! 규칙 0110: 시그널 그룹(`Signal_groups { 그룹: 크기 { 시그널, 오프셋; } }`)이 맞지 않는다.
//!
//! 그룹에 든 시그널은 `Signals`에 정의되어 있어야 하고, 그룹 안에서 시그널이 차지하는 비트가 그룹 크기를 넘지 않아야 한다.
//! LIN 1.3에서는 오류다. LIN 2.x에서는 이 섹션이 클러스터에 영향이 없으므로(LIN 2.2A §9.2.3.3) 같은 문제를 경고로만 알린다.
//! (이 번호는 Vector의 검사에 없는 이 프로젝트의 것이다.)

use super::r0101_sections::section_allowed;
use crate::Rule;
use crate::util::{Proto, diag, file_proto};
use ldf_model::{Diagnostic, LdfModel, Severity};

/// 규칙 0110.
pub(crate) struct R0110;
/// 등록용 인스턴스.
pub(crate) static RULE: R0110 = R0110;

impl Rule for R0110 {
    fn code(&self) -> u16 {
        110
    }

    fn title(&self) -> &'static str {
        "Signal group inconsistent"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        let Some(groups) = &m.signal_groups else {
            return;
        };
        // 이 버전에서 쓸 수 없는 섹션은 0101이 보고하고 여기서는 내용을 따지지 않는다
        let proto = file_proto(m);
        if !section_allowed("Signal_groups", proto) {
            return;
        }
        let severity = if proto == Proto::V1_3 {
            Severity::Error
        } else {
            Severity::Warning
        };
        for g in &groups.groups {
            for s in &g.signals {
                let Some(sig) = m.find_signal(&s.signal).and_then(|id| m.signals.get(id)) else {
                    out.push(diag(
                        severity,
                        110,
                        groups.span,
                        format!(
                            "시그널 그룹 `{}`의 시그널 `{}`이(가) 정의되어 있지 않다",
                            g.name, s.signal
                        ),
                    ));
                    continue;
                };
                if s.offset.saturating_add(sig.size_bits) > g.size_bits {
                    out.push(diag(
                        severity,
                        110,
                        groups.span,
                        format!(
                            "시그널 그룹 `{}`의 시그널 `{}`이(가) 그룹 크기 {}비트를 넘는다",
                            g.name, s.signal, g.size_bits
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

    fn n(version: &str, groups: &str) -> usize {
        let src = format!(
            "LIN_protocol_version = \"{version}\";\nNodes {{ Master: M, 1 ms, 0 ms; Slaves: A; }}\n\
             Signals {{ S8: 8, 0, M, A; S4: 4, 0, A, M; }}\n{groups}"
        );
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        out.len()
    }

    #[test]
    fn members_must_exist_and_fit() {
        assert_eq!(n("1.3", "Signal_groups { G: 16 { S8, 0; S4, 8; } }"), 0);
        assert_eq!(n("1.3", "Signal_groups { }"), 0);
        // 정의되지 않은 시그널
        assert_eq!(n("1.3", "Signal_groups { G: 16 { Ghost, 0; } }"), 1);
        // 그룹 크기를 넘는다(8 + 4 > 10)
        assert_eq!(n("1.3", "Signal_groups { G: 10 { S8, 0; S4, 8; } }"), 1);
    }

    #[test]
    fn lin_2_x_only_gets_warnings_and_iso_is_left_to_rule_0101() {
        let severities = |version: &str| {
            let src = format!(
                "LIN_protocol_version = \"{version}\";\nNodes {{ Master: M, 1 ms, 0 ms; Slaves: A; }}\n\
                 Signals {{ S8: 8, 0, M, A; }}\nSignal_groups {{ G: 16 {{ Ghost, 0; }} }}"
            );
            let mut out = Vec::new();
            RULE.check(&load(&src).model, &mut out);
            out.iter().map(|d| d.severity).collect::<Vec<_>>()
        };
        assert_eq!(severities("1.3"), [Severity::Error]);
        // 2.x에서는 섹션이 클러스터에 영향이 없다(LIN 2.2A §9.2.3.3)
        assert_eq!(severities("2.0"), [Severity::Warning]);
        assert_eq!(severities("2.2"), [Severity::Warning]);
        // ISO 17987에는 섹션 자체가 없다(0101)
        assert!(severities("ISO17987:2015").is_empty());
    }
}
