//! 모델 -> DB. 리비전 하나를 단일 트랜잭션으로 쓴다.

use crate::{NewRevision, Result};
use ldf_model::{
    ArenaId, EncodingEntry, FrameKind, InitValue, NodeRole, Ref, Severity, SignalKind, SlotEntry,
    Span,
};
use rusqlite::{Connection, params};

/// FNV-1a 64비트. 동일 파일 재임포트 감지용(충돌 시에는 원본 바이트를 직접 비교한다).
pub(crate) fn source_hash(bytes: &[u8]) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h as i64
}

fn ix(id: impl ArenaId) -> i64 {
    id.index() as i64
}

fn span_cols(s: Option<Span>) -> (Option<i64>, Option<i64>) {
    match s {
        Some(s) => (Some(s.start as i64), Some(s.end as i64)),
        None => (None, None),
    }
}

/// 참조 컬럼 두 개: 해석되면 인덱스, 아니면 이름.
pub(crate) fn ref_cols<T: ArenaId>(r: &Ref<T>) -> (Option<i64>, Option<&str>) {
    match r {
        Ref::Resolved(id) => (Some(ix(*id)), None),
        Ref::Unresolved(name) => (None, Some(name.as_str())),
    }
}

fn opt_ref_cols<T: ArenaId>(r: Option<&Ref<T>>) -> (Option<i64>, Option<&str>) {
    r.map_or((None, None), ref_cols)
}

pub(crate) fn severity_str(s: Severity) -> &'static str {
    match s {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Error => "error",
    }
}

