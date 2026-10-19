//! `ldf-cli diff <이전.ldf> <이후.ldf>`: 두 LDF의 차이(FR-CLI-03, FR-REV-03과 같은 결과).

use crate::args::Args;
use ldf_engine::{analyze, diff_report};
use ldf_i18n::{tf, twriteln};
use std::io::Write;

/// 종료 코드: 0 = 차이 없음, 1 = 차이 있음, 3 = 실행 실패.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match Args::parse(args, &["--format", "-o"], &[]) {
        Ok(p) => p,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return 3;
        }
    };
    let [before, after] = parsed.positional.as_slice() else {
        let _ = twriteln!(err, "diff는 파일 두 개(이전, 이후)가 필요하다");
        return 3;
    };
    let format = parsed.value("--format").unwrap_or("text");
    if !matches!(format, "text" | "json" | "csv" | "html") {
        let _ = twriteln!(
            err,
            "--format 값은 text, json, csv, html 중 하나여야 한다: {format}"
        );
        return 3;
    }
    let model = |path: &str| -> Result<_, String> {
        let bytes = std::fs::read(path).map_err(|e| tf!("{path}: 파일을 읽을 수 없다: {e}"))?;
        Ok(analyze(&bytes).model)
    };
    let (a, b) = match (model(before), model(after)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            let _ = writeln!(err, "{e}");
            return 3;
        }
    };
    let d = ldf_model::diff(&a, &b);
    let bytes: Vec<u8> = match format {
        "json" => diff_report::to_json(&d).into_bytes(),
        "csv" => diff_report::to_csv(&d),
        "html" => diff_report::to_html(&d, before, after).into_bytes(),
        _ => diff_report::to_text(&d, before, after).into_bytes(),
    };
    let written = match parsed.value("-o") {
        Some(path) => std::fs::write(path, &bytes),
        None => out.write_all(&bytes),
    };
    if let Err(e) = written {
        let _ = twriteln!(err, "출력을 쓸 수 없다: {e}");
        return 3;
    }
    u8::from(!d.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str, content: &str) -> String {
        let dir = std::env::temp_dir().join(format!("ldfcli-diff-{}", std::process::id()));
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

    const A: &str = "LIN_speed = 19.2 kbps;\nNodes { Master: M, 1 ms, 0 ms; Slaves: S; }\nSignals { X: 8, 0, M, S; }\n";
    const B: &str = "LIN_speed = 10.4 kbps;\nNodes { Master: M, 1 ms, 0 ms; Slaves: S, T; }\nSignals { X: 8, 0, M, S; }\n";

    #[test]
    fn identical_files_exit_zero_and_different_files_exit_one() {
        let a = tmp("a.ldf", A);
        let same = tmp("a2.ldf", &format!("// 주석만 다르다\n{A}"));
        let b = tmp("b.ldf", B);
        let (code, out, _) = go(&[&a, &same]);
        assert_eq!(code, 0);
        assert!(out.contains("차이 없음"), "{out}");
        let (code, out, _) = go(&[&a, &b]);
        assert_eq!(code, 1);
        assert!(
            out.contains("speed_bps: 19200 → 10400") && out.contains("+ Node T"),
            "{out}"
        );
    }

    #[test]
    fn json_csv_and_html_formats() {
        let (a, b) = (tmp("fa.ldf", A), tmp("fb.ldf", B));
        let (_, json, _) = go(&[&a, &b, "--format", "json"]);
        let v: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert!(v["entries"].as_array().is_some_and(|e| !e.is_empty()));
        let (_, csv, _) = go(&[&a, &b, "--format", "csv"]);
        assert!(
            csv.contains("Object,Name,Change,Field,Before,After"),
            "{csv}"
        );
        let (_, html, _) = go(&[&a, &b, "--format", "html"]);
        assert!(html.contains("<table>"));
        // 파일로 저장
        let target = tmp("report.html", "");
        let (code, out, _) = go(&[&a, &b, "--format", "html", "-o", &target]);
        assert_eq!((code, out.as_str()), (1, ""));
        assert!(
            std::fs::read_to_string(&target)
                .expect("read")
                .contains("<table>")
        );
    }

    #[test]
    fn bad_input_fails_with_three() {
        let a = tmp("ok.ldf", A);
        assert_eq!(go(&[&a]).0, 3);
        assert_eq!(go(&[&a, "Z:/nope.ldf"]).0, 3);
        assert_eq!(go(&[&a, &a, "--format", "xml"]).0, 3);
    }
}
