//! LDF 도메인 모델.
//!
//! 구문 트리(`ldf-syntax`)를 이름이 아닌 ID로 서로를 가리키는 타입 있는 모델로 바꾸고(`lower`),
//! 모든 크레이트가 공유하는 어휘(타입, 진단, PID·체크섬·프레임 시간)를 제공한다.
//! `ldf-check`, `ldf-writer`, `ldf-store`는 이 크레이트에만 의존하며 서로를 모른다.
//!
//! 설계서와의 차이: lowering이 구문 트리를 읽으므로 이 크레이트는 `ldf-syntax`에 의존한다
//! (`ldf-syntax`는 어떤 내부 크레이트에도 의존하지 않는다).

mod arena;
mod lower;
mod protocol;
mod semantic;
mod types;

pub use arena::{Arena, ArenaId, EncodingId, FrameId, NodeId, ScheduleId, SignalId};
pub use ldf_syntax::{Diagnostic, LineIndex, Severity, Span, codes};
pub use lower::{Loaded, load, lower};
pub use protocol::{
    classic_checksum, enhanced_checksum, frame_bits_nominal, frame_time_max_ms,
    frame_time_nominal_ms, pid_is_valid, protected_id,
};
pub use semantic::{semantic_diff, semantically_equal};
pub use types::*;

#[cfg(test)]
mod tests;
