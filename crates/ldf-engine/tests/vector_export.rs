//! 편집하지 않은 리비전을 내보내면 원본 바이트 그대로인지 Vector `TestFiles` 전체로 확인한다(FR-EXP-03).
//!
//! `LDF_VECTOR_TESTFILES`가 없으면 건너뛴다. 상용 제품 동봉물이므로 저장소에 넣지 않는다(NFR-06).

#![allow(clippy::expect_used, clippy::panic)]

use ldf_engine::{ExportOptions, Session};
use std::path::{Path, PathBuf};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ldf")) {
            out.push(p);
        }
    }
}

#[test]
fn unedited_export_is_byte_identical_for_every_vector_test_file() {
    let Ok(dir) = std::env::var("LDF_VECTOR_TESTFILES") else {
        eprintln!("LDF_VECTOR_TESTFILES가 없어 건너뜀");
        return;
    };
    let mut files = Vec::new();
    collect(Path::new(&dir), &mut files);
    files.sort();
    assert!(!files.is_empty());

    let tmp = std::env::temp_dir().join(format!("ldf-verbatim-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).expect("tmp");
    let opts = ExportOptions {
        allow_errors: true,
        ..ExportOptions::default()
    };
    let mut s = Session::open_in_memory().expect("session");
    let mut different = Vec::new();
    for f in &files {
        let bytes = std::fs::read(f).expect("read");
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let snap = s.import_bytes(&name, &bytes).expect("import").snapshot;
        let out = tmp.join(&name);
        s.export_snapshot(&snap, &out, &opts).expect("export");
        if std::fs::read(&out).expect("read back") != bytes {
            different.push(name);
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    eprintln!(
        "{}개 파일 중 원본과 다른 것 {}개",
        files.len(),
        different.len()
    );
    assert!(different.is_empty(), "{different:?}");
}
