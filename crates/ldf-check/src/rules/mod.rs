//! 규칙 모음. 규칙 하나 = 파일 하나(`rNNNN_이름.rs`), 여기에 등록한다.

use crate::Rule;
use crate::util::{Proto, attr_proto, is_master};
use ldf_model::{Diagnostic, LdfModel, Severity};

mod r0010_version;
mod r0020_structure;
mod r0021_ignored;
mod r0030_slave_protocol;
mod r0101_sections;
mod r0102_required_section;
mod r0103_node_attr_version;
mod r0104_missing_attr;
mod r0105_etf_format;
mod r0106_commands;
mod r0107_byte_init;
mod r0108_node_attribute_set;
mod r0109_composition;
mod r0201_schedule;
mod r0202_node_attributes;
mod r0203_frames;
mod r0204_encoding_target;
mod r0205_encoding_values;
mod r0206_multiple_encodings;
mod r0301_speed;
mod r0302_node_parameters;
mod r0303_frame_parameters;
mod r0304_signal_parameters;
mod r0305_command_parameters;
mod r0306_logical_values;
mod r0401_unmapped_signal;
mod r0402_publisher_mapping;
mod r0403_configurable_frames;
mod r0404_fault_state;
mod r0405_publisher_subscriber;
mod r0406_frame_layout;

/// 등록된 규칙 목록. 새 규칙은 모듈을 추가하고 아래 벡터에 넣는다.
pub(crate) fn registered() -> Vec<&'static dyn Rule> {
    vec![
        &r0010_version::RULE,
        &r0020_structure::RULE,
        &r0021_ignored::RULE,
        &r0030_slave_protocol::RULE,
        &r0101_sections::RULE,
        &r0102_required_section::RULE,
        &r0103_node_attr_version::RULE,
        &r0104_missing_attr::RULE,
        &r0105_etf_format::RULE,
        &r0106_commands::RULE,
        &r0403_configurable_frames::RULE,
        &r0405_publisher_subscriber::RULE,
        &r0404_fault_state::RULE,
        &r0402_publisher_mapping::RULE,
        &r0401_unmapped_signal::RULE,
        &r0306_logical_values::RULE,
        &r0305_command_parameters::RULE,
        &r0304_signal_parameters::RULE,
        &r0303_frame_parameters::RULE,
        &r0302_node_parameters::RULE,
        &r0301_speed::RULE,
        &r0206_multiple_encodings::RULE,
        &r0205_encoding_values::RULE,
        &r0204_encoding_target::RULE,
        &r0203_frames::RULE,
        &r0202_node_attributes::RULE,
        &r0201_schedule::RULE,
        &r0109_composition::RULE,
        &r0108_node_attribute_set::RULE,
        &r0107_byte_init::RULE,
        &r0406_frame_layout::RULE,
    ]
}

/// 노드 속성에 형식 오류(0103)가 있으면 Vector는 속성 블록을 읽지 못한 것처럼 취급해, 뒤따르는 검사(0104·0108·0302·0402·0404)도
/// 한 번씩 결과를 낸다. 마스터에 둔 속성이나 LIN 1.3 슬레이브의 2.x 속성이면 0202도 함께 난다.
/// 형식 오류가 없으면 `None`, 있으면 그 두 번째 조건을 돌려준다.
pub(crate) fn attribute_format_cascade(m: &LdfModel) -> Option<bool> {
    let mut hits = Vec::new();
    r0103_node_attr_version::RULE.check(m, &mut hits);
    if hits.is_empty() {
        return None;
    }
    Some(
        m.node_attrs.iter().any(|a| {
            is_master(m, &a.node) || attr_proto(m, a.lin_protocol.as_deref()) == Proto::V1_3
        }),
    )
}

/// 위 연쇄 결과로 낼 진단 하나.
pub(crate) fn cascade_diag(code: u16, severity: Severity) -> Diagnostic {
    crate::util::diag(
        severity,
        code,
        None,
        "노드 속성 형식 오류(0103)로 노드 속성을 읽지 못해 이 검사를 끝까지 하지 못했다",
    )
}
