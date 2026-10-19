//! `ldf-cli`: LDF 배치 검사·비교·변환 CLI. GUI Checker와 같은 분석 경로(`ldf_engine::analyze`)를 쓴다.

mod args;
mod check;
mod convert;
mod diff;
mod ncf;
mod ncf_import;
mod project;

use ldf_i18n::{Lang, lang, set_lang, tf};
use std::io::Write;
use std::process::ExitCode;

const USAGE: &str = "\
사용법:
  ldf-cli check <파일...> [--format text|json]
  ldf-cli diff <이전.ldf> <이후.ldf> [--format text|json|csv|html] [-o 파일]
  ldf-cli convert <입력.ldf> -o <출력> [--to keep|2.0|2.1|2.2] [--eol lf|crlf]
                  [--encoding utf-8|utf-8-bom|windows-1252] [--format ldf|json] [--allow-errors]
  ldf-cli ncf <파일.ncf...> [--format text|json]
  ldf-cli ncf-import <입력.ldf> <노드.ncf> [-o 출력.ldf] [--node 이름] [--allow-errors]
  ldf-cli list <프로젝트.ldfdb>
  ldf-cli import <프로젝트.ldfdb> <파일...>
  ldf-cli export <프로젝트.ldfdb> -o <출력> [--revision N] [--format ldf|json] [--allow-errors]
  모든 명령에 --lang ko|en을 붙이면 출력 언어를 고른다(환경 변수 LDF_LANG도 같다. 기본 ko)

  check    LDF 파일을 검사하고 진단을 출력한다
  diff     두 LDF의 차이(추가·삭제·변경·이름 변경)를 낸다
  convert  다른 LIN 버전·줄바꿈·인코딩으로 다시 쓰고, 빠지는 항목은 stderr로 경고한다
  ncf      노드 기능 파일(NCF)을 읽어 노드·프레임·인코딩을 보여 준다(읽기 전용)
  ncf-import  NCF의 노드를 LDF의 슬레이브로 가져온다(-o가 없으면 미리 보기만)
  list/import/export  프로젝트 파일(리비전 저장소)을 스크립트로 다룬다

종료 코드: 0 = 성공/문제 없음/차이 없음, 1 = 경고(check) 또는 차이 있음(diff), 2 = 오류 진단,
          3 = 실행 실패(파일 읽기 실패, 잘못된 인자)
";

const USAGE_EN: &str = "\
Usage:
  ldf-cli check <file...> [--format text|json]
  ldf-cli diff <before.ldf> <after.ldf> [--format text|json|csv|html] [-o file]
  ldf-cli convert <input.ldf> -o <output> [--to keep|2.0|2.1|2.2] [--eol lf|crlf]
                  [--encoding utf-8|utf-8-bom|windows-1252] [--format ldf|json] [--allow-errors]
  ldf-cli ncf <file.ncf...> [--format text|json]
  ldf-cli ncf-import <input.ldf> <node.ncf> [-o output.ldf] [--node name] [--allow-errors]
  ldf-cli list <project.ldfdb>
  ldf-cli import <project.ldfdb> <file...>
  ldf-cli export <project.ldfdb> -o <output> [--revision N] [--format ldf|json] [--allow-errors]
  Add --lang ko|en to any command to choose the output language (the LDF_LANG environment variable does the same; default ko)

  check    Check LDF files and print the diagnostics
  diff     Report the differences between two LDFs (added, removed, changed, renamed)
  convert  Rewrite for another LIN version, line ending or encoding; items that get dropped are warned on stderr
  ncf      Read a Node Capability File (NCF) and show its node, frames and encodings (read-only)
  ncf-import  Import a node of an NCF into the LDF as a slave (preview only without -o)
  list/import/export  Work with a project file (the revision store) from scripts

Exit codes: 0 = success / no problems / no differences, 1 = warnings (check) or differences (diff), 2 = error diagnostics,
            3 = failed to run (unreadable file, bad arguments)
";

fn usage() -> &'static str {
    if lang() == Lang::En { USAGE_EN } else { USAGE }
}

