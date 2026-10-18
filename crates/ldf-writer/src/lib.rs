//! 모델 -> LDF 텍스트(FR-EXP).
//!
//! 정규 서식으로 다시 쓰되, 최상위 문장 순서는 원문(`section_order`)을 따르고 모르는 섹션은 원문 그대로 둔다.
//! 출력을 다시 읽으면 의미가 같은 모델이 나온다(`ldf_model::semantically_equal`).
//! 대상 버전에서 허용되지 않는 항목은 빼고 경고로 알려준다(FR-EXP-01).
//!
//! 파일 머리말 주석은 되살리지만 그 밖의 주석과 원래 서식은 보존하지 않는다. 편집하지 않은 파일은 엔진이 원문을 그대로 내보낸다.

mod atomic;
mod emit;

pub use atomic::write_file_atomic;

use emit::{HEADER_KEYS, header_item};
use ldf_model::{LdfModel, SignalKind};

/// 출력 대상 버전.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Target {
    /// 원본 버전 그대로.
    #[default]
    Keep,
    /// LIN 2.0.
    V2_0,
    /// LIN 2.1.
    V2_1,
    /// LIN 2.2.
    V2_2,
}

impl Target {
    fn version(self) -> Option<&'static str> {
        match self {
            Target::Keep => None,
            Target::V2_0 => Some("2.0"),
            Target::V2_1 => Some("2.1"),
            Target::V2_2 => Some("2.2"),
        }
    }
}

/// 줄바꿈 형식.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Eol {
    /// `\n`.
    #[default]
    Lf,
    /// `\r\n`.
    CrLf,
}

/// 출력 옵션.
#[derive(Clone, Copy, Debug, Default)]
pub struct WriteOptions {
    /// 대상 버전.
    pub target: Target,
    /// 줄바꿈.
    pub eol: Eol,
}

/// 출력 결과.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    /// LDF 텍스트.
    pub text: String,
    /// 대상 버전으로 바꾸면서 빠진 항목 등의 경고.
    pub warnings: Vec<String>,
}

/// 대상 버전에서 허용되지 않는 항목을 빼고 경고를 모은다. 모델을 복사해 고친다.
fn adapt(model: &LdfModel, target: Target) -> (LdfModel, Vec<String>) {
    let mut m = model.clone();
    let mut warnings = Vec::new();
    let Some(version) = target.version() else {
        return (m, warnings);
    };

    if m.header.protocol_version.as_deref() != Some(version) {
        warnings.push(format!(
            "LIN_protocol_version을 {} 에서 {version} 으로 바꾼다",
            m.header.protocol_version.as_deref().unwrap_or("(없음)")
        ));
    }
    m.header.protocol_version = Some(version.to_owned());
    m.header.language_version = Some(version.to_owned());

    // ISO 17987 전용 헤더 항목
    if m.header.ldf_file_revision.take().is_some() {
        warnings.push(format!(
            "LDF_file_revision은 ISO 17987 전용이라 뺐다 (대상 {version})"
        ));
    }
    if std::mem::take(&mut m.header.sig_byte_order_big_endian) {
        warnings.push(format!(
            "LIN_sig_byte_order_big_endian은 ISO 17987 전용이라 뺐다 (대상 {version})"
        ));
    }

    if target == Target::V2_0 {
        if m.header.channel_name.take().is_some() {
            warnings.push("Channel_name은 LIN 2.1부터라 뺐다 (대상 2.0)".to_owned());
        }
        let mut dropped = 0;
        for a in &mut m.node_attrs {
            if a.initial_nad.take().is_some() {
                dropped += 1;
            }
        }
        if dropped > 0 {
            warnings.push(format!(
                "initial_NAD는 LIN 2.1부터라 노드 {dropped}개에서 뺐다 (대상 2.0)"
            ));
        }
    }

    let before = m.unknown_sections.len();
    let mut dropped_names = Vec::new();
    m.unknown_sections.retain(|s| {
        let drop = match s.name.as_str() {
            "Node_composition" => target == Target::V2_0,
            "dynamic_frames" | "Dynamic_frames" => matches!(target, Target::V2_0 | Target::V2_1),
            _ => false,
        };
        if drop {
            dropped_names.push(s.name.clone());
        }
        !drop
    });
    if m.unknown_sections.len() != before {
        warnings.push(format!(
            "대상 {version}에서 허용되지 않는 섹션을 뺐다: {}",
            dropped_names.join(", ")
        ));
    }
    (m, warnings)
}

