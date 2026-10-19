//! `ldf-cli ncf-import`: NCF의 노드 하나를 LDF의 슬레이브로 가져온다(프레임·시그널·인코딩·노드 속성).
//!
//! `-o`가 없으면 무엇이 생기는지만 보여 주고(미리 보기) 파일은 쓰지 않는다.

use crate::args::Args;
use ldf_engine::{ExportOptions, Session, plan_ncf_import, read_ncf};
use ldf_i18n::{t, tr, twriteln};
use std::io::Write;
use std::path::Path;

/// 명령을 실행하고 종료 코드를 돌려준다. 0 = 성공(또는 미리 보기), 2 = 충돌·오류 진단으로 쓰지 못함, 3 = 실행 실패.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match Args::parse(args, &["-o", "--node"], &["--allow-errors"]) {
        Ok(a) => a,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return 3;
        }
    };
    let [ldf_path, ncf_path] = parsed.positional.as_slice() else {
        let _ = twriteln!(err, "ncf-import는 LDF 파일과 NCF 파일이 필요하다");
        return 3;
    };
    let read = |path: &str, err: &mut dyn Write| match std::fs::read(Path::new(path)) {
        Ok(b) => Some(b),
        Err(e) => {
            let _ = twriteln!(err, "{path}: 파일을 읽을 수 없다: {e}");
            None
        }
    };
    let (Some(ldf_bytes), Some(ncf_bytes)) = (read(ldf_path, err), read(ncf_path, err)) else {
        return 3;
    };

    // 가져올 노드: `--node`로 고르거나 하나뿐이면 그것
    let doc = read_ncf(&ncf_bytes);
    let node = match (parsed.value("--node"), doc.file.nodes.as_slice()) {
        (Some(name), nodes) => nodes.iter().find(|n| n.name == name),
        (None, [only]) => Some(only),
        (None, []) => None,
        (None, _) => {
            let _ = twriteln!(err, "NCF에 노드가 여럿이다: --node 이름으로 고른다");
            return 3;
        }
    };
    let Some(node) = node else {
        let _ = twriteln!(err, "{ncf_path}: 가져올 노드를 찾지 못했다");
        for d in &doc.diagnostics {
            let _ = writeln!(err, "  [{:04}] {}", d.code, tr(&d.message));
        }
        return 3;
    };

    let mut session = match Session::open_in_memory() {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return 3;
        }
    };
    let snap = match session.import_bytes(ldf_path, &ldf_bytes) {
        Ok(o) => o.snapshot,
        Err(e) => {
            let _ = twriteln!(err, "{ldf_path}: {e}");
            return 3;
        }
    };

    let plan = match plan_ncf_import(&snap.model, node) {
        Ok(p) => p,
        Err(errs) => {
            let _ = twriteln!(err, "노드 `{}`을(를) 가져올 수 없다:", node.name);
            for e in errs {
                let _ = writeln!(err, "  {}", tr(&e));
            }
            return 2;
        }
    };
    let _ = twriteln!(
        out,
        "노드 `{}`: 프레임 {}개, 시그널 {}개, 인코딩 {}개를 더한다",
        node.name,
        plan.frames,
        plan.signals,
        plan.encodings
    );
    for n in &plan.notes {
        let _ = writeln!(out, "  - {}", tr(n));
    }

    let Some(output) = parsed.value("-o") else {
        let _ = writeln!(out, "{}", t("(-o가 없어 파일은 쓰지 않았다)"));
        return 0;
    };
    let edited = match session.edit(&snap, &plan.op) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(err, "{}", tr(&e.to_string()));
            return 2;
        }
    };
    let opts = ExportOptions {
        allow_errors: parsed.flag("--allow-errors"),
        ..ExportOptions::default()
    };
    match session.export_snapshot(&edited, Path::new(output), &opts) {
        Ok(r) => {
            let _ = twriteln!(out, "{output} ({} 바이트)", r.bytes);
            for w in &r.warnings {
                let _ = writeln!(err, "{}", tr(w));
            }
            0
        }
        Err(e) => {
            // 가져온 결과에 오류 진단이 남았으면 쓰지 않는다(`--allow-errors`로 쓸 수 있다)
            let _ = writeln!(err, "{}", tr(&e.to_string()));
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str, content: &str) -> String {
        let dir = std::env::temp_dir().join(format!("ldfcli-ncfimp-{}", std::process::id()));
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

    const LDF: &str = "LIN_description_file;\nLIN_protocol_version = \"2.1\";\nLIN_language_version = \"2.1\";\nLIN_speed = 19.2 kbps;\n\
Nodes { Master: M, 5 ms, 0.1 ms; Slaves: Old; }\n\
Signals { OldSig: 8, 0, Old, M; }\n\
Frames { OldFrame: 0, Old, 1 { OldSig, 0; } }\n\
Schedule_tables { T { OldFrame delay 10 ms; } }\n";

    const NCF: &str = "node_capability_file;\nnode Motor {\n\
 general { LIN_protocol_version = \"2.1\"; supplier = 0x1e; function = 1; variant = 0; }\n\
 diagnostic { NAD = 5; P2_min = 50 ms; ST_min = 0 ms; }\n\
 frames { subscribe Control { length = 2; signals { Speed { init_value = 0; size = 16; offset = 0; } } } }\n}\n";

    #[test]
    fn without_o_it_only_previews() {
        let (code, out, _) = go(&[&tmp("a.ldf", LDF), &tmp("a.ncf", NCF)]);
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("노드 `Motor`: 프레임 1개, 시그널 1개, 인코딩 0개를 더한다"),
            "{out}"
        );
        assert!(out.contains("스케줄 테이블에는 넣지 않았다"), "{out}");
        assert!(out.contains("파일은 쓰지 않았다"), "{out}");
    }

    #[test]
    fn it_writes_the_merged_ldf_that_reads_back_with_the_new_node() {
        let output = tmp("merged-out.ldf", "");
        let _ = std::fs::remove_file(&output);
        let (code, out, err) = go(&[
            &tmp("b.ldf", LDF),
            &tmp("b.ncf", NCF),
            "-o",
            &output,
            "--allow-errors",
        ]);
        assert_eq!(code, 0, "{out}\n{err}");
        let text = std::fs::read_to_string(&output).expect("read");
        assert!(text.contains("Slaves: Old, Motor;"), "{text}");
        assert!(text.contains("Control: "), "{text}");
        let m = ldf_model::load(&text).model;
        assert!(m.find_node("Motor").is_some());
        assert!(m.find_frame("Control").is_some());
    }

    #[test]
    fn conflicts_exit_with_two_and_write_nothing() {
        let output = tmp("conflict-out.ldf", "");
        let _ = std::fs::remove_file(&output);
        let clash = NCF.replace("node Motor", "node Old");
        let (code, _, err) = go(&[&tmp("c.ldf", LDF), &tmp("c.ncf", &clash), "-o", &output]);
        assert_eq!(code, 2);
        assert!(err.contains("`Old`"), "{err}");
        assert!(!Path::new(&output).exists());
    }

    #[test]
    fn node_choice_and_bad_arguments() {
        let two = format!("{NCF}node Other {{ }}\n");
        let (code, _, err) = go(&[&tmp("d.ldf", LDF), &tmp("d.ncf", &two)]);
        assert_eq!(code, 3);
        assert!(err.contains("--node"), "{err}");
        let (code, out, _) = go(&[&tmp("d.ldf", LDF), &tmp("d.ncf", &two), "--node", "Other"]);
        assert_eq!(code, 0, "{out}");
        assert_eq!(go(&[&tmp("d.ldf", LDF)]).0, 3);
        assert_eq!(go(&[&tmp("d.ldf", LDF), "Z:/none.ncf"]).0, 3);
        let (code, _, _) = go(&[&tmp("d.ldf", LDF), &tmp("d.ncf", &two), "--node", "Nope"]);
        assert_eq!(code, 3);
    }
}
