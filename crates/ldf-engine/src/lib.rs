//! 엔진 파사드. UI와 CLI는 이 크레이트만 호출한다.
//!
//! - [`Session`]: 프로젝트 DB 하나에 대한 동기 작업(임포트, 리비전 열기, 인코딩 판별).
//! - [`Engine`]: `Session`을 워커 스레드에서 돌리고 [`Command`]/[`Event`] 채널로 말한다.
//!   UI 프레임 안에서는 SQL도 파일 I/O도 하지 않고, 워커가 보낸 [`ModelSnapshot`]만 읽는다.

mod analysis;
pub mod decode;
pub mod diff_report;
mod error;
mod export;
mod ncf;
mod ncf_import;
mod session;
mod worker;

pub use analysis::{Analysis, analyze, analyze_text, analyze_text_with, analyze_with};
pub use error::{EngineError, Result};
pub use export::{ExportFormat, ExportOptions, ExportReport};
pub use ncf::{NcfDocument, is_ncf_path, read_ncf};
pub use ncf_import::{NcfImport, plan_ncf_import};
pub use session::{ImportOutcome, ModelSnapshot, Session};
pub use worker::{Command, Engine, Event};

pub use ldf_check::{RuleInfo, RuleSetting, RuleSettings, rule_infos};
pub use ldf_model::{
    AttrsDraft, EditError, EditOp, FrameDraft, FrameDraftKind, SignalDraft, SlotDraft,
};
/// UI가 리비전을 표시하는 데 쓰는 요약 타입(저장소 크레이트를 직접 알 필요가 없도록 다시 내보낸다).
pub use ldf_store::RevisionInfo;
pub use ldf_writer::{Eol, Target};