/// 출력 언어를 정한다: `--lang ko|en`(인자 어디에 있든)이 환경 변수 `LDF_LANG`보다 앞선다.
/// `--lang`과 그 값을 뺀 인자와 고른 언어를 돌려준다.
fn take_lang(args: Vec<String>, env: Option<&str>) -> Result<(Vec<String>, Lang), String> {
    let parse = |v: &str| match v {
        "ko" => Ok(Lang::Ko),
        "en" => Ok(Lang::En),
        other => Err(tf!("--lang 값은 ko 또는 en이어야 한다: {other}")),
    };
    let mut lang = match env {
        Some(v) if !v.is_empty() => parse(v)?,
        _ => Lang::Ko,
    };
    let mut rest = Vec::new();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        if a == "--lang" {
            let v = it.next().ok_or_else(|| tf!("{a} 옵션에 값이 필요하다"))?;
            lang = parse(&v)?;
        } else {
            rest.push(a);
        }
    }
    Ok((rest, lang))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let env = std::env::var("LDF_LANG").ok();
    let (args, chosen) = match take_lang(args, env.as_deref()) {
        Ok(v) => v,
        Err(e) => {
            let _ = writeln!(stderr.lock(), "{e}");
            return ExitCode::from(3);
        }
    };
    set_lang(chosen);
    let code = run(&args, &mut stdout.lock(), &mut stderr.lock());
    ExitCode::from(code)
}

fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match args.first().map(String::as_str) {
        Some("check") => check::run(args.get(1..).unwrap_or(&[]), out, err),
        Some("diff") => diff::run(args.get(1..).unwrap_or(&[]), out, err),
        Some("convert") => convert::run(args.get(1..).unwrap_or(&[]), out, err),
        Some("ncf") => ncf::run(args.get(1..).unwrap_or(&[]), out, err),
        Some("ncf-import") => ncf_import::run(args.get(1..).unwrap_or(&[]), out, err),
        Some("list") => project::list(args.get(1..).unwrap_or(&[]), out, err),
        Some("import") => project::import(args.get(1..).unwrap_or(&[]), out, err),
        Some("export") => project::export(args.get(1..).unwrap_or(&[]), out, err),
        Some("--help" | "-h") | None => {
            let _ = write!(out, "{}", usage());
            u8::from(args.is_empty()) * 3
        }
        Some("--version" | "-V") => {
            let _ = writeln!(out, "ldf-cli {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some(other) => {
            let _ = writeln!(err, "{}\n\n{}", tf!("알 수 없는 명령 `{other}`"), usage());
            3
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn the_language_comes_from_the_flag_or_the_environment() {
        let v = |a: &[&str]| a.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let (rest, l) = take_lang(v(&["check", "a.ldf"]), None).expect("default");
        assert_eq!((rest, l), (v(&["check", "a.ldf"]), Lang::Ko));
        // 플래그는 어디에 있어도 되고 인자에서 빠진다
        let (rest, l) = take_lang(v(&["check", "--lang", "en", "a.ldf"]), None).expect("flag");
        assert_eq!((rest, l), (v(&["check", "a.ldf"]), Lang::En));
        // 환경 변수는 플래그보다 약하다
        assert_eq!(take_lang(v(&["x"]), Some("en")).expect("env").1, Lang::En);
        assert_eq!(
            take_lang(v(&["--lang", "ko"]), Some("en")).expect("both").1,
            Lang::Ko
        );
        assert_eq!(
            take_lang(v(&["x"]), Some("")).expect("empty env").1,
            Lang::Ko
        );
        // 잘못된 값, 값이 없는 플래그
        assert!(take_lang(v(&["--lang", "fr"]), None).is_err());
        assert!(take_lang(v(&["--lang"]), None).is_err());
        assert!(take_lang(v(&["x"]), Some("de")).is_err());
    }

    #[test]
    fn help_version_and_unknown_command() {
        let (code, out, _) = go(&["--help"]);
        assert_eq!(code, 0);
        assert!(out.contains("사용법"));
        let (code, out, _) = go(&["--version"]);
        assert_eq!(code, 0);
        assert!(out.starts_with("ldf-cli "));
        let (code, _, err) = go(&["frobnicate"]);
        assert_eq!(code, 3);
        assert!(err.contains("알 수 없는 명령"));
        // 인자가 없으면 사용법을 보여주고 실패로 본다
        assert_eq!(go(&[]).0, 3);
    }
}
