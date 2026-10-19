//! 영어 출력(`--lang en`, `LDF_LANG=en`). 언어는 프로세스 전역이라 실행 파일을 따로 띄워 시험한다.

#![allow(clippy::expect_used)] // 통합 시험의 도우미 함수는 clippy.toml의 시험 예외에 들지 않는다

use std::path::PathBuf;
use std::process::{Command, Output};

const BROKEN: &str = "LIN_description_file;\n\
LIN_protocol_version = \"2.1\";\nLIN_language_version = \"2.1\";\nLIN_speed = 19.2 kbps;\n\
Nodes { Master: M, 1 ms, 0 ms; Slaves: S; }\n\
Signals { A: 8, 0, Ghost, S; }\n\
Frames { F: 1, M, 1 { A, 0; Missing, 8; } }\n\
Schedule_tables { T { F delay 10 ms; } }\n";

const OTHER: &str = "LIN_description_file;\n\
LIN_protocol_version = \"2.1\";\nLIN_language_version = \"2.1\";\nLIN_speed = 10.4 kbps;\n\
Nodes { Master: M, 1 ms, 0 ms; Slaves: S; }\n\
Signals { A: 8, 0, M, S; }\n\
Frames { F: 1, M, 1 { A, 0; } }\n\
Schedule_tables { T { F delay 10 ms; } }\n";

fn has_hangul(s: &str) -> bool {
    s.chars().any(|c| ('\u{AC00}'..='\u{D7A3}').contains(&c))
}

fn write(name: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ldfcli-en-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let p = dir.join(name);
    std::fs::write(&p, text).expect("write");
    p
}

fn run(args: &[&str]) -> Output {
    run_env(args, None)
}

fn run_env(args: &[&str], env_lang: Option<&str>) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_ldf-cli"));
    c.args(args).env_remove("LDF_LANG");
    if let Some(l) = env_lang {
        c.env("LDF_LANG", l);
    }
    c.output().expect("run")
}

fn text(o: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

#[test]
fn check_prints_english_diagnostics_with_the_flag_or_the_variable() {
    let f = write("broken.ldf", BROKEN);
    let f = f.to_str().expect("path");
    for (args, env) in [
        (vec!["check", f, "--lang", "en"], None),
        (vec!["--lang", "en", "check", f], None),
        (vec!["check", f], Some("en")),
    ] {
        let (out, err) = text(&run_env(&args, env));
        assert!(
            !has_hangul(&out) && !has_hangul(&err),
            "{args:?}\n{out}\n{err}"
        );
        assert!(out.contains("is not defined"), "{out}");
        assert!(out.contains("1 files, "), "{out}");
    }
    // 기본은 한국어
    let (out, _) = text(&run(&["check", f]));
    assert!(out.contains("정의되지 않았다"), "{out}");
    // JSON 메시지도 같은 언어다
    let (out, _) = text(&run(&["check", f, "--format", "json", "--lang", "en"]));
    assert!(!has_hangul(&out), "{out}");
}

#[test]
fn diff_reports_follow_the_language() {
    let a = write("a.ldf", OTHER);
    let b = write(
        "b.ldf",
        &OTHER
            .replace("10.4 kbps", "19.2 kbps")
            .replace("Slaves: S;", "Slaves: S, T;"),
    );
    let (a, b) = (a.to_str().expect("path"), b.to_str().expect("path"));
    for format in ["text", "csv", "html"] {
        let (out, err) = text(&run(&["diff", a, b, "--format", format, "--lang", "en"]));
        assert!(
            !has_hangul(&out) && !has_hangul(&err),
            "{format}\n{out}\n{err}"
        );
    }
    let (text_out, _) = text(&run(&["diff", a, b, "--lang", "en"]));
    assert!(
        text_out.contains("1 added, 0 removed, 1 changed, 0 renamed"),
        "{text_out}"
    );
    let (html, _) = text(&run(&["diff", a, b, "--format", "html", "--lang", "en"]));
    assert!(
        html.contains("<html lang=\"en\">") && html.contains("<th>Name</th>"),
        "{html}"
    );
    let (html_ko, _) = text(&run(&["diff", a, b, "--format", "html"]));
    assert!(
        html_ko.contains("<html lang=\"ko\">") && html_ko.contains("<th>이름</th>"),
        "{html_ko}"
    );
    // 변경 종류 이름은 CSV에서도 영어다
    let (csv, _) = text(&run(&["diff", a, b, "--format", "csv", "--lang", "en"]));
    assert!(csv.contains("Added") && csv.contains("Changed"), "{csv}");
}

#[test]
fn usage_and_argument_errors_are_translated() {
    let (out, _) = text(&run(&["--help", "--lang", "en"]));
    assert!(out.starts_with("Usage:") && !has_hangul(&out), "{out}");
    let (out, _) = text(&run(&["--help"]));
    assert!(out.starts_with("사용법:"), "{out}");
    let o = run(&["frobnicate", "--lang", "en"]);
    let (_, err) = text(&o);
    assert_eq!(o.status.code(), Some(3));
    assert!(
        err.contains("Unknown command `frobnicate`") && !has_hangul(&err),
        "{err}"
    );
    // --lang 자체의 잘못된 값
    let o = run(&["check", "--lang", "fr"]);
    assert_eq!(o.status.code(), Some(3));
    assert!(text(&o).1.contains("--lang"), "{:?}", text(&o));
    // 환경 변수 값이 틀려도 같다
    assert_eq!(run_env(&["--help"], Some("de")).status.code(), Some(3));
}

#[test]
fn convert_warnings_and_failures_are_translated() {
    let f = write(
        "v22.ldf",
        &OTHER
            .replace("\"2.1\"", "\"2.2\"")
            .replace("kbps;", "kbps;\nChannel_name = \"Body\";"),
    );
    let out = write("out20.ldf", "");
    let (f, out) = (f.to_str().expect("path"), out.to_str().expect("path"));
    let o = run(&[
        "convert",
        f,
        "-o",
        out,
        "--to",
        "2.0",
        "--allow-errors",
        "--lang",
        "en",
    ]);
    let (stdout, stderr) = text(&o);
    assert!(
        !has_hangul(&stdout) && !has_hangul(&stderr),
        "{stdout}\n{stderr}"
    );
    assert!(stderr.contains("Warning: "), "{stderr}");
    assert!(stderr.contains("Channel_name"), "{stderr}");
    // 입력 누락
    let (_, err) = text(&run(&["convert", "--lang", "en"]));
    assert!(err.contains("convert needs one input file"), "{err}");
}
