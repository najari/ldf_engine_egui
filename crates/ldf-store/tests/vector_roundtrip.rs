//! 로컬 Vector `TestFiles` 전체를 DB에 저장했다가 복원해도 모델이 같은지 확인한다(FR-IMP-05).
//!
//! `LDF_VECTOR_TESTFILES`가 없으면 건너뛴다. 상용 제품 동봉물이므로 저장소에 넣지 않는다(NFR-06).

use ldf_store::{NewRevision, Store};
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
fn every_vector_test_file_survives_a_database_round_trip() {
    let Ok(dir) = std::env::var("LDF_VECTOR_TESTFILES") else {
        eprintln!("LDF_VECTOR_TESTFILES가 없어 건너뜀");
        return;
    };
    let mut files = Vec::new();
    collect(Path::new(&dir), &mut files);
    files.sort();
    assert!(!files.is_empty());

    let mut store = Store::open_in_memory().expect("open");
    let project = store.ensure_project("corpus").expect("project");
    let mut failures = Vec::new();

    for f in &files {
        let bytes = std::fs::read(f).expect("read");
        let text = String::from_utf8_lossy(&bytes);
        let loaded = ldf_model::load(&text);
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let saved = store.save_revision(&NewRevision {
            project_id: project,
            parent_id: None,
            label: &name,
            source_name: Some(&name),
            source: &bytes,
            model: &loaded.model,
            diagnostics: &loaded.diagnostics,
        });
        let id = match saved {
            Ok(id) => id,
            Err(e) => {
                failures.push(format!("{name}: 저장 실패 {e}"));
                continue;
            }
        };
        match store.load_revision(id) {
            Ok(back) => {
                if back.model != loaded.model {
                    failures.push(format!("{name}: 복원한 모델이 다르다"));
                }
                if back.diagnostics != loaded.diagnostics {
                    failures.push(format!("{name}: 복원한 진단이 다르다"));
                }
                if back.source != bytes {
                    failures.push(format!("{name}: 원본 바이트가 다르다"));
                }
            }
            Err(e) => failures.push(format!("{name}: 복원 실패 {e}")),
        }
    }

    store.check_integrity().expect("integrity");
    eprintln!(
        "파일 {}개 왕복 검사, 실패 {}개",
        files.len(),
        failures.len()
    );
    assert!(failures.is_empty(), "{failures:#?}");
}
