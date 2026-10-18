//! Checker 창의 결과 트리: 카테고리 → 규칙 `[코드]` → 개별 결과(FR-CHK-03).
//! egui와 무관한 순수 로직이라 단위 테스트한다.

use ldf_check::Category;
use ldf_engine::ModelSnapshot;
use ldf_model::{Diagnostic, Severity};
use std::collections::{BTreeMap, HashMap, HashSet};

/// 결과 한 건.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// 심각도.
    pub severity: Severity,
    /// 설명(Inconsistency 열).
    pub message: String,
    /// 문제가 있는 원문 조각(Source 열).
    pub source: String,
    /// 위반한 규칙(Constraint 열).
    pub constraint: String,
    /// 1부터 시작하는 줄·열(위치를 모르면 `None`).
    pub line_col: Option<(usize, usize)>,
}

/// 규칙 하나와 그 결과들.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleGroup {
    /// 규칙 코드.
    pub code: u16,
    /// 규칙 제목.
    pub title: String,
    /// 결과 중 가장 높은 심각도.
    pub worst: Severity,
    /// 결과.
    pub items: Vec<Item>,
}

/// 카테고리 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// 표시 이름.
    pub label: String,
    /// 이 카테고리의 규칙(결과가 있는 것만).
    pub rules: Vec<RuleGroup>,
}

impl Group {
    /// 결과 수.
    pub fn count(&self) -> usize {
        self.rules.iter().map(|r| r.items.len()).sum()
    }
}

/// 결과 트리 전체.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CheckerTree {
    /// 카테고리(읽기 오류 → Basic → Format → Modelling → Parameter → Mapping 순).
    pub groups: Vec<Group>,
}

impl CheckerTree {
    /// 모든 결과 수.
    pub fn total(&self) -> usize {
        self.groups.iter().map(Group::count).sum()
    }
}

/// 펼침 상태의 키.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum OpenKey {
    /// 카테고리(트리 안 위치).
    Group(usize),
    /// 규칙(카테고리 위치, 규칙 위치).
    Rule(usize, usize),
}

/// 화면에 그릴 한 줄.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlatRow {
    /// 카테고리 줄: `Basic checks (OK)` 또는 `Modelling checks (6 results)`.
    Group {
        /// 위치.
        g: usize,
        /// 표시 텍스트.
        text: String,
        /// 결과가 있는지.
        has_results: bool,
    },
    /// 규칙 줄: `[0202] Node Attributes inconsistency (4)`.
    Rule {
        /// 카테고리 위치.
        g: usize,
        /// 규칙 위치.
        r: usize,
        /// 표시 텍스트.
        text: String,
        /// 가장 높은 심각도.
        worst: Severity,
    },
    /// 개별 결과 줄.
    Item {
        /// 카테고리 위치.
        g: usize,
        /// 규칙 위치.
        r: usize,
        /// 결과 위치.
        i: usize,
    },
}

const SYNTAX_LABEL: &str = "Reading errors";

fn snippet(src: &str, span: ldf_model::Span) -> String {
    let text = span.text(src);
    let first = text.lines().next().unwrap_or("").trim();
    let mut s: String = first.chars().take(48).collect();
    if first.chars().count() > 48 {
        s.push('…');
    }
    s
}

fn rule_titles() -> HashMap<u16, &'static str> {
    ldf_check::all_rules()
        .into_iter()
        .map(|r| (r.code(), r.title()))
        .collect()
}

/// 스냅샷의 진단으로 결과 트리를 만든다.
pub fn build(snap: &ModelSnapshot) -> CheckerTree {
    build_from(&snap.diagnostics, &snap.source, |off| snap.line_col(off))
}

