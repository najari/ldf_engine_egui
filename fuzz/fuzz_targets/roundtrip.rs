//! 읽은 모델을 쓰고 다시 읽는다. 패닉하면 안 되고, 구문 오류가 없던 입력은 쓴 결과도 구문 오류가 없어야 한다.
#![no_main]

use libfuzzer_sys::fuzz_target;
use ldf_writer::{WriteOptions, write};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let first = ldf_model::load(&text);
    let out = write(&first.model, &WriteOptions::default()).text;
    let second = ldf_model::load(&out);
    let clean = |d: &ldf_model::Diagnostic| d.code >= 9000 && d.severity == ldf_model::Severity::Error;
    if !first.diagnostics.iter().any(clean) {
        assert!(!second.diagnostics.iter().any(clean), "쓴 결과가 읽히지 않는다:\n{out}");
    }
});
