//! DB -> 모델.

use crate::error::corrupt;
use crate::{Result, RevisionInfo, StoreError, StoredRevision};
use ldf_model::{
    ArenaId, ConfigurableFrame, Diagnostic, EncodingEntry, EncodingId, EncodingType, Frame,
    FrameId, FrameKind, FrameSignal, InitValue, LdfModel, Node, NodeAttrs, NodeId, NodeRole,
    ProductId, RawAttr, RawSection, Ref, ScheduleId, ScheduleTable, Severity, Signal, SignalId,
    SignalKind, Slot, SlotEntry, Span,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

fn span_from(s0: Option<i64>, s1: Option<i64>) -> Option<Span> {
    match (s0, s1) {
        (Some(a), Some(b)) => Some(Span::new(
            usize::try_from(a).ok()?,
            usize::try_from(b).ok()?,
        )),
        _ => None,
    }
}

fn u32_of(v: i64, what: &str) -> Result<u32> {
    u32::try_from(v).map_err(|_| corrupt(format!("{what}: 범위를 벗어난 값 {v}")))
}

fn opt_u32(v: Option<i64>, what: &str) -> Result<Option<u32>> {
    v.map(|v| u32_of(v, what)).transpose()
}

fn make_ref<T: ArenaId>(idx: Option<i64>, name: Option<String>) -> Result<Option<Ref<T>>> {
    Ok(match (idx, name) {
        (Some(i), _) => {
            let i = usize::try_from(i).map_err(|_| corrupt(format!("음수 인덱스 {i}")))?;
            Some(Ref::Resolved(T::from_index(i)))
        }
        (None, Some(n)) => Some(Ref::Unresolved(n)),
        (None, None) => None,
    })
}

fn req_ref<T: ArenaId>(idx: Option<i64>, name: Option<String>) -> Result<Ref<T>> {
    Ok(make_ref(idx, name)?.unwrap_or_else(|| Ref::Unresolved(String::new())))
}

fn rows<T>(
    conn: &Connection,
    sql: &str,
    rev: i64,
    f: impl FnMut(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>> {
    let mut st = conn.prepare_cached(sql)?;
    let out = st
        .query_map(params![rev], f)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(out)
}

/// arena 인덱스가 0부터 빈틈없이 이어지는지 확인한다.
fn check_dense(table: &str, idxs: impl Iterator<Item = i64>) -> Result<()> {
    for (pos, idx) in idxs.enumerate() {
        if idx != pos as i64 {
            return Err(corrupt(format!(
                "{table}: idx {idx} 가 위치 {pos} 와 다르다"
            )));
        }
    }
    Ok(())
}

/// 부모 인덱스별로 자식을 모은다.
fn group<T>(n: usize, table: &str, items: Vec<(i64, T)>) -> Result<Vec<Vec<T>>> {
    let mut out: Vec<Vec<T>> = (0..n).map(|_| Vec::new()).collect();
    for (parent, item) in items {
        let slot = usize::try_from(parent)
            .ok()
            .and_then(|p| out.get_mut(p))
            .ok_or_else(|| corrupt(format!("{table}: 부모 인덱스 {parent} 가 없다")))?;
        slot.push(item);
    }
    Ok(out)
}

pub(crate) fn list(conn: &Connection, project_id: i64) -> Result<Vec<RevisionInfo>> {
    rows(
        conn,
        "SELECT id, project_id, parent_id, label, source_name, created_at, protocol_ver, \
         language_ver, speed_bps, encoding FROM ldf_revision WHERE project_id = ?1 ORDER BY id",
        project_id,
        info_from_row,
    )
}

fn info_from_row(r: &Row<'_>) -> rusqlite::Result<RevisionInfo> {
    Ok(RevisionInfo {
        id: r.get(0)?,
        project_id: r.get(1)?,
        parent_id: r.get(2)?,
        label: r.get(3)?,
        source_name: r.get(4)?,
        created_at: r.get(5)?,
        protocol_version: r.get(6)?,
        language_version: r.get(7)?,
        speed_bps: r.get(8)?,
        encoding: r.get(9)?,
    })
}

pub(crate) fn load(conn: &Connection, id: i64) -> Result<StoredRevision> {
    let head = conn
        .query_row(
            "SELECT id, project_id, parent_id, label, source_name, created_at, protocol_ver, \
             language_ver, speed_bps, encoding, has_marker, ldf_file_revision, big_endian, \
             channel, source_text, section_order, lower_notes \
             FROM ldf_revision WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    info_from_row(r)?,
                    r.get::<_, bool>(10)?,
                    r.get::<_, Option<String>>(11)?,
                    r.get::<_, bool>(12)?,
                    r.get::<_, Option<String>>(13)?,
                    r.get::<_, Vec<u8>>(14)?,
                    r.get::<_, String>(15)?,
                    r.get::<_, String>(16)?,
                ))
            },
        )
        .optional()?;
    let Some((info, has_marker, ldf_file_revision, big_endian, channel, source, order, notes)) =
        head
    else {
        return Err(StoreError::NotFound(format!("리비전 {id}")));
    };

    let mut model = LdfModel::default();
    model.header.has_description_marker = has_marker;
    model.header.protocol_version = info.protocol_version.clone();
    model.header.language_version = info.language_version.clone();
    model.header.speed_bps = info.speed_bps;
    model.header.channel_name = channel;
    model.header.ldf_file_revision = ldf_file_revision;
    model.header.sig_byte_order_big_endian = big_endian;
    model.header.encoding = info.encoding.clone();
    model.section_order = serde_json::from_str(&order)?;
    model.notes = serde_json::from_str(&notes)?;

    load_nodes(conn, id, &mut model)?;
    load_encodings(conn, id, &mut model)?;
    load_signals(conn, id, &mut model)?;
    let sched_slots = load_schedules(conn, id, &mut model)?;
    load_frames(conn, id, &mut model)?;
    attach_slots(&mut model, sched_slots);
    load_node_attrs(conn, id, &mut model)?;
    load_raw_and_diagnostics(conn, id, &mut model).map(|diagnostics| StoredRevision {
        info,
        model,
        diagnostics,
        source,
    })
}

