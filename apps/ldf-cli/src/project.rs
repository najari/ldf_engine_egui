//! `ldf-cli list|import|export <프로젝트.ldfdb> ...`: 프로젝트 파일(리비전 저장소)을 스크립트로 다룬다(FR-CLI-04).

use crate::args::Args;
use ldf_engine::{ExportFormat, ExportOptions, Session};
use std::io::Write;
use std::path::Path;

fn open(path: &str, err: &mut dyn Write) -> Option<Session> {
    match Session::open(Path::new(path)) {
        Ok(s) => Some(s),
        Err(e) => {
            let _ = writeln!(err, "{path}: 프로젝트를 열 수 없다: {e}");
            None
        }
    }
}

/// `list <프로젝트>`: 리비전 목록(ID, 부모, 시각, 이름).
pub fn list(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let [project] = args else {
        let _ = writeln!(err, "list는 프로젝트 파일 하나가 필요하다");
        return 3;
    };
    let Some(s) = open(project, err) else {
        return 3;
    };
    match s.revisions() {
        Ok(revs) => {
            for r in revs {
                let parent = r
                    .parent_id
                    .map_or_else(|| "-".to_owned(), |p| p.to_string());
                let _ = writeln!(
                    out,
                    "{}\t{}\t{}\t{}\t{}",
                    r.id,
                    parent,
                    r.created_at,
                    r.protocol_version.as_deref().unwrap_or("?"),
                    r.label
                );
            }
            0
        }
        Err(e) => {
            let _ = writeln!(err, "{e}");
            3
        }
    }
}

/// `import <프로젝트> <파일...>`: 파일마다 리비전을 만든다(같은 내용이면 기존 리비전을 쓴다). 줄마다 `ID<TAB>파일`.
pub fn import(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let [project, files @ ..] = args else {
        let _ = writeln!(err, "import는 프로젝트 파일과 LDF 파일이 필요하다");
        return 3;
    };
    if files.is_empty() {
        let _ = writeln!(err, "가져올 LDF 파일을 하나 이상 지정해야 한다");
        return 3;
    }
    let Some(mut s) = open(project, err) else {
        return 3;
    };
    let mut failed = false;
    for f in files {
        match s.import_file(Path::new(f)) {
            Ok(o) => {
                let note = if o.reused {
                    "\t(같은 내용이 이미 있어 기존 리비전)"
                } else {
                    ""
                };
                let _ = writeln!(out, "{}\t{f}{note}", o.snapshot.revision.id);
            }
            Err(e) => {
                let _ = writeln!(err, "{f}: {e}");
                failed = true;
            }
        }
    }
    if failed { 3 } else { 0 }
}

