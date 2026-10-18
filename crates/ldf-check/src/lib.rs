//! LDF 검사 규칙(PRD FR-CHK). Vector Consistency Checker와 같은 자리수의 규칙 코드를 쓴다.
//!
//! 코드 대역: 00xx Basic · 01xx LDF Format · 02xx Modelling · 03xx Parameter · 04xx Mapping.
//! 규칙 하나는 한 파일(`rules/rNNNN_*.rs`)이며, 결과는 `ldf_model::Diagnostic`으로 돌려준다.
//! 진단의 `code`가 곧 규칙 번호이고, 한 규칙이 여러 결과(진단)를 낼 수 있다.

mod category;
mod raw;
mod registry;
pub mod rules;
mod util;

pub use category::Category;
pub use registry::{Rule, all_rules, run_all, run_rules};
