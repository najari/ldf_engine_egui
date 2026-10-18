//! `ldf-cli check`: 파일을 검사해 진단을 출력하고 종료 코드를 정한다.

use ldf_engine::analyze;
use ldf_model::{Diagnostic, LineIndex, Severity};
use serde_json::json;
use std::io::Write;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Text,
    Json,
}

/// 한 파일의 검사 결과.
struct FileReport {
    path: String,
    text: String,
    diagnostics: Vec<Diagnostic>,
}

fn severity_name(s: Severity) -> &'static str {
    match s {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Error => "error",
    }
}

/// 종료 코드: 오류가 하나라도 있으면 2, 경고가 있으면 1, 아니면 0(참고만 있으면 0).
fn exit_code(reports: &[FileReport]) -> u8 {
    let worst = reports
        .iter()
        .flat_map(|r| r.diagnostics.iter())
        .map(|d| d.severity)
        .max();
    match worst {
        Some(Severity::Error) => 2,
        Some(Severity::Warning) => 1,
        _ => 0,
    }
}

fn parse_args(args: &[String]) -> Result<(Vec<String>, Format), String> {
    let mut files = Vec::new();
    let mut format = Format::Text;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--format" => {
                format = match it.next().map(String::as_str) {
                    Some("text") => Format::Text,
                    Some("json") => Format::Json,
                    other => {
                        return Err(format!(
                            "--format 값은 text 또는 json이어야 한다: {other:?}"
                        ));
                    }
                }
            }
            flag if flag.starts_with("--") => return Err(format!("알 수 없는 옵션 `{flag}`")),
            file => files.push(file.to_owned()),
        }
    }
    if files.is_empty() {
        return Err("검사할 파일을 하나 이상 지정해야 한다".to_owned());
    }
    Ok((files, format))
}

fn write_text(out: &mut dyn Write, reports: &[FileReport]) -> std::io::Result<()> {
    for r in reports {
        let idx = LineIndex::new(&r.text);
        for d in &r.diagnostics {
            let (line, col) = d.span.map_or((0, 0), |s| idx.line_col(&r.text, s.start));
            writeln!(
                out,
                "{}:{line}:{col}: {} [{:04}] {}",
                r.path,
                severity_name(d.severity),
                d.code,
                d.message
            )?;
        }
    }
    let total: usize = reports.iter().map(|r| r.diagnostics.len()).sum();
    writeln!(out, "파일 {}개, 진단 {total}건", reports.len())
}

fn write_json(out: &mut dyn Write, reports: &[FileReport]) -> std::io::Result<()> {
    let files: Vec<_> = reports
        .iter()
        .map(|r| {
            let idx = LineIndex::new(&r.text);
            let diags: Vec<_> = r
                .diagnostics
                .iter()
                .map(|d| {
                    let (line, col) = d.span.map_or((0, 0), |s| idx.line_col(&r.text, s.start));
                    json!({
                        "code": d.code,
                        "severity": severity_name(d.severity),
                        "line": line,
                        "col": col,
                        "message": d.message,
                    })
                })
                .collect();
            json!({ "file": r.path, "diagnostics": diags })
        })
        .collect();
    let s = serde_json::to_string_pretty(&files).map_err(std::io::Error::other)?;
    writeln!(out, "{s}")
}

/// 명령을 실행하고 종료 코드를 돌려준다.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let (files, format) = match parse_args(args) {
        Ok(v) => v,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return 3;
        }
    };

    let mut reports = Vec::new();
    let mut failed = false;
    for f in files {
        match std::fs::read(Path::new(&f)) {
            Ok(bytes) => {
                let a = analyze(&bytes);
                reports.push(FileReport {
                    path: f,
                    text: a.text,
                    diagnostics: a.diagnostics,
                });
            }
            Err(e) => {
                let _ = writeln!(err, "{f}: 파일을 읽을 수 없다: {e}");
                failed = true;
            }
        }
    }

    let written = match format {
        Format::Text => write_text(out, &reports),
        Format::Json => write_json(out, &reports),
    };
    if written.is_err() {
        return 3;
    }
    if failed { 3 } else { exit_code(&reports) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str, content: &[u8]) -> String {
        let dir = std::env::temp_dir().join(format!("ldfcli-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let p = dir.join(name);
        std::fs::write(&p, content).expect("write");
        p.to_string_lossy().into_owned()
    }

    fn go(args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&args, &mut out, &mut err);
        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }

    const OK: &[u8] = b"LIN_description_file;\nLIN_protocol_version = \"2.1\";\nLIN_language_version = \"2.1\";\nLIN_speed = 19.2 kbps;\nNodes { Master: M, 1 ms, 0 ms; Slaves: A; }\n";

    #[test]
    fn syntax_errors_make_exit_code_two_and_print_positions() {
        let f = tmp(
            "broken.ldf",
            b"LIN_description_file;\n@ Nodes { Master: M, 1 ms, 0 ms; }\n",
        );
        let (code, out, _) = go(&[&f]);
        assert_eq!(code, 2);
        assert!(out.contains(":2:1: error [9001]"), "{out}");
        assert!(out.contains("파일 1개"));
    }

    #[test]
    fn json_format_is_valid_and_lists_positions() {
        let f = tmp("broken2.ldf", b"@\n");
        let (code, out, _) = go(&[&f, "--format", "json"]);
        assert_eq!(code, 2);
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid json");
        assert_eq!(v[0]["file"], f.as_str());
        assert_eq!(v[0]["diagnostics"][0]["code"], 9001);
        assert_eq!(v[0]["diagnostics"][0]["line"], 1);
    }

    #[test]
    fn a_missing_file_fails_with_three_but_other_files_are_still_checked() {
        let ok = tmp("fine.ldf", OK);
        let (code, out, err) = go(&["Z:/no/such.ldf", &ok]);
        assert_eq!(code, 3);
        assert!(err.contains("읽을 수 없다"));
        assert!(out.contains("파일 1개"));
    }

    #[test]
    fn bad_arguments_are_rejected() {
        assert_eq!(go(&[]).0, 3);
        assert_eq!(go(&["x.ldf", "--format", "xml"]).0, 3);
        assert_eq!(go(&["--bogus", "x.ldf"]).0, 3);
    }

    #[test]
    fn exit_code_ignores_info_only_results() {
        let info = Diagnostic::info(406, ldf_model::Span::new(0, 1), "i");
        let warn = Diagnostic::warning(203, ldf_model::Span::new(0, 1), "w");
        let report = |d: Vec<Diagnostic>| {
            vec![FileReport {
                path: "x".into(),
                text: String::new(),
                diagnostics: d,
            }]
        };
        assert_eq!(exit_code(&report(vec![])), 0);
        assert_eq!(exit_code(&report(vec![info.clone()])), 0);
        assert_eq!(exit_code(&report(vec![info, warn])), 1);
    }
}