fn load_nodes(conn: &Connection, rev: i64, model: &mut LdfModel) -> Result<()> {
    let rows = rows(
        conn,
        "SELECT idx, name, role, timebase_ms, jitter_ms, max_header_bits, response_tolerance_pct, \
         src_start, src_end FROM node WHERE revision_id = ?1 ORDER BY idx",
        rev,
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<f64>>(3)?,
                r.get::<_, Option<f64>>(4)?,
                r.get::<_, Option<i64>>(5)?,
                r.get::<_, Option<f64>>(6)?,
                r.get::<_, Option<i64>>(7)?,
                r.get::<_, Option<i64>>(8)?,
            ))
        },
    )?;
    check_dense("node", rows.iter().map(|r| r.0))?;
    for (_, name, role, timebase_ms, jitter_ms, mhb, rtp, s0, s1) in rows {
        let role = match role.as_str() {
            "master" => NodeRole::Master,
            "slave" => NodeRole::Slave,
            other => return Err(corrupt(format!("node.role: {other}"))),
        };
        model.nodes.push(Node {
            name,
            role,
            timebase_ms,
            jitter_ms,
            max_header_bits: opt_u32(mhb, "max_header_bits")?,
            response_tolerance_pct: rtp,
            span: span_from(s0, s1),
        });
    }
    Ok(())
}

fn load_encodings(conn: &Connection, rev: i64, model: &mut LdfModel) -> Result<()> {
    let encs = rows(
        conn,
        "SELECT idx, name, src_start, src_end FROM encoding WHERE revision_id = ?1 ORDER BY idx",
        rev,
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, Option<i64>>(3)?,
            ))
        },
    )?;
    check_dense("encoding", encs.iter().map(|r| r.0))?;
    type EntryRow = (
        i64,
        String,
        Option<i64>,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<f64>,
        Option<f64>,
        Option<String>,
    );
    let entries: Vec<EntryRow> = rows(
        conn,
        "SELECT encoding_idx, kind, logical_value, text, min_raw, max_raw, scale, offset, unit \
         FROM encoding_entry WHERE revision_id = ?1 ORDER BY encoding_idx, pos",
        rev,
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            ))
        },
    )?;
    let mut grouped = Vec::new();
    for (enc, kind, logical, text, min, max, scale, offset, unit) in entries {
        let entry = match kind.as_str() {
            "logical" => EncodingEntry::Logical {
                value: logical.ok_or_else(|| corrupt("logical_value 없음"))? as u64,
                text: text.unwrap_or_default(),
            },
            "physical" => EncodingEntry::Physical {
                min: min.ok_or_else(|| corrupt("min_raw 없음"))?,
                max: max.ok_or_else(|| corrupt("max_raw 없음"))?,
                scale: scale.ok_or_else(|| corrupt("scale 없음"))?,
                offset: offset.ok_or_else(|| corrupt("offset 없음"))?,
                unit: unit.unwrap_or_default(),
            },
            "bcd" => EncodingEntry::Bcd,
            "ascii" => EncodingEntry::Ascii,
            other => return Err(corrupt(format!("encoding_entry.kind: {other}"))),
        };
        grouped.push((enc, entry));
    }
    let mut by_enc = group(encs.len(), "encoding_entry", grouped)?.into_iter();
    for (_, name, s0, s1) in encs {
        model.encodings.push(EncodingType {
            name,
            entries: by_enc.next().unwrap_or_default(),
            span: span_from(s0, s1),
        });
    }
    Ok(())
}

