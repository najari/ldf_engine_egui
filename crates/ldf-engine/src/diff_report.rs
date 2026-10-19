//! 리비전 비교 결과를 사람이 읽는 형식(텍스트·CSV·HTML)과 JSON으로 만든다(FR-REV-04, FR-CLI-03).

//!
//! 문구는 화면 언어(`ldf_i18n`)를 따른다. 파일에 쓰는 보고서도 지금 고른 언어로 만들어진다.

use ldf_i18n::{Lang, change_label, lang, t, tf};
use ldf_model::{Change, Diff, DiffEntry};

fn shown(v: &str) -> &str {
    if v.is_empty() { t("(없음)") } else { v }
}

/// 이름 변경이면 `옛 이름 → 새 이름`, 아니면 이름.
fn title_of(e: &DiffEntry) -> String {
    match &e.change {
        Change::Renamed { from } => format!("{from} → {}", e.name),
        _ => e.name.clone(),
    }
}

/// 터미널용 텍스트.
pub fn to_text(d: &Diff, before: &str, after: &str) -> String {
    let (added, removed, changed, renamed) = d.counts();
    let mut out = format!("--- {before}\n+++ {after}\n");
    if d.is_empty() {
        out.push_str(&format!("{}\n", t("차이 없음")));
        return out;
    }
    out.push_str(&tf!(
        "추가 {added}, 삭제 {removed}, 변경 {changed}, 이름 변경 {renamed}\n"
    ));
    for e in &d.entries {
        let mark = match e.change {
            Change::Added => '+',
            Change::Removed => '-',
            Change::Changed => '~',
            Change::Renamed { .. } => '>',
        };
        out.push_str(&format!("\n{mark} {} {}\n", e.kind.label(), title_of(e)));
        for f in &e.fields {
            match e.change {
                Change::Added => out.push_str(&format!("    {}: {}\n", f.field, shown(&f.after))),
                Change::Removed => {
                    out.push_str(&format!("    {}: {}\n", f.field, shown(&f.before)))
                }
                _ => out.push_str(&format!(
                    "    {}: {} → {}\n",
                    f.field,
                    shown(&f.before),
                    shown(&f.after)
                )),
            }
        }
    }
    out
}

fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