fn build_from(
    diags: &[Diagnostic],
    src: &str,
    line_col: impl Fn(usize) -> (usize, usize),
) -> CheckerTree {
    let titles = rule_titles();
    // 카테고리 키: None = 읽기 오류, Some(c) = 규칙 카테고리
    let mut by_cat: BTreeMap<Option<Category>, BTreeMap<u16, Vec<Item>>> = BTreeMap::new();
    for d in diags {
        let cat = Category::of(d.code);
        let title = titles.get(&d.code).copied().map_or_else(
            || {
                if cat.is_none() {
                    "Syntax / reading".to_owned()
                } else {
                    format!("Rule {:04}", d.code)
                }
            },
            str::to_owned,
        );
        let item = Item {
            severity: d.severity,
            message: d.message.clone(),
            source: d.span.map_or_else(String::new, |s| snippet(src, s)),
            constraint: title,
            line_col: d.span.map(|s| line_col(s.start)),
        };
        by_cat
            .entry(cat)
            .or_default()
            .entry(d.code)
            .or_default()
            .push(item);
    }

    let order = [
        None,
        Some(Category::Basic),
        Some(Category::Format),
        Some(Category::Modelling),
        Some(Category::Parameter),
        Some(Category::Mapping),
    ];
    let mut groups = Vec::new();
    for cat in order {
        let rules_map = by_cat.remove(&cat);
        // 읽기 오류는 있을 때만, 규칙 카테고리는 항상(없으면 "(OK)") 보여준다
        if cat.is_none() && rules_map.is_none() {
            continue;
        }
        let label = cat.map_or_else(|| SYNTAX_LABEL.to_owned(), |c| c.label().to_owned());
        let rules = rules_map
            .unwrap_or_default()
            .into_iter()
            .map(|(code, items)| RuleGroup {
                code,
                title: items
                    .first()
                    .map(|i| i.constraint.clone())
                    .unwrap_or_default(),
                worst: items
                    .iter()
                    .map(|i| i.severity)
                    .max()
                    .unwrap_or(Severity::Info),
                items,
            })
            .collect();
        groups.push(Group { label, rules });
    }
    CheckerTree { groups }
}

/// 접힌 항목을 뺀 평탄한 줄 목록.
pub fn flatten(tree: &CheckerTree, collapsed: &HashSet<OpenKey>) -> Vec<FlatRow> {
    let mut rows = Vec::new();
    for (g, group) in tree.groups.iter().enumerate() {
        let n = group.count();
        let text = if n == 0 {
            format!("{} (OK)", group.label)
        } else {
            format!("{} ({n} results)", group.label)
        };
        rows.push(FlatRow::Group {
            g,
            text,
            has_results: n > 0,
        });
        if collapsed.contains(&OpenKey::Group(g)) {
            continue;
        }
        for (r, rule) in group.rules.iter().enumerate() {
            rows.push(FlatRow::Rule {
                g,
                r,
                text: format!("[{:04}] {} ({})", rule.code, rule.title, rule.items.len()),
                worst: rule.worst,
            });
            if collapsed.contains(&OpenKey::Rule(g, r)) {
                continue;
            }
            rows.extend((0..rule.items.len()).map(|i| FlatRow::Item { g, r, i }));
        }
    }
    rows
}

