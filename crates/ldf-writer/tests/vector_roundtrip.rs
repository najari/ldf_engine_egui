//! 로컬 Vector `TestFiles`를 읽어 쓰고 다시 읽어도 의미가 같은지 확인한다(FR-EXP-02).
//!
//! `LDF_VECTOR_TESTFILES`가 없으면 건너뛴다. 상용 제품 동봉물이므로 저장소에 넣지 않는다(NFR-06).
//! 이름·섹션이 중복되었거나 구문 자체가 틀려 모델이 값을 보정해야 하는 파일은 왕복이 보장되지 않으므로 제외한다.

use ldf_model::{load, semantic_diff};
use ldf_writer::{WriteOptions, write};
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
fn every_vector_test_file_survives_write_and_reread() {
    let Ok(dir) = std::env::var("LDF_VECTOR_TESTFILES") else {
        eprintln!("LDF_VECTOR_TESTFILES가 없어 건너뜀");
        return;
    };
    let mut files = Vec::new();
    collect(Path::new(&dir), &mut files);
    files.sort();
    assert!(!files.is_empty());

    let (mut checked, mut skipped) = (0, 0);
    let mut failures = Vec::new();
    for f in &files {
        let bytes = std::fs::read(f).expect("read");
        let text = String::from_utf8_lossy(&bytes);
        let first = load(&text);
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if first
            .diagnostics
            .iter()
            .any(|d| d.severity == ldf_model::Severity::Error && d.code >= 9000)
        {
            skipped += 1;
            continue;
        }
        checked += 1;
        let out = write(&first.model, &WriteOptions::default()).text;
        let second = load(&out);
        if let Some(d) = semantic_diff(&first.model, &second.model) {
            failures.push(format!("{name}: {d}"));
            continue;
        }
        // 고정점: 한 번 쓴 출력을 다시 쓰면 같은 글자가 나온다
        let again = write(&second.model, &WriteOptions::default()).text;
        if again != out {
            failures.push(format!("{name}: 두 번째 출력이 첫 출력과 다르다"));
        }
    }
    eprintln!(
        "파일 {checked}개 검사, {skipped}개 제외(구문 오류·중복), 실패 {}개",
        failures.len()
    );
    assert!(failures.is_empty(), "{failures:#?}");
}

/// 다시 쓴 텍스트에 주석을 옮기면 원문의 주석이 얼마나 남는지(FR-EXP-03). 파일 머리말 주석은 따로 되살리므로 합쳐서 센다.
#[test]
fn carrying_comments_keeps_nearly_all_comments_of_the_corpus() {
    let Ok(dir) = std::env::var("LDF_VECTOR_TESTFILES") else {
        eprintln!("LDF_VECTOR_TESTFILES가 없어 건너뜀");
        return;
    };
    let mut files = Vec::new();
    collect(Path::new(&dir), &mut files);
    files.sort();
    let (mut total, mut kept) = (0usize, 0usize);
    let mut lost_files = Vec::new();
    for f in &files {
        let bytes = std::fs::read(f).expect("read");
        let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
        let loaded = load(&text);
        if loaded
            .diagnostics
            .iter()
            .any(|d| d.severity == ldf_model::Severity::Error && d.code >= 9000)
        {
            continue;
        }
        let new = write(&loaded.model, &WriteOptions::default()).text;
        let out = ldf_writer::carry_comments(&text, &new);
        let count = |s: &str| ldf_syntax::lex(s).comments.len();
        let (before, after) = (count(&text), count(&out));
        total += before;
        kept += after.min(before);
        if after < before {
            lost_files.push(format!(
                "{}: {before} -> {after}",
                f.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ));
        }
        // 어떤 경우에도 결과는 같은 모델로 읽혀야 한다
        let again = load(&out);
        assert!(
            ldf_model::semantically_equal(&loaded.model, &again.model),
            "{}: 주석을 옮긴 뒤 모델이 달라졌다",
            f.display()
        );
    }
    eprintln!(
        "주석 {total}개 중 {kept}개 유지, 줄어든 파일 {}개 {lost_files:?}",
        lost_files.len()
    );
    assert!(total > 0);
    assert!(
        kept * 100 >= total * 95,
        "주석 유지율이 95% 아래다: {kept}/{total}"
    );
}