/// CSV(UTF-8 BOM 포함: 한국어 Windows의 Excel에서 바로 열린다). 필드 하나가 한 줄이고, 필드가 없는 변경은 필드 칸이 빈 한 줄이다.
pub fn to_csv(d: &Diff) -> Vec<u8> {
    let mut out = String::from("\u{feff}Object,Name,Change,Field,Before,After\r\n");
    for e in &d.entries {
        let rows: Vec<(String, String, String)> = if e.fields.is_empty() {
            vec![(String::new(), String::new(), String::new())]
        } else {
            e.fields
                .iter()
                .map(|f| (f.field.clone(), f.before.clone(), f.after.clone()))
                .collect()
        };
        for (field, before, after) in rows {
            out.push_str(&format!(
                "{},{},{},{},{},{}\r\n",
                csv_cell(e.kind.label()),
                csv_cell(&title_of(e)),
                csv_cell(change_label(e.change.label())),
                csv_cell(&field),
                csv_cell(&before),
                csv_cell(&after),
            ));
        }
    }
    out.into_bytes()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const HTML_STYLE: &str = "<style>\n\
body{font-family:Segoe UI,Malgun Gothic,sans-serif;margin:24px;color:#222}\n\
table{border-collapse:collapse;width:100%}\nth,td{border:1px solid #ccc;padding:4px 8px;text-align:left;vertical-align:top}\n\
th{background:#f0f0f0}\ntr.added td:first-child{background:#e6ffed}\ntr.removed td:first-child{background:#ffeef0}\n\
tr.changed td:first-child,tr.renamed td:first-child{background:#fff8c5}\n.before{color:#a40000}.after{color:#006400}\n</style>";

/// 공급사에 그대로 보낼 수 있는 단독 HTML 문서.
pub fn to_html(d: &Diff, before: &str, after: &str) -> String {
    let (added, removed, changed, renamed) = d.counts();
    let title = t("LDF 비교");
    let html_lang = if lang() == Lang::En { "en" } else { "ko" };
    let mut out = format!(
        "<!doctype html>\n<html lang=\"{html_lang}\"><head><meta charset=\"utf-8\">\n<title>{title}</title>\n{HTML_STYLE}</head><body>\n"
    );
    out.push_str(&format!(
        "<h1>{title}</h1>\n<p>{}: <b>{}</b><br>{}: <b>{}</b></p>\n<p>{}</p>\n",
        t("이전"),
        esc(before),
        t("이후"),
        esc(after),
        tf!("추가 {added}, 삭제 {removed}, 변경 {changed}, 이름 변경 {renamed}")
    ));
    if d.is_empty() {
        out.push_str(&format!("<p>{}</p>\n</body></html>\n", t("차이 없음")));
        return out;
    }
    out.push_str(&format!(
        "<table>\n<tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th></tr>\n",
        t("종류"),
        t("이름"),
        t("변경"),
        t("필드"),
        t("이전"),
        t("이후")
    ));
    for e in &d.entries {
        let class = match e.change {
            Change::Added => "added",
            Change::Removed => "removed",
            Change::Changed => "changed",
            Change::Renamed { .. } => "renamed",
        };
        let rows = e.fields.len().max(1);
        for i in 0..rows {
            out.push_str(&format!("<tr class=\"{class}\">"));
            if i == 0 {
                out.push_str(&format!(
                    "<td rowspan=\"{rows}\">{}</td><td rowspan=\"{rows}\">{}</td><td rowspan=\"{rows}\">{}</td>",
                    esc(e.kind.label()),
                    esc(&title_of(e)),
                    esc(change_label(e.change.label()))
                ));
            }
            match e.fields.get(i) {
                Some(f) => out.push_str(&format!(
                    "<td>{}</td><td class=\"before\">{}</td><td class=\"after\">{}</td>",
                    esc(&f.field),
                    esc(&f.before),
                    esc(&f.after)
                )),
                None => out.push_str("<td></td><td></td><td></td>"),
            }
            out.push_str("</tr>\n");
        }
    }
    out.push_str("</table>\n</body></html>\n");
    out
}
/// JSON(들여 쓴 문자열).
pub fn to_json(d: &Diff) -> String {
    serde_json::to_string_pretty(d).unwrap_or_else(|_| "{}".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::{EditOp, diff, load};

    fn sample() -> Diff {
        let a = load(
            "LIN_speed = 19.2 kbps;\nNodes { Master: M, 1 ms, 0 ms; Slaves: S; }\nSignals { A: 8, 0, M, S; }\nFrames { F: 1, M, 1 { A, 0; } }",
        )
        .model;
        let mut b = a.clone();
        EditOp::Batch(vec![
            EditOp::SetSpeed(10_400),
            EditOp::AddSlave("T1".into()),
            EditOp::DeleteSignal("A".into()),
        ])
        .apply(&mut b)
        .expect("apply");
        diff(&a, &b)
    }

    #[test]
    fn text_lists_each_change_with_before_and_after() {
        let t = to_text(&sample(), "old.ldf", "new.ldf");
        assert!(t.starts_with("--- old.ldf\n+++ new.ldf\n"));
        assert!(t.contains("speed_bps: 19200 → 10400"), "{t}");
        assert!(t.contains("+ Node T1"), "{t}");
        assert!(t.contains("- Signal A"), "{t}");
        assert!(to_text(&Diff::default(), "a", "b").contains("차이 없음"));
    }

    #[test]
    fn csv_quotes_cells_and_starts_with_a_bom() {
        let bytes = to_csv(&sample());
        assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
        let s = String::from_utf8(bytes).expect("utf8");
        assert!(s.contains("Object,Name,Change,Field,Before,After\r\n"));
        assert!(
            s.contains("Header,header,변경,speed_bps,19200,10400\r\n"),
            "{s}"
        );
        let tricky = Diff {
            entries: vec![DiffEntry {
                kind: ldf_model::ObjectKind::Signal,
                name: "a,b".into(),
                change: Change::Changed,
                fields: vec![ldf_model::FieldChange {
                    field: "init".into(),
                    before: "\"x\"".into(),
                    after: "1\n2".into(),
                }],
            }],
        };
        let t = String::from_utf8(to_csv(&tricky)).expect("utf8");
        assert!(
            t.contains("\"a,b\",변경,init,\"\"\"x\"\"\",\"1\n2\""),
            "{t}"
        );
    }

    #[test]
    fn html_escapes_names_and_values() {
        let nasty = Diff {
            entries: vec![DiffEntry {
                kind: ldf_model::ObjectKind::Node,
                name: "T<&>".into(),
                change: Change::Added,
                fields: vec![ldf_model::FieldChange {
                    field: "role".into(),
                    before: String::new(),
                    after: "<script>".into(),
                }],
            }],
        };
        let h = to_html(&nasty, "a<b>.ldf", "c&d.ldf");
        assert!(
            h.contains("a&lt;b&gt;.ldf") && h.contains("c&amp;d.ldf"),
            "{h}"
        );
        assert!(
            h.contains("T&lt;&amp;&gt;") && h.contains("&lt;script&gt;"),
            "{h}"
        );
        assert!(!h.contains("T<&>") && !h.contains("<script>"));
        let normal = to_html(&sample(), "a", "b");
        assert!(normal.contains("<table>") && normal.contains("class=\"added\""));
    }
    #[test]
    fn json_round_trips_the_entries() {
        let j: serde_json::Value = serde_json::from_str(&to_json(&sample())).expect("json");
        let entries = j["entries"].as_array().expect("entries");
        assert!(
            entries
                .iter()
                .any(|e| e["kind"] == "node" && e["change"] == "added")
        );
        assert!(
            entries
                .iter()
                .any(|e| e["change"] == "removed" && e["name"] == "A")
        );
    }
}
