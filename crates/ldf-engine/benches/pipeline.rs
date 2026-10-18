//! NFR-01 성능 벤치마크: 임포트+전체 검사(100 KB, 1 MB), 편집→재검사, 쓰기, 비교.
//! 실행: `cargo bench -p ldf-engine` (결과는 `target/criterion`, 요약은 `docs/performance.md`).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use criterion::{Criterion, criterion_group, criterion_main};
use ldf_engine::{EditOp, Session, analyze};
use ldf_writer::{WriteOptions, write};
use std::hint::black_box;

/// 합성 LDF: 슬레이브 20개, 시그널 `frames*4`개, 프레임 `frames`개, 스케줄 하나.
fn synthetic(frames: usize) -> String {
    let slaves: Vec<String> = (0..20).map(|i| format!("S{i}")).collect();
    let mut s = String::from(
        "LIN_description_file;\nLIN_protocol_version = \"2.1\";\nLIN_language_version = \"2.1\";\nLIN_speed = 19.2 kbps;\n",
    );
    s.push_str(&format!(
        "Nodes {{ Master: M, 1 ms, 0 ms; Slaves: {}; }}\nSignals {{\n",
        slaves.join(", ")
    ));
    for i in 0..frames * 4 {
        s.push_str(&format!("  Sig{i}: 8, 0, {}, M;\n", slaves[i % 20]));
    }
    s.push_str("}\nFrames {\n");
    for f in 0..frames {
        s.push_str(&format!(
            "  Fr{f}: {}, {}, 4 {{ ",
            f % 60,
            slaves[(f * 4) % 20]
        ));
        for k in 0..4 {
            s.push_str(&format!("Sig{}, {}; ", f * 4 + k, k * 8));
        }
        s.push_str("}\n");
    }
    s.push_str("}\nSchedule_tables { T { ");
    for f in 0..frames.min(500) {
        s.push_str(&format!("Fr{f} delay 10 ms; "));
    }
    s.push_str("} }\n");
    s
}

fn benches(c: &mut Criterion) {
    // 약 100 KB (프레임 600개) 와 약 1 MB (프레임 6,000개)
    let small = synthetic(600);
    let large = synthetic(6_000);
    eprintln!(
        "합성 LDF: {} KB, {} KB",
        small.len() / 1024,
        large.len() / 1024
    );

    c.bench_function("analyze_100kb", |b| {
        b.iter(|| black_box(analyze(small.as_bytes())))
    });
    let mut g = c.benchmark_group("large");
    g.sample_size(10);
    g.bench_function("analyze_1mb", |b| {
        b.iter(|| black_box(analyze(large.as_bytes())))
    });
    g.finish();

    let model = analyze(small.as_bytes()).model;
    c.bench_function("write_100kb", |b| {
        b.iter(|| black_box(write(&model, &WriteOptions::default())))
    });

    c.bench_function("import_100kb_in_memory", |b| {
        b.iter_batched(
            || Session::open_in_memory().expect("session"),
            |mut s| black_box(s.import_bytes("a.ldf", small.as_bytes()).expect("import")),
            criterion::BatchSize::LargeInput,
        )
    });

    let mut s = Session::open_in_memory().expect("session");
    let snap = s
        .import_bytes("a.ldf", small.as_bytes())
        .expect("import")
        .snapshot;
    c.bench_function("edit_recheck_100kb", |b| {
        b.iter(|| black_box(s.edit(&snap, &EditOp::SetSpeed(10_400)).expect("edit")))
    });

    let other = {
        let mut m = model.clone();
        EditOp::SetSpeed(10_400).apply(&mut m).expect("apply");
        m
    };
    c.bench_function("diff_100kb", |b| {
        b.iter(|| black_box(ldf_model::diff(&model, &other)))
    });
}

criterion_group!(pipeline, benches);
criterion_main!(pipeline);
