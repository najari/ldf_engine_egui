//! 규칙별 설정(FR-CHK-06): 규칙을 끄거나 심각도를 바꾼다. 프로젝트에 저장되어 다른 PC에서 같은 `.ldfdb`를 열어도 유지된다.

use crate::{Category, all_rules, run_rules};
use ldf_model::{Diagnostic, LdfModel, Severity};
use std::collections::BTreeMap;

/// 규칙 하나의 설정.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleSetting {
    /// 켜져 있는지. 꺼진 규칙은 실행하지 않는다.
    pub enabled: bool,
    /// 기본 심각도를 대신할 값. `None`이면 규칙의 기본 심각도.
    pub severity: Option<Severity>,
}

impl Default for RuleSetting {
    fn default() -> Self {
        Self {
            enabled: true,
            severity: None,
        }
    }
}

/// 프로젝트의 규칙 설정. 기본값과 다른 규칙만 담는다.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuleSettings {
    overrides: BTreeMap<u16, RuleSetting>,
}

impl RuleSettings {
    /// 코드의 설정(없으면 기본값).
    pub fn get(&self, code: u16) -> RuleSetting {
        self.overrides.get(&code).copied().unwrap_or_default()
    }

    /// 코드의 설정을 바꾼다. 기본값과 같아지면 항목을 지운다.
    pub fn set(&mut self, code: u16, setting: RuleSetting) {
        if setting == RuleSetting::default() {
            self.overrides.remove(&code);
        } else {
            self.overrides.insert(code, setting);
        }
    }

    /// 기본값과 다른 설정들(코드 순).
    pub fn iter(&self) -> impl Iterator<Item = (u16, RuleSetting)> + '_ {
        self.overrides.iter().map(|(c, s)| (*c, *s))
    }

    /// 모든 규칙이 기본 설정인지.
    pub fn is_default(&self) -> bool {
        self.overrides.is_empty()
    }
}

/// 설정 화면에 보여 줄 규칙 정보.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleInfo {
    /// 규칙 번호.
    pub code: u16,
    /// 제목.
    pub title: &'static str,
    /// 기본 심각도.
    pub severity: Severity,
    /// 카테고리.
    pub category: Option<Category>,
}

/// 등록된 모든 규칙의 정보(코드 순).
pub fn rule_infos() -> Vec<RuleInfo> {
    all_rules()
        .iter()
        .map(|r| RuleInfo {
            code: r.code(),
            title: r.title(),
            severity: r.severity(),
            category: Category::of(r.code()),
        })
        .collect()
}

/// 설정에 따라 규칙을 실행한다. 꺼진 규칙은 건너뛰고, 심각도를 바꾼 규칙의 결과는 그 심각도로 낸다.
pub fn run_all_with(model: &LdfModel, settings: &RuleSettings) -> Vec<Diagnostic> {
    if settings.is_default() {
        return crate::run_all(model);
    }
    let rules: Vec<_> = all_rules()
        .into_iter()
        .filter(|r| settings.get(r.code()).enabled)
        .collect();
    let mut out = run_rules(model, &rules);
    for d in &mut out {
        if let Some(sev) = settings.get(d.code).severity {
            d.severity = sev;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::load;

    /// 0301(속도 범위)과 0406(프레임 배치)이 함께 나는 모델.
    const SRC: &str = "LIN_speed = 25 kbps;\nNodes { Master: M, 1 ms, 0 ms; Slaves: S; }\n\
                       Signals { A: 8, 0, M, S; }\nFrames { F: 1, M, 4 { A, 0; } }";

    fn codes(d: &[Diagnostic]) -> Vec<u16> {
        d.iter().map(|d| d.code).collect()
    }

    #[test]
    fn defaults_match_run_all() {
        let m = load(SRC).model;
        assert_eq!(
            run_all_with(&m, &RuleSettings::default()),
            crate::run_all(&m)
        );
    }

    #[test]
    fn a_disabled_rule_produces_nothing_and_others_remain() {
        let m = load(SRC).model;
        assert!(codes(&crate::run_all(&m)).contains(&301));
        let mut s = RuleSettings::default();
        s.set(
            301,
            RuleSetting {
                enabled: false,
                severity: None,
            },
        );
        let after = codes(&run_all_with(&m, &s));
        assert!(!after.contains(&301));
        assert!(after.contains(&406));
    }

    #[test]
    fn severity_can_be_overridden_per_rule() {
        let m = load(SRC).model;
        let mut s = RuleSettings::default();
        s.set(
            406,
            RuleSetting {
                enabled: true,
                severity: Some(Severity::Info),
            },
        );
        let out = run_all_with(&m, &s);
        assert!(
            out.iter()
                .filter(|d| d.code == 406)
                .all(|d| d.severity == Severity::Info)
        );
        assert!(
            out.iter()
                .any(|d| d.code == 301 && d.severity == Severity::Error)
        );
    }

    #[test]
    fn setting_back_to_the_default_forgets_the_override() {
        let mut s = RuleSettings::default();
        s.set(
            301,
            RuleSetting {
                enabled: false,
                severity: None,
            },
        );
        assert!(!s.is_default());
        s.set(301, RuleSetting::default());
        assert!(s.is_default());
        assert_eq!(s.iter().count(), 0);
    }

    #[test]
    fn rule_infos_list_every_registered_rule_once() {
        let infos = rule_infos();
        let mut seen: Vec<u16> = infos.iter().map(|i| i.code).collect();
        let n = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), n);
        assert!(
            infos
                .iter()
                .all(|i| !i.title.is_empty() && i.category.is_some())
        );
        assert!(infos.iter().any(|i| i.code == 406));
    }
}
