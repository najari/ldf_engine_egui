//! 임의의 바이트를 분석(디코딩 → 파싱 → lowering → 검사)한다. 어떤 입력에도 패닉하면 안 된다.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = ldf_engine::analyze(data);
});
