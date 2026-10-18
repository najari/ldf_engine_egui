//! 모델을 JSON으로 내보낸다(FR-EXP-06, 다른 도구 연동용).
//!
//! 내부 모델은 ID로 서로를 가리키지만, 내보내는 JSON은 **이름**으로 가리켜 읽기 쉽고 형식이 안정적이다.
//! 스키마는 `docs/model.schema.json`이며 `format`/`version`으로 구분한다.

use crate::{EncodingEntry, FrameKind, InitValue, LdfModel, NodeRole, Ref, SignalKind, SlotEntry};
use serde_json::{Value, json};

/// 내보내는 JSON 형식의 버전. 호환되지 않게 바꿀 때만 올린다.
pub const JSON_VERSION: u32 = 1;

fn name<T: crate::arena::ArenaId>(r: &Ref<T>, lookup: impl Fn(T) -> Option<String>) -> String {
    match r {
        Ref::Resolved(id) => lookup(*id).unwrap_or_default(),
        Ref::Unresolved(n) => n.clone(),
    }
}

fn opt(v: Option<f64>) -> Value {
    v.map_or(Value::Null, |v| json!(v))
}

/// 모델을 JSON 값으로 바꾼다.
pub fn to_json(m: &LdfModel) -> Value {
    let node = |r: &Ref<crate::NodeId>| name(r, |id| m.nodes.get(id).map(|n| n.name.clone()));
    let signal = |r: &Ref<crate::SignalId>| name(r, |id| m.signals.get(id).map(|n| n.name.clone()));
    let frame = |r: &Ref<crate::FrameId>| name(r, |id| m.frames.get(id).map(|n| n.name.clone()));
    let encoding =
        |r: &Ref<crate::EncodingId>| name(r, |id| m.encodings.get(id).map(|n| n.name.clone()));
    let table =
        |r: &Ref<crate::ScheduleId>| name(r, |id| m.schedules.get(id).map(|n| n.name.clone()));

    let nodes: Vec<Value> = m
        .nodes
        .iter()
        .map(|(_, n)| {
            json!({
                "name": n.name,
                "role": if n.role == NodeRole::Master { "master" } else { "slave" },
                "timebase_ms": opt(n.timebase_ms),
                "jitter_ms": opt(n.jitter_ms),
                "max_header_bits": n.max_header_bits,
                "response_tolerance_pct": opt(n.response_tolerance_pct),
            })
        })
        .collect();

    let signals: Vec<Value> = m
        .signals
        .iter()
        .map(|(_, s)| {
            json!({
                "name": s.name,
                "kind": if s.kind == SignalKind::Normal { "normal" } else { "diagnostic" },
                "size_bits": s.size_bits,
                "init": match &s.init {
                    InitValue::Scalar(v) => json!({ "scalar": v }),
                    InitValue::Array(b) => json!({ "bytes": b }),
                },
                "publisher": s.publisher.as_ref().map(&node),
                "subscribers": s.subscribers.iter().map(&node).collect::<Vec<_>>(),
                "encodings": s.encodings.iter().map(&encoding).collect::<Vec<_>>(),
            })
        })
        .collect();

    let placed = |signals: &[crate::FrameSignal]| -> Vec<Value> {
        signals
            .iter()
            .map(|fs| json!({ "signal": signal(&fs.signal), "offset": fs.offset }))
            .collect()
    };
    let frames: Vec<Value> = m
        .frames
        .iter()
        .map(|(_, f)| match &f.kind {
            FrameKind::Unconditional {
                id,
                publisher,
                size,
                signals,
            } => json!({
                "name": f.name, "kind": "unconditional", "id": id,
                "publisher": node(publisher), "size": size, "signals": placed(signals),
            }),
            FrameKind::Sporadic { associated } => json!({
                "name": f.name, "kind": "sporadic",
                "associated": associated.iter().map(&frame).collect::<Vec<_>>(),
            }),
            FrameKind::EventTriggered {
                id,
                collision_table,
                associated,
            } => json!({
                "name": f.name, "kind": "event_triggered", "id": id,
                "collision_table": collision_table.as_ref().map(&table),
                "associated": associated.iter().map(&frame).collect::<Vec<_>>(),
            }),
            FrameKind::Diagnostic { id, signals } => json!({
                "name": f.name, "kind": "diagnostic", "id": id, "signals": placed(signals),
            }),
        })
        .collect();

    let encodings: Vec<Value> = m
        .encodings
        .iter()
        .map(|(_, e)| {
            let entries: Vec<Value> = e
                .entries
                .iter()
                .map(|x| match x {
                    EncodingEntry::Logical { value, text } => {
                        json!({ "type": "logical", "value": value, "text": text })
                    }
                    EncodingEntry::Physical {
                        min,
                        max,
                        scale,
                        offset,
                        unit,
                    } => json!({
                        "type": "physical", "min": min, "max": max,
                        "scale": scale, "offset": offset, "unit": unit,
                    }),
                    EncodingEntry::Bcd => json!({ "type": "bcd" }),
                    EncodingEntry::Ascii => json!({ "type": "ascii" }),
                })
                .collect();
            json!({ "name": e.name, "entries": entries })
        })
        .collect();

    let schedules: Vec<Value> = m
        .schedules
        .iter()
        .map(|(_, t)| {
            let slots: Vec<Value> = t
                .slots
                .iter()
                .map(|s| {
                    let mut v = match &s.entry {
                        SlotEntry::Frame(r) => json!({ "type": "frame", "frame": frame(r) }),
                        SlotEntry::AssignNad { node: n } => {
                            json!({ "type": "assign_nad", "node": node(n) })
                        }
                        SlotEntry::ConditionalChangeNad { args } => {
                            json!({ "type": "conditional_change_nad", "args": args })
                        }
                        SlotEntry::DataDump { node: n, data } => {
                            json!({ "type": "data_dump", "node": node(n), "data": data })
                        }
                        SlotEntry::SaveConfiguration { node: n } => {
                            json!({ "type": "save_configuration", "node": node(n) })
                        }
                        SlotEntry::AssignFrameIdRange {
                            node: n,
                            start_index,
                            pids,
                        } => json!({
                            "type": "assign_frame_id_range", "node": node(n),
                            "start_index": start_index, "pids": pids,
                        }),
                        SlotEntry::AssignFrameId { node: n, frame: f } => {
                            json!({ "type": "assign_frame_id", "node": node(n), "frame": frame(f) })
                        }
                        SlotEntry::FreeFormat { data } => {
                            json!({ "type": "free_format", "data": data })
                        }
                        SlotEntry::Unknown(raw) => json!({ "type": "unknown", "text": raw }),
                    };
                    v["delay_ms"] = json!(s.delay_ms);
                    v
                })
                .collect();
            json!({ "name": t.name, "slots": slots })
        })
        .collect();

    let node_attributes: Vec<Value> = m
        .node_attrs
        .iter()
        .map(|a| {
            json!({
                "node": node(&a.node),
                "lin_protocol": a.lin_protocol,
                "configured_nad": a.configured_nad,
                "initial_nad": a.initial_nad,
                "product_id": a.product_id.as_ref().map(|p| json!({
                    "supplier": p.supplier, "function": p.function, "variant": p.variant,
                })),
                "response_error": a.response_error.as_ref().map(&signal),
                "fault_state_signals": a.fault_state_signals.iter().map(&signal).collect::<Vec<_>>(),
                "p2_min_ms": opt(a.p2_min_ms),
                "st_min_ms": opt(a.st_min_ms),
                "n_as_timeout_ms": opt(a.n_as_timeout_ms),
                "n_cr_timeout_ms": opt(a.n_cr_timeout_ms),
                "configurable_frames": a.configurable_frames.iter().map(|c| json!({
                    "frame": frame(&c.frame), "message_id": c.message_id,
                })).collect::<Vec<_>>(),
                "extra": a.extra.iter().map(|e| json!({ "key": e.key, "text": e.text })).collect::<Vec<_>>(),
            })
        })
        .collect();

    json!({
        "format": "ldf-model",
        "version": JSON_VERSION,
        "header": {
            "protocol_version": m.header.protocol_version,
            "language_version": m.header.language_version,
            "speed_bps": m.header.speed_bps,
            "channel_name": m.header.channel_name,
            "ldf_file_revision": m.header.ldf_file_revision,
            "sig_byte_order_big_endian": m.header.sig_byte_order_big_endian,
            "encoding": m.header.encoding,
            "leading_comment": m.header.leading_comment,
        },
        "nodes": nodes,
        "signals": signals,
        "frames": frames,
        "encodings": encodings,
        "schedules": schedules,
        "node_attributes": node_attributes,
        "unknown_sections": m.unknown_sections.iter()
            .map(|s| json!({ "name": s.name, "text": s.text })).collect::<Vec<_>>(),
    })
}

/// `to_json`을 보기 좋게 들여 쓴 문자열로.
pub fn to_json_string(m: &LdfModel) -> String {
    // Value 직렬화는 실패하지 않는다. 그래도 패닉하지 않도록 빈 문서로 대체한다.
    serde_json::to_string_pretty(&to_json(m)).unwrap_or_else(|_| "{}".to_owned())
}

#[cfg(test)]
mod tests;