fn load_signals(conn: &Connection, rev: i64, model: &mut LdfModel) -> Result<()> {
    type SigRow = (
        i64,
        String,
        i64,
        Option<i64>,
        Option<Vec<u8>>,
        String,
        Option<i64>,
        Option<String>,
        Option<i64>,
        Option<i64>,
    );
    let sigs: Vec<SigRow> = rows(
        conn,
        "SELECT idx, name, size_bits, init_scalar, init_bytes, kind, publisher_idx, publisher_name, \
         src_start, src_end FROM signal WHERE revision_id = ?1 ORDER BY idx",
        rev,
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
            ))
        },
    )?;
    check_dense("signal", sigs.iter().map(|r| r.0))?;

    let subs = rows(
        conn,
        "SELECT signal_idx, node_idx, node_name FROM signal_subscriber \
         WHERE revision_id = ?1 ORDER BY signal_idx, pos",
        rev,
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        },
    )?;
    let mut sub_items = Vec::new();
    for (s, i, n) in subs {
        sub_items.push((s, req_ref::<NodeId>(i, n)?));
    }
    let mut subs = group(sigs.len(), "signal_subscriber", sub_items)?.into_iter();

    let encs = rows(
        conn,
        "SELECT signal_idx, enc_idx, enc_name FROM signal_encoding \
         WHERE revision_id = ?1 ORDER BY signal_idx, pos",
        rev,
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        },
    )?;
    let mut enc_items = Vec::new();
    for (s, i, n) in encs {
        enc_items.push((s, req_ref::<EncodingId>(i, n)?));
    }
    let mut encs = group(sigs.len(), "signal_encoding", enc_items)?.into_iter();

    for (_, name, size, scalar, bytes, kind, pi, pn, s0, s1) in sigs {
        let init = match (scalar, bytes) {
            (_, Some(b)) => InitValue::Array(b),
            (Some(v), None) => InitValue::Scalar(v as u64),
            (None, None) => InitValue::Scalar(0),
        };
        let kind = match kind.as_str() {
            "normal" => SignalKind::Normal,
            "diagnostic" => SignalKind::Diagnostic,
            other => return Err(corrupt(format!("signal.kind: {other}"))),
        };
        model.signals.push(Signal {
            name,
            size_bits: u32_of(size, "size_bits")?,
            init,
            publisher: make_ref::<NodeId>(pi, pn)?,
            subscribers: subs.next().unwrap_or_default(),
            encodings: encs.next().unwrap_or_default(),
            kind,
            span: span_from(s0, s1),
        });
    }
    Ok(())
}

/// 슬롯은 프레임이 복원된 뒤에 붙이므로 일단 모아서 돌려준다.
fn load_schedules(conn: &Connection, rev: i64, model: &mut LdfModel) -> Result<Vec<Vec<Slot>>> {
    let schedules = rows(
        conn,
        "SELECT idx, name, src_start, src_end FROM schedule WHERE revision_id = ?1 ORDER BY idx",
        rev,
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, Option<i64>>(3)?,
            ))
        },
    )?;
    check_dense("schedule", schedules.iter().map(|r| r.0))?;

    type SlotRow = (
        i64,
        f64,
        String,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
    );
    let slot_rows: Vec<SlotRow> = rows(
        conn,
        "SELECT schedule_idx, delay_ms, kind, frame_idx, frame_name, entry_json, src_start, src_end \
         FROM schedule_slot WHERE revision_id = ?1 ORDER BY schedule_idx, pos",
        rev,
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
            ))
        },
    )?;
    let mut items = Vec::new();
    for (sched, delay_ms, kind, fi, fname, json, s0, s1) in slot_rows {
        let entry = match kind.as_str() {
            "frame" => SlotEntry::Frame(req_ref::<FrameId>(fi, fname)?),
            "command" => serde_json::from_str(&json.ok_or_else(|| corrupt("entry_json 없음"))?)?,
            other => return Err(corrupt(format!("schedule_slot.kind: {other}"))),
        };
        items.push((
            sched,
            Slot {
                entry,
                delay_ms,
                span: span_from(s0, s1),
            },
        ));
    }
    let grouped = group(schedules.len(), "schedule_slot", items)?;

    for (_, name, s0, s1) in schedules {
        model.schedules.push(ScheduleTable {
            name,
            slots: Vec::new(),
            span: span_from(s0, s1),
        });
    }
    Ok(grouped)
}

