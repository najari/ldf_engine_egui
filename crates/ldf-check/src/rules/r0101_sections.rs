//! 규칙 0101: 선언한 LIN 버전에서 허용되지 않는 섹션·헤더 항목.

use crate::Rule;
use crate::util::{Proto, diag, file_proto};
use ldf_model::{Diagnostic, LdfModel, Severity};

/// 모델이 원문으로 보관하는 섹션을 이 버전에서 쓸 수 있는지.
///
/// `composite`는 LIN 2.x 문법이다(LIN 2.2A §9.2.2.3). ISO 17987 파일의 `composite`는 Vector가 0101로 보고한다(`Format/0201`).
pub(crate) fn section_allowed(name: &str, p: Proto) -> bool {
    match name {
        "dynamic_frames" | "Dynamic_frames" => p == Proto::V2_0,
        // LIN 2.2A §9.2.3.3: LIN 1.3의 기능이고 2.x에서는 쓰지 않지만(deprecated) 문법으로는 허용되고 클러스터에 영향이 없다
        "Signal_groups" => matches!(p, Proto::V1_3 | Proto::V2_0 | Proto::V2_1 | Proto::V2_2),
        "Diagnostic_addresses" => p == Proto::Iso,
        "Node_composition" => matches!(p, Proto::V2_1 | Proto::V2_2 | Proto::Iso),
        "composite" => matches!(p, Proto::V2_0 | Proto::V2_1 | Proto::V2_2),
        _ => true,
    }
}

/// 규칙 0101.
pub(crate) struct R0101;
/// 등록용 인스턴스.
pub(crate) static RULE: R0101 = R0101;

impl Rule for R0101 {
    fn code(&self) -> u16 {
        101
    }

    fn title(&self) -> &'static str {
        "Section or header item not allowed for LIN version"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        let p = file_proto(m);
        if p == Proto::Unknown || p == Proto::Oem {
            return;
        }
        let ver = m.header.protocol_version.as_deref().unwrap_or("?");
        let mut bad = |what: &str, span: Option<ldf_model::Span>| {
            out.push(diag(
                Severity::Error,
                101,
                span,
                format!("{what}은(는) LIN {ver} 파일에서 허용되지 않는다"),
            ));
        };

        // 헤더 항목
        if m.header.channel_name.is_some() && !matches!(p, Proto::V2_1 | Proto::V2_2 | Proto::Iso) {
            bad("Channel_name", None);
        }
        if m.header.ldf_file_revision.is_some() && p != Proto::Iso {
            bad("LDF_file_revision", None);
        }
        if m.header.sig_byte_order_big_endian && p != Proto::Iso {
            bad("LIN_sig_byte_order_big_endian", None);
        }

        // 보관 섹션
        let composition = m.composition.iter().map(|c| (c.keyword.as_str(), c.span));
        let dynamic = m
            .dynamic_frames
            .iter()
            .map(|d| (d.keyword.as_str(), d.span));
        let groups = m.signal_groups.iter().map(|g| ("Signal_groups", g.span));
        let raw = m.unknown_sections.iter().map(|s| (s.name.as_str(), s.span));
        for (name, span) in composition.chain(dynamic).chain(groups).chain(raw) {
            if !section_allowed(name, p) {
                bad(&format!("`{name}` 섹션"), span);
            }
        }

        // LIN 1.3에는 2.x에서 생긴 섹션이 없다
        if p == Proto::V1_3 {
            for name in [
                "Diagnostic_signals",
                "Diagnostic_frames",
                "Node_attributes",
                "Event_triggered_frames",
            ] {
                if m.section_order.iter().any(|s| s == name) {
                    bad(&format!("`{name}` 섹션"), None);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::load;

    fn n(version: &str, extra: &str) -> usize {
        let src = format!(
            "LIN_description_file;\nLIN_protocol_version = \"{version}\";\nLIN_language_version = \"{version}\";\n{extra}\n"
        );
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        out.len()
    }

    #[test]
    fn header_and_sections_by_version() {
        assert_eq!(n("2.2", "Channel_name = A;"), 0);
        assert_eq!(n("2.0", "Channel_name = A;"), 1);
        assert_eq!(n("2.2", "LIN_sig_byte_order_big_endian;"), 1);
        assert_eq!(n("ISO17987:2015", "LIN_sig_byte_order_big_endian;"), 0);
        assert_eq!(n("2.2", "composite { configuration c { P { A; } } }"), 0);
        assert_eq!(n("2.0", "composite { configuration c { P { A; } } }"), 0);
        assert_eq!(
            n(
                "ISO17987:2015",
                "composite { configuration c { P { A; } } }"
            ),
            1
        );
        assert_eq!(n("2.1", "dynamic_frames { 1 }"), 1);
        assert_eq!(n("2.0", "dynamic_frames { 1 }"), 0);
        // 명세(§9.2.3.3)는 LIN 2.x의 Signal_groups를 허용한다(무시될 뿐이다). ISO 17987에는 없다
        assert_eq!(n("2.0", "Signal_groups { }"), 0);
        assert_eq!(n("2.2", "Signal_groups { }"), 0);
        assert_eq!(n("ISO17987:2015", "Signal_groups { }"), 1);
        assert_eq!(n("2.2", "Diagnostic_addresses { }"), 1);
        assert_eq!(n("1.3", "Channel_name = A;\ndynamic_frames { 1 }"), 2);
    }
}