const CANONICAL_SECTIONS: [&str; 13] = [
    "Nodes",
    "Node_composition",
    "Signals",
    "Diagnostic_signals",
    "Frames",
    "Sporadic_frames",
    "Event_triggered_frames",
    "Diagnostic_frames",
    "Node_attributes",
    "Schedule_tables",
    "Signal_groups",
    "Signal_encoding_types",
    "Signal_representation",
];

/// 섹션 이름에 해당하는 본문. 해당 모델이 비었고 원문에도 없던 섹션이면 `None`.
fn section_text(m: &LdfModel, name: &str, in_order: bool) -> Option<String> {
    let raw: Vec<&str> = m
        .unknown_sections
        .iter()
        .filter(|s| s.name == name)
        .map(|s| s.text.trim())
        .collect();
    if !raw.is_empty() {
        return Some(format!("{}\n", raw.join("\n")));
    }
    let has = |present: bool| present || in_order;
    let normal = |k| m.signals.iter().any(|(_, s)| s.kind == k);
    let any_frame = |f: fn(&ldf_model::FrameKind) -> bool| m.frames.iter().any(|(_, x)| f(&x.kind));
    match name {
        "Nodes" => has(!m.nodes.is_empty()).then(|| emit::nodes(m)).flatten(),
        "Signals" => has(normal(SignalKind::Normal))
            .then(|| emit::signals(m, SignalKind::Normal))
            .flatten(),
        "Diagnostic_signals" => has(normal(SignalKind::Diagnostic))
            .then(|| emit::signals(m, SignalKind::Diagnostic))
            .flatten(),
        "Frames" => has(any_frame(|k| {
            matches!(k, ldf_model::FrameKind::Unconditional { .. })
        }))
        .then(|| emit::frames(m, "Frames"))
        .flatten(),
        "Sporadic_frames" => has(any_frame(|k| {
            matches!(k, ldf_model::FrameKind::Sporadic { .. })
        }))
        .then(|| emit::frames(m, "Sporadic_frames"))
        .flatten(),
        "Event_triggered_frames" => has(any_frame(|k| {
            matches!(k, ldf_model::FrameKind::EventTriggered { .. })
        }))
        .then(|| emit::frames(m, "Event_triggered_frames"))
        .flatten(),
        "Diagnostic_frames" => has(any_frame(|k| {
            matches!(k, ldf_model::FrameKind::Diagnostic { .. })
        }))
        .then(|| emit::frames(m, "Diagnostic_frames"))
        .flatten(),
        "Node_attributes" => has(!m.node_attrs.is_empty())
            .then(|| emit::node_attributes(m))
            .flatten(),
        "Schedule_tables" => has(!m.schedules.is_empty())
            .then(|| emit::schedule_tables(m))
            .flatten(),
        "Signal_encoding_types" => has(!m.encodings.is_empty())
            .then(|| emit::encodings(m))
            .flatten(),
        "Signal_representation" => has(m.signals.iter().any(|(_, s)| !s.encodings.is_empty()))
            .then(|| emit::representation(m))
            .flatten(),
        _ => None,
    }
}

/// 모델을 LDF 텍스트로 만든다. 절대 패닉하지 않는다.
pub fn write(model: &LdfModel, opts: &WriteOptions) -> Written {
    let (m, warnings) = adapt(model, opts.target);

    // 순서: 원문 순서(중복 제거) 뒤에, 아직 안 나온 정규 섹션과 모르는 섹션
    let mut names: Vec<&str> = Vec::new();
    for n in m.section_order.iter().map(String::as_str) {
        if !names.contains(&n) {
            names.push(n);
        }
    }
    for n in HEADER_KEYS.iter().chain(CANONICAL_SECTIONS.iter()) {
        if !names.contains(n) {
            names.push(n);
        }
    }
    for s in &m.unknown_sections {
        if !names.contains(&s.name.as_str()) {
            names.push(s.name.as_str());
        }
    }

    let mut header = String::new();
    // 파일 머리말 주석을 맨 앞에 되살린다
    if !m.header.leading_comment.is_empty() {
        header.push_str(&m.header.leading_comment);
        header.push_str("\n\n");
    }
    let mut sections: Vec<String> = Vec::new();
    for name in names {
        let in_order = m.section_order.iter().any(|n| n == name);
        if HEADER_KEYS.contains(&name) {
            if let Some(t) = header_item(&m, name) {
                header.push_str(&t);
            }
        } else if let Some(t) = section_text(&m, name, in_order) {
            sections.push(t);
        }
    }

    let mut text = header;
    for s in sections {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&s);
    }
    if opts.eol == Eol::CrLf {
        text = text.replace('\n', "\r\n");
    }
    Written { text, warnings }
}

#[cfg(test)]
mod tests;