pub(crate) fn save(conn: &mut Connection, r: &NewRevision<'_>) -> Result<i64> {
    let tx = conn.transaction()?;
    let m = r.model;
    let h = &m.header;

    tx.execute(
        "INSERT INTO ldf_revision (project_id, parent_id, label, source_name, has_marker, \
         protocol_ver, language_ver, speed_bps, channel, ldf_file_revision, big_endian, encoding, \
         source_hash, source_text, section_order, lower_notes) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            r.project_id,
            r.parent_id,
            r.label,
            r.source_name,
            h.has_description_marker,
            h.protocol_version,
            h.language_version,
            h.speed_bps,
            h.channel_name,
            h.ldf_file_revision,
            h.sig_byte_order_big_endian,
            if h.encoding.is_empty() {
                "utf-8"
            } else {
                h.encoding.as_str()
            },
            source_hash(r.source),
            r.source,
            serde_json::to_string(&m.section_order)?,
            serde_json::to_string(&m.notes)?,
        ],
    )?;
    let rev = tx.last_insert_rowid();

    {
        let mut st = tx.prepare_cached(
            "INSERT INTO node (revision_id, idx, name, role, timebase_ms, jitter_ms, \
             max_header_bits, response_tolerance_pct, src_start, src_end) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        for (id, n) in m.nodes.iter() {
            let (s0, s1) = span_cols(n.span);
            let role = match n.role {
                NodeRole::Master => "master",
                NodeRole::Slave => "slave",
            };
            st.execute(params![
                rev,
                ix(id),
                n.name,
                role,
                n.timebase_ms,
                n.jitter_ms,
                n.max_header_bits,
                n.response_tolerance_pct,
                s0,
                s1
            ])?;
        }
    }

    {
        let mut enc = tx.prepare_cached(
            "INSERT INTO encoding (revision_id, idx, name, src_start, src_end) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        let mut ent = tx.prepare_cached(
            "INSERT INTO encoding_entry (revision_id, encoding_idx, pos, kind, logical_value, \
             text, min_raw, max_raw, scale, offset, unit) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )?;
        for (id, e) in m.encodings.iter() {
            let (s0, s1) = span_cols(e.span);
            enc.execute(params![rev, ix(id), e.name, s0, s1])?;
            for (pos, entry) in e.entries.iter().enumerate() {
                let pos = pos as i64;
                match entry {
                    EncodingEntry::Logical { value, text } => ent.execute(params![
                        rev,
                        ix(id),
                        pos,
                        "logical",
                        *value as i64,
                        text,
                        None::<i64>,
                        None::<i64>,
                        None::<f64>,
                        None::<f64>,
                        None::<String>
                    ])?,
                    EncodingEntry::Physical {
                        min,
                        max,
                        scale,
                        offset,
                        unit,
                    } => ent.execute(params![
                        rev,
                        ix(id),
                        pos,
                        "physical",
                        None::<i64>,
                        None::<String>,
                        min,
                        max,
                        scale,
                        offset,
                        unit
                    ])?,
                    EncodingEntry::Bcd => ent.execute(params![
                        rev,
                        ix(id),
                        pos,
                        "bcd",
                        None::<i64>,
                        None::<String>,
                        None::<i64>,
                        None::<i64>,
                        None::<f64>,
                        None::<f64>,
                        None::<String>
                    ])?,
                    EncodingEntry::Ascii => ent.execute(params![
                        rev,
                        ix(id),
                        pos,
                        "ascii",
                        None::<i64>,
                        None::<String>,
                        None::<i64>,
                        None::<i64>,
                        None::<f64>,
                        None::<f64>,
                        None::<String>
                    ])?,
                };
            }
        }
    }

    {
        let mut sig = tx.prepare_cached(
            "INSERT INTO signal (revision_id, idx, name, size_bits, init_scalar, init_bytes, kind, \
             publisher_idx, publisher_name, src_start, src_end) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )?;
        let mut sub = tx.prepare_cached(
            "INSERT INTO signal_subscriber (revision_id, signal_idx, pos, node_idx, node_name) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        let mut enc = tx.prepare_cached(
            "INSERT INTO signal_encoding (revision_id, signal_idx, pos, enc_idx, enc_name) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for (id, s) in m.signals.iter() {
            let (s0, s1) = span_cols(s.span);
            let (init_scalar, init_bytes): (Option<i64>, Option<&[u8]>) = match &s.init {
                InitValue::Scalar(v) => (Some(*v as i64), None),
                InitValue::Array(b) => (None, Some(b.as_slice())),
            };
            let kind = match s.kind {
                SignalKind::Normal => "normal",
                SignalKind::Diagnostic => "diagnostic",
            };
            let (pi, pn) = opt_ref_cols(s.publisher.as_ref());
            sig.execute(params![
                rev,
                ix(id),
                s.name,
                s.size_bits,
                init_scalar,
                init_bytes,
                kind,
                pi,
                pn,
                s0,
                s1
            ])?;
            for (pos, r) in s.subscribers.iter().enumerate() {
                let (i, n) = ref_cols(r);
                sub.execute(params![rev, ix(id), pos as i64, i, n])?;
            }
            for (pos, r) in s.encodings.iter().enumerate() {
                let (i, n) = ref_cols(r);
                enc.execute(params![rev, ix(id), pos as i64, i, n])?;
            }
        }
    }

    {
        let mut st = tx.prepare_cached(
            "INSERT INTO schedule (revision_id, idx, name, src_start, src_end) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for (id, s) in m.schedules.iter() {
            let (s0, s1) = span_cols(s.span);
            st.execute(params![rev, ix(id), s.name, s0, s1])?;
        }
    }

    {
        let mut fr = tx.prepare_cached(
            "INSERT INTO frame (revision_id, idx, name, kind, lin_id, size_bytes, publisher_idx, \
             publisher_name, collision_idx, collision_name, src_start, src_end) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )?;
        let mut fs = tx.prepare_cached(
            "INSERT INTO frame_signal (revision_id, frame_idx, pos, signal_idx, signal_name, \
             bit_offset) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        let mut fa = tx.prepare_cached(
            "INSERT INTO frame_assoc (revision_id, frame_idx, pos, assoc_idx, assoc_name) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for (id, f) in m.frames.iter() {
            let (s0, s1) = span_cols(f.span);
            let mut signals = &[][..];
            let mut assoc = &[][..];
            let (kind, lin_id, size, publisher, collision) = match &f.kind {
                FrameKind::Unconditional {
                    id,
                    publisher,
                    size,
                    signals: s,
                } => {
                    signals = s.as_slice();
                    (
                        "unconditional",
                        Some(*id),
                        Some(*size),
                        Some(publisher),
                        None,
                    )
                }
                FrameKind::Sporadic { associated } => {
                    assoc = associated.as_slice();
                    ("sporadic", None, None, None, None)
                }
                FrameKind::EventTriggered {
                    id,
                    collision_table,
                    associated,
                } => {
                    assoc = associated.as_slice();
                    (
                        "event_triggered",
                        Some(*id),
                        None,
                        None,
                        collision_table.as_ref(),
                    )
                }
                FrameKind::Diagnostic { id, signals: s } => {
                    signals = s.as_slice();
                    ("diagnostic", Some(*id), None, None, None)
                }
            };
            let (pi, pn) = opt_ref_cols(publisher);
            let (ci, cn) = opt_ref_cols(collision);
            fr.execute(params![
                rev,
                ix(id),
                f.name,
                kind,
                lin_id,
                size,
                pi,
                pn,
                ci,
                cn,
                s0,
                s1
            ])?;
            for (pos, s) in signals.iter().enumerate() {
                let (i, n) = ref_cols(&s.signal);
                fs.execute(params![rev, ix(id), pos as i64, i, n, s.offset])?;
            }
            for (pos, a) in assoc.iter().enumerate() {
                let (i, n) = ref_cols(a);
                fa.execute(params![rev, ix(id), pos as i64, i, n])?;
            }
        }
    }

    {
        let mut st = tx.prepare_cached(
            "INSERT INTO schedule_slot (revision_id, schedule_idx, pos, delay_ms, kind, frame_idx, \
             frame_name, entry_json, src_start, src_end) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        for (sid, sch) in m.schedules.iter() {
            for (pos, slot) in sch.slots.iter().enumerate() {
                let (s0, s1) = span_cols(slot.span);
                match &slot.entry {
                    SlotEntry::Frame(r) => {
                        let (i, n) = ref_cols(r);
                        st.execute(params![
                            rev,
                            ix(sid),
                            pos as i64,
                            slot.delay_ms,
                            "frame",
                            i,
                            n,
                            None::<String>,
                            s0,
                            s1
                        ])?;
                    }
                    other => {
                        st.execute(params![
                            rev,
                            ix(sid),
                            pos as i64,
                            slot.delay_ms,
                            "command",
                            None::<i64>,
                            None::<String>,
                            serde_json::to_string(other)?,
                            s0,
                            s1
                        ])?;
                    }
                }
            }
        }
    }

    {
        let mut at = tx.prepare_cached(
            "INSERT INTO node_attr (revision_id, idx, node_idx, node_name, lin_protocol, \
             configured_nad, initial_nad, product_supplier, product_function, product_variant, \
             resp_err_idx, resp_err_name, p2_min_ms, st_min_ms, n_as_ms, n_cr_ms, extra_json, \
             src_start, src_end) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
        )?;
        let mut fault = tx.prepare_cached(
            "INSERT INTO node_attr_fault_signal (revision_id, attr_idx, pos, signal_idx, \
             signal_name) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        let mut cf = tx.prepare_cached(
            "INSERT INTO node_config_frame (revision_id, attr_idx, pos, frame_idx, frame_name, \
             message_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for (idx, a) in m.node_attrs.iter().enumerate() {
            let idx = idx as i64;
            let (s0, s1) = span_cols(a.span);
            let (ni, nn) = ref_cols(&a.node);
            let (ri, rn) = opt_ref_cols(a.response_error.as_ref());
            let (sup, fun, var) = match &a.product_id {
                Some(p) => (Some(p.supplier), Some(p.function), p.variant),
                None => (None, None, None),
            };
            at.execute(params![
                rev,
                idx,
                ni,
                nn,
                a.lin_protocol,
                a.configured_nad,
                a.initial_nad,
                sup,
                fun,
                var,
                ri,
                rn,
                a.p2_min_ms,
                a.st_min_ms,
                a.n_as_timeout_ms,
                a.n_cr_timeout_ms,
                serde_json::to_string(&a.extra)?,
                s0,
                s1
            ])?;
            for (pos, r) in a.fault_state_signals.iter().enumerate() {
                let (i, n) = ref_cols(r);
                fault.execute(params![rev, idx, pos as i64, i, n])?;
            }
            for (pos, c) in a.configurable_frames.iter().enumerate() {
                let (i, n) = ref_cols(&c.frame);
                cf.execute(params![rev, idx, pos as i64, i, n, c.message_id])?;
            }
        }
    }

    {
        let mut st = tx.prepare_cached(
            "INSERT INTO raw_section (revision_id, pos, name, text, src_start, src_end) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for (pos, s) in m.unknown_sections.iter().enumerate() {
            let (s0, s1) = span_cols(s.span);
            st.execute(params![rev, pos as i64, s.name, s.text, s0, s1])?;
        }
    }

    {
        let mut st = tx.prepare_cached(
            "INSERT INTO diagnostic (revision_id, pos, code, severity, src_start, src_end, message) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for (pos, d) in r.diagnostics.iter().enumerate() {
            let (s0, s1) = span_cols(d.span);
            st.execute(params![
                rev,
                pos as i64,
                d.code,
                severity_str(d.severity),
                s0,
                s1,
                d.message
            ])?;
        }
    }

    tx.commit()?;
    Ok(rev)
}
