//! 규칙 0301: `LIN_speed`가 허용 범위(1–20 kbps)를 벗어났다.

use crate::Rule;
use crate::util::diag;
use ldf_model::{Diagnostic, LdfModel, Severity};

/// 규칙 0301.
pub(crate) struct R0301;
/// 등록용 인스턴스.
pub(crate) static RULE: R0301 = R0301;

impl Rule for R0301 {
    fn code(&self) -> u16 {
        301
    }

    fn title(&self) -> &'static str {
        "LIN speed out of range"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        if let Some(bps) = m.header.speed_bps
            && !(1_000..=20_000).contains(&bps)
        {
            out.push(diag(
                Severity::Error,
                301,
                None,
                format!(
                    "LIN_speed {} kbps가 허용 범위 1–20 kbps를 벗어났다",
                    f64::from(bps) / 1000.0
                ),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::load;

    #[test]
    fn speed_range() {
        let n = |s: &str| {
            let mut out = Vec::new();
            RULE.check(&load(&format!("LIN_speed = {s} kbps;")).model, &mut out);
            out.len()
        };
        assert_eq!(n("19.2"), 0);
        assert_eq!(n("10.417"), 0);
        assert_eq!(n("20"), 0);
        assert_eq!(n("21"), 1);
        assert_eq!(n("0.5"), 1);
    }
}