/// `export <프로젝트> [--revision N] -o <출력> [--format ldf|json]`: 리비전(기본: 가장 최근)을 꺼낸다.
pub fn export(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match Args::parse(args, &["--revision", "-o", "--format"], &["--allow-errors"]) {
        Ok(p) => p,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return 3;
        }
    };
    let [project] = parsed.positional.as_slice() else {
        let _ = writeln!(err, "export는 프로젝트 파일 하나가 필요하다");
        return 3;
    };
    let Some(output) = parsed.value("-o") else {
        let _ = writeln!(err, "-o <출력 파일>이 필요하다");
        return 3;
    };
    let Some(mut s) = open(project, err) else {
        return 3;
    };
    let revision = match parsed.value("--revision") {
        Some(v) => match v.parse::<i64>() {
            Ok(n) => n,
            Err(_) => {
                let _ = writeln!(err, "--revision 값은 정수여야 한다: {v}");
                return 3;
            }
        },
        None => match s.revisions().map(|r| r.last().map(|x| x.id)) {
            Ok(Some(id)) => id,
            Ok(None) => {
                let _ = writeln!(err, "프로젝트에 리비전이 없다");
                return 3;
            }
            Err(e) => {
                let _ = writeln!(err, "{e}");
                return 3;
            }
        },
    };
    let mut opts = ExportOptions {
        allow_errors: parsed.flag("--allow-errors"),
        ..ExportOptions::default()
    };
    match parsed.value("--format") {
        None | Some("ldf") => {}
        Some("json") => opts.format = ExportFormat::Json,
        Some(other) => {
            let _ = writeln!(err, "--format 값은 ldf 또는 json이어야 한다: {other}");
            return 3;
        }
    }
    match s.export(revision, Path::new(output), &opts) {
        Ok(r) => {
            let _ = writeln!(out, "리비전 {revision} -> {output} ({} 바이트)", r.bytes);
            0
        }
        Err(ldf_engine::EngineError::ExportBlocked { errors }) => {
            let _ = writeln!(
                err,
                "오류 진단이 {errors}개 남아 있어 쓰지 않았다(--allow-errors)"
            );
            2
        }
        Err(e) => {
            let _ = writeln!(err, "{e}");
            3
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ldfcli-proj-{}", std::process::id()));
        std::fs::create_dir_all(&d).expect("dir");
        d
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    const A: &str = "LIN_speed = 19.2 kbps;\nNodes { Master: M, 1 ms, 0 ms; Slaves: S; }\n";

    #[test]
    fn scripts_can_stack_revisions_and_take_them_out_again() {
        let d = dir();
        let db = d.join("p.ldfdb");
        let _ = std::fs::remove_file(&db);
        let (f1, f2) = (d.join("v1.ldf"), d.join("v2.ldf"));
        std::fs::write(&f1, A).expect("write");
        std::fs::write(&f2, format!("{A}// 두 번째 판\n")).expect("write");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let p = db.to_str().expect("p");
        assert_eq!(
            import(
                &s(&[p, f1.to_str().expect("p"), f2.to_str().expect("p")]),
                &mut out,
                &mut err
            ),
            0
        );
        let imported = String::from_utf8_lossy(&out).into_owned();
        assert_eq!(imported.lines().count(), 2, "{imported}");

        // 같은 파일을 다시 가져오면 기존 리비전을 쓴다
        out.clear();
        assert_eq!(
            import(&s(&[p, f1.to_str().expect("p")]), &mut out, &mut err),
            0
        );
        assert!(String::from_utf8_lossy(&out).contains("기존 리비전"));

        out.clear();
        assert_eq!(list(&s(&[p]), &mut out, &mut err), 0);
        let listing = String::from_utf8_lossy(&out).into_owned();
        assert_eq!(listing.lines().count(), 2, "{listing}");

        // 기본은 가장 최근 리비전, --revision으로 고른다
        let latest = d.join("latest.ldf");
        out.clear();
        assert_eq!(
            export(
                &s(&[p, "-o", latest.to_str().expect("p"), "--allow-errors"]),
                &mut out,
                &mut err
            ),
            0,
            "{}",
            String::from_utf8_lossy(&err)
        );
        assert!(
            std::fs::read_to_string(&latest)
                .expect("read")
                .contains("두 번째 판")
        );
        let first = d.join("first.ldf");
        assert_eq!(
            export(
                &s(&[
                    p,
                    "--revision",
                    "1",
                    "-o",
                    first.to_str().expect("p"),
                    "--allow-errors"
                ]),
                &mut out,
                &mut err
            ),
            0
        );
        assert_eq!(std::fs::read_to_string(&first).expect("read"), A);
        let json = d.join("m.json");
        assert_eq!(
            export(
                &s(&[p, "--format", "json", "-o", json.to_str().expect("p")]),
                &mut out,
                &mut err
            ),
            0
        );
        assert!(
            std::fs::read_to_string(&json)
                .expect("read")
                .contains("\"format\": \"ldf-model\"")
        );
    }

    #[test]
    fn bad_arguments_and_missing_things_fail_with_three() {
        let mut sink = Vec::new();
        let mut err = Vec::new();
        assert_eq!(list(&s(&[]), &mut sink, &mut err), 3);
        assert_eq!(import(&s(&["only-project.ldfdb"]), &mut sink, &mut err), 3);
        assert_eq!(export(&s(&["p.ldfdb"]), &mut sink, &mut err), 3);
        let d = dir();
        let db = d.join("empty.ldfdb");
        let _ = std::fs::remove_file(&db);
        let p = db.to_str().expect("p");
        assert_eq!(
            export(&s(&[p, "-o", "x"]), &mut sink, &mut err),
            3,
            "리비전이 없으면 실패"
        );
        assert_eq!(
            export(
                &s(&[p, "--revision", "abc", "-o", "x"]),
                &mut sink,
                &mut err
            ),
            3
        );
    }
}