fn attach_slots(model: &mut LdfModel, slots: Vec<Vec<Slot>>) {
    for (i, s) in slots.into_iter().enumerate() {
        if let Some(t) = model.schedules.get_mut(ScheduleId::from_index(i)) {
            t.slots = s;
        }
    }
}

fn load_frames(conn: &Connection, rev: i64, model: &mut LdfModel) -> Result<()> {
    type FrameRow = (
        i64,
        String,
        String,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<i64>,
        Option<i64>,
    );
    let frames: Vec<FrameRow> = rows(
        conn,
        "SELECT idx, name, kind, lin_id, size_bytes, publisher_idx, publisher_name, collision_idx, \
         collision_name, src_start, src_end FROM frame WHERE revision_id = ?1 ORDER BY idx",
        rev,
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
                r.get(10)?,
            ))
        },
    )?;
    check_dense("frame", frames.iter().map(|r| r.0))?;

    let fs = rows(
        conn,
        "SELECT frame_idx, signal_idx, signal_name, bit_offset FROM frame_signal \
         WHERE revision_id = ?1 ORDER BY frame_idx, pos",
        rev,
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, i64>(3)?,
            ))
        },
    )?;
    let mut fs_items = Vec::new();
    for (f, si, sn, off) in fs {
        fs_items.push((
            f,
            FrameSignal {
                signal: req_ref::<SignalId>(si, sn)?,
                offset: u32_of(off, "bit_offset")?,
            },
        ));
    }
    let mut signals = group(frames.len(), "frame_signal", fs_items)?.into_iter();

    let fa = rows(
        conn,
        "SELECT frame_idx, assoc_idx, assoc_name FROM frame_assoc \
         WHERE revision_id = ?1 ORDER BY frame_idx, pos",
        rev,
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        },
    )?;
    let mut fa_items = Vec::new();
    for (f, i, n) in fa {
        fa_items.push((f, req_ref::<FrameId>(i, n)?));
    }
    let mut assoc = group(frames.len(), "frame_assoc", fa_items)?.into_iter();

    for (_, name, kind, lin_id, size, pi, pn, ci, cn, s0, s1) in frames {
        let sigs = signals.next().unwrap_or_default();
        let assoc = assoc.next().unwrap_or_default();
        let lin = |what: &str| -> Result<u32> {
            u32_of(
                lin_id.ok_or_else(|| corrupt(format!("{what}: lin_id 없음")))?,
                "lin_id",
            )
        };
        let kind = match kind.as_str() {
            "unconditional" => FrameKind::Unconditional {
                id: lin("unconditional")?,
                publisher: req_ref::<NodeId>(pi, pn)?,
                size: u32_of(
                    size.ok_or_else(|| corrupt("size_bytes 없음"))?,
                    "size_bytes",
                )?,
                signals: sigs,
            },
            "sporadic" => FrameKind::Sporadic { associated: assoc },
            "event_triggered" => FrameKind::EventTriggered {
                id: lin("event_triggered")?,
                collision_table: make_ref::<ScheduleId>(ci, cn)?,
                associated: assoc,
            },
            "diagnostic" => FrameKind::Diagnostic {
                id: lin("diagnostic")?,
                signals: sigs,
            },
            other => return Err(corrupt(format!("frame.kind: {other}"))),
        };
        model.frames.push(Frame {
            name,
            kind,
            span: span_from(s0, s1),
        });
    }
    Ok(())
}