/// 보고서를 CSV로 만든다(FR-CHK-10). Excel에서 한글이 깨지지 않도록 UTF-8 BOM을 붙인다.
pub fn to_csv(tree: &CheckerTree) -> Vec<u8> {
    fn esc(s: &str) -> String {
        format!("\"{}\"", s.replace('"', "\"\""))
    }
    let mut out = String::from("Category,Code,Severity,Line,Column,Message,Source,Constraint\r\n");
    for g in &tree.groups {
        for r in &g.rules {
            for it in &r.items {
                let (line, col) = it
                    .line_col
                    .map_or((String::new(), String::new()), |(l, c)| {
                        (l.to_string(), c.to_string())
                    });
                let sev = match it.severity {
                    Severity::Info => "Info",
                    Severity::Warning => "Warning",
                    Severity::Error => "Error",
                };
                out.push_str(&format!(
                    "{},{:04},{sev},{line},{col},{},{},{}\r\n",
                    esc(&g.label),
                    r.code,
                    esc(&crate::i18n::tr(&it.message)),
                    esc(&it.source),
                    esc(&it.constraint)
                ));
            }
        }
    }
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(out.as_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::Span;

    const SRC: &str =
        "LIN_description_file;\nNodes { Master: M, 1 ms, 0 ms; }\nSignals { S: 4, 0, M, A; }\n";

    fn lc(off: usize) -> (usize, usize) {
        ldf_model::LineIndex::new(SRC).line_col(SRC, off)
    }

    fn diags() -> Vec<Diagnostic> {
        let sig = SRC.find("Signals").unwrap_or(0);
        vec![
            Diagnostic::error(9001, Span::new(0, 3), "알 수 없는 문자"),
            Diagnostic::warning(203, Span::new(sig, sig + 20), "프레임 ID 중복"),
            Diagnostic::info(406, Span::new(sig, sig + 5), "사용하지 않는 바이트"),
            Diagnostic::info(406, Span::new(0, 5), "또 하나"),
        ]
    }

    #[test]
    fn groups_follow_the_vector_order_and_empty_ones_are_ok() {
        let t = build_from(&diags(), SRC, lc);
        let labels: Vec<_> = t.groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Reading errors",
                "Basic checks",
                "LDF Format checks",
                "Modelling checks",
                "Parameter checks",
                "Mapping checks"
            ]
        );
        assert_eq!(t.total(), 4);
        let rows = flatten(&t, &HashSet::new());
        let FlatRow::Group {
            text, has_results, ..
        } = &rows[3]
        else {
            panic!("group row")
        };
        assert_eq!(text, "Basic checks (OK)");
        assert!(!has_results);
    }

    #[test]
    fn no_diagnostics_gives_five_ok_groups_and_no_reading_group() {
        let t = build_from(&[], SRC, lc);
        assert_eq!(t.groups.len(), 5);
        assert_eq!(t.total(), 0);
        let rows = flatten(&t, &HashSet::new());
        assert_eq!(rows.len(), 5);
        assert!(matches!(&rows[0], FlatRow::Group { text, .. } if text == "Basic checks (OK)"));
    }

    #[test]
    fn items_carry_position_snippet_and_constraint() {
        let t = build_from(&diags(), SRC, lc);
        let mapping = t
            .groups
            .iter()
            .find(|g| g.label == "Mapping checks")
            .expect("group");
        assert_eq!(mapping.count(), 2);
        let rule = &mapping.rules[0];
        assert_eq!(rule.code, 406);
        assert_eq!(rule.worst, Severity::Info);
        // 위치는 원문 순서와 상관없이 입력 순서를 유지한다
        let first = &rule.items[0];
        assert_eq!(first.line_col, Some((3, 1)));
        // 스팬은 5바이트(`Signa`)라 조각도 그만큼이다
        assert_eq!(first.source, "Signa");
        let modelling = t
            .groups
            .iter()
            .find(|g| g.label == "Modelling checks")
            .expect("group");
        assert_eq!(modelling.rules[0].items[0].source, "Signals { S: 4, 0, M");
    }

    #[test]
    fn long_source_snippets_are_cut() {
        let long = format!("{}\n", "x".repeat(100));
        let s = snippet(&long, Span::new(0, 100));
        assert_eq!(s.chars().count(), 49);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn collapsing_hides_children_only() {
        let t = build_from(&diags(), SRC, lc);
        let mapping = t
            .groups
            .iter()
            .position(|g| g.label == "Mapping checks")
            .expect("group");
        let all = flatten(&t, &HashSet::new());
        let mut collapsed = HashSet::new();
        collapsed.insert(OpenKey::Group(mapping));
        let fewer = flatten(&t, &collapsed);
        // 규칙 1줄 + 결과 2줄이 사라진다
        assert_eq!(all.len() - fewer.len(), 3);

        let mut collapsed = HashSet::new();
        collapsed.insert(OpenKey::Rule(mapping, 0));
        assert_eq!(all.len() - flatten(&t, &collapsed).len(), 2);
    }

    #[test]
    fn unknown_rule_codes_still_get_a_group_and_title() {
        let d = vec![Diagnostic::error(777, Span::new(0, 1), "?")];
        let t = build_from(&d, SRC, lc);
        // 777은 어느 카테고리에도 속하지 않으므로 읽기 오류 묶음에 들어간다
        assert_eq!(t.groups[0].label, "Reading errors");
        assert_eq!(t.groups[0].rules[0].title, "Syntax / reading");
        // 299는 Modelling 대역이므로 등록된 규칙이 아니어도 그 카테고리 아래 "Rule 0299"로 나온다
        let d = vec![Diagnostic::error(299, Span::new(0, 1), "?")];
        let t = build_from(&d, SRC, lc);
        let modelling = t
            .groups
            .iter()
            .find(|g| g.label == "Modelling checks")
            .expect("group");
        assert_eq!(modelling.rules[0].title, "Rule 0299");
    }

    #[test]
    fn csv_has_bom_header_and_escaped_cells() {
        let d = vec![Diagnostic::warning(
            203,
            Span::new(0, 3),
            "따옴표 \"포함\", 쉼표",
        )];
        let t = build_from(&d, SRC, lc);
        let bytes = to_csv(&t);
        assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF]);
        let text = String::from_utf8_lossy(&bytes[3..]).into_owned();
        assert!(
            text.starts_with("Category,Code,Severity,Line,Column,Message,Source,Constraint\r\n")
        );
        assert!(text.contains("\"따옴표 \"\"포함\"\", 쉼표\""));
        assert!(text.contains(",0203,Warning,1,1,"));
    }
}
