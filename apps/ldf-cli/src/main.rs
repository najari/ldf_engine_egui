//! `ldf-cli`: LDF 배치 검사 CLI. GUI Checker와 같은 분석 경로(`ldf_engine::analyze`)를 쓴다.

mod check;

use std::io::Write;
use std::process::ExitCode;

const USAGE: &str = "\
사용법: ldf-cli check <파일...> [--format text|json]

  check    LDF 파일을 검사하고 진단을 출력한다
  --format 출력 형식 (기본 text)

종료 코드: 0 = 문제 없음, 1 = 경고, 2 = 오류, 3 = 실행 실패(파일 읽기 실패, 잘못된 인자)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let code = run(&args, &mut stdout.lock(), &mut stderr.lock());
    ExitCode::from(code)
}

fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match args.first().map(String::as_str) {
        Some("check") => check::run(args.get(1..).unwrap_or(&[]), out, err),
        Some("--help" | "-h") | None => {
            let _ = write!(out, "{USAGE}");
            u8::from(args.is_empty()) * 3
        }
        Some("--version" | "-V") => {
            let _ = writeln!(out, "ldf-cli {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some(other) => {
            let _ = writeln!(err, "알 수 없는 명령 `{other}`\n\n{USAGE}");
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