fn load_node_attrs(conn: &Connection, rev: i64, model: &mut LdfModel) -> Result<()> {
    type AttrRow = (
        i64,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<String>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        String,
        Option<i64>,
        Option<i64>,
    );
    let attrs: Vec<AttrRow> = rows(
        conn,
        "SELECT idx, node_idx, node_name, lin_protocol, configured_nad, initial_nad, \
         product_supplier, product_function, product_variant, resp_err_idx, resp_err_name, \
         p2_min_ms, st_min_ms, n_as_ms, n_cr_ms, extra_json, src_start, src_end \
         FROM node_attr WHERE revision_id = ?1 ORDER BY idx",
        rev,
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
                r.get(10)?,
                r.get(11)?,
                r.get(12)?,
                r.get(13)?,
                r.get(14)?,
                r.get(15)?,
                r.get(16)?,
                r.get(17)?,
            ))
        },
    )?;
    check_dense("node_attr", attrs.iter().map(|r| r.0))?;

    let fault = rows(
        conn,
        "SELECT attr_idx, signal_idx, signal_name FROM node_attr_fault_signal \
         WHERE revision_id = ?1 ORDER BY attr_idx, pos",
        rev,
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        },
    )?;
    let mut fault_items = Vec::new();
    for (a, i, n) in fault {
        fault_items.push((a, req_ref::<SignalId>(i, n)?));
    }
    let mut fault = group(attrs.len(), "node_attr_fault_signal", fault_items)?.into_iter();

    let cfg = rows(
        conn,
        "SELECT attr_idx, frame_idx, frame_name, message_id FROM node_config_frame \
         WHERE revision_id = ?1 ORDER BY attr_idx, pos",
        rev,
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<i64>>(3)?,
            ))
        },
    )?;
    let mut cfg_items = Vec::new();
    for (a, i, n, mid) in cfg {
        cfg_items.push((
            a,
            ConfigurableFrame {
                frame: req_ref::<FrameId>(i, n)?,
                message_id: opt_u32(mid, "message_id")?,
            },
        ));
    }
    let mut cfg = group(attrs.len(), "node_config_frame", cfg_items)?.into_iter();

    for (_, ni, nn, proto, cnad, inad, sup, fun, var, ri, rn, p2, st, nas, ncr, extra, s0, s1) in
        attrs
    {
        let product_id = match (sup, fun) {
            (Some(s), Some(f)) => Some(ProductId {
                supplier: u32_of(s, "product_supplier")?,
                function: u32_of(f, "product_function")?,
                variant: opt_u32(var, "product_variant")?,
            }),
            _ => None,
        };
        let extra: Vec<RawAttr> = serde_json::from_str(&extra)?;
        model.node_attrs.push(NodeAttrs {
            node: req_ref::<NodeId>(ni, nn)?,
            lin_protocol: proto,
            configured_nad: opt_u32(cnad, "configured_nad")?,
            initial_nad: opt_u32(inad, "initial_nad")?,
            product_id,
            response_error: make_ref::<SignalId>(ri, rn)?,
            fault_state_signals: fault.next().unwrap_or_default(),
            p2_min_ms: p2,
            st_min_ms: st,
            n_as_timeout_ms: nas,
            n_cr_timeout_ms: ncr,
            configurable_frames: cfg.next().unwrap_or_default(),
            extra,
            span: span_from(s0, s1),
        });
    }
    Ok(())
}

fn load_raw_and_diagnostics(
    conn: &Connection,
    rev: i64,
    model: &mut LdfModel,
) -> Result<Vec<Diagnostic>> {
    let raws = rows(
        conn,
        "SELECT name, text, src_start, src_end FROM raw_section WHERE revision_id = ?1 ORDER BY pos",
        rev,
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, Option<i64>>(3)?,
            ))
        },
    )?;
    for (name, text, s0, s1) in raws {
        model.unknown_sections.push(RawSection {
            name,
            text,
            span: span_from(s0, s1),
        });
    }

    let diags = rows(
        conn,
        "SELECT code, severity, src_start, src_end, message FROM diagnostic \
         WHERE revision_id = ?1 ORDER BY pos",
        rev,
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, String>(4)?,
            ))
        },
    )?;
    let mut out = Vec::new();
    for (code, sev, s0, s1, message) in diags {
        let severity = match sev.as_str() {
            "info" => Severity::Info,
            "warning" => Severity::Warning,
            "error" => Severity::Error,
            other => return Err(corrupt(format!("diagnostic.severity: {other}"))),
        };
        out.push(Diagnostic {
            code: u16::try_from(code).map_err(|_| corrupt(format!("진단 코드 {code}")))?,
            severity,
            span: span_from(s0, s1),
            message,
        });
    }
    Ok(out)
}
