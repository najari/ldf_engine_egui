//! 모델 -> LDF 텍스트. 섹션 하나씩 문자열 블록으로 만든다.

use ldf_model::{
    Composition, EncodingEntry, EncodingId, FrameId, FrameKind, FrameSignal, InitValue, LdfModel,
    NodeId, NodeRole, Ref, ScheduleId, SignalId, SignalKind, SlotEntry,
};

fn name_of<'a, T: Copy>(r: &'a Ref<T>, lookup: impl Fn(T) -> Option<&'a str>) -> &'a str {
    match r {
        Ref::Resolved(id) => lookup(*id).unwrap_or(""),
        Ref::Unresolved(n) => n.as_str(),
    }
}

pub(crate) struct Names<'a>(pub &'a LdfModel);

impl<'a> Names<'a> {
    fn node(&self, r: &'a Ref<NodeId>) -> &'a str {
        name_of(r, |id| self.0.nodes.get(id).map(|n| n.name.as_str()))
    }
    fn signal(&self, r: &'a Ref<SignalId>) -> &'a str {
        name_of(r, |id| self.0.signals.get(id).map(|n| n.name.as_str()))
    }
    fn frame(&self, r: &'a Ref<FrameId>) -> &'a str {
        name_of(r, |id| self.0.frames.get(id).map(|n| n.name.as_str()))
    }
    fn encoding(&self, r: &'a Ref<EncodingId>) -> &'a str {
        name_of(r, |id| self.0.encodings.get(id).map(|n| n.name.as_str()))
    }
    fn schedule(&self, r: &'a Ref<ScheduleId>) -> &'a str {
        name_of(r, |id| self.0.schedules.get(id).map(|n| n.name.as_str()))
    }
}

/// 숫자 표기: 정수면 소수점 없이, 아니면 가장 짧은 십진 표기.
pub(crate) fn num(v: f64) -> String {
    if v.is_finite() && v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn ms(v: f64) -> String {
    format!("{} ms", num(v))
}

/// LDF 문자열에는 이스케이프가 없으므로 따옴표는 작은따옴표로 바꾼다.
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "'"))
}

fn hex(v: u32) -> String {
    format!("0x{v:02X}")
}

fn init(v: &InitValue) -> String {
    match v {
        InitValue::Scalar(n) => n.to_string(),
        InitValue::Array(b) => {
            format!(
                "{{{}}}",
                b.iter().map(u8::to_string).collect::<Vec<_>>().join(", ")
            )
        }
    }
}

/// 원문 그대로 보관한 문장: 끝의 `;`를 보장한다.
fn verbatim_stmt(text: &str) -> String {
    let t = text.trim();
    if t.ends_with(';') || t.ends_with('}') {
        t.to_owned()
    } else {
        format!("{t};")
    }
}

fn section(name: &str, body: &str) -> String {
    if body.is_empty() {
        format!("{name} {{\n}}\n")
    } else {
        format!("{name} {{\n{body}}}\n")
    }
}

// ---- 헤더 ------------------------------------------------------------------------------------

/// 헤더 항목 하나. 해당 값이 없으면 `None`.
pub(crate) fn header_item(m: &LdfModel, key: &str) -> Option<String> {
    let h = &m.header;
    match key {
        "LIN_description_file" => h
            .has_description_marker
            .then(|| "LIN_description_file;\n".to_owned()),
        "LIN_protocol_version" => h
            .protocol_version
            .as_ref()
            .map(|v| format!("LIN_protocol_version = {};\n", quote(v))),
        "LIN_language_version" => h
            .language_version
            .as_ref()
            .map(|v| format!("LIN_language_version = {};\n", quote(v))),
        "LDF_file_revision" => h
            .ldf_file_revision
            .as_ref()
            .map(|v| format!("LDF_file_revision = {};\n", quote(v))),
        "LIN_speed" => h
            .speed_bps
            .map(|bps| format!("LIN_speed = {} kbps;\n", num(f64::from(bps) / 1000.0))),
        "Channel_name" => h
            .channel_name
            .as_ref()
            .map(|v| format!("Channel_name = {};\n", quote(v))),
        "LIN_sig_byte_order_big_endian" => h
            .sig_byte_order_big_endian
            .then(|| "LIN_sig_byte_order_big_endian;\n".to_owned()),
        _ => None,
    }
}

pub(crate) const HEADER_KEYS: [&str; 7] = [
    "LIN_description_file",
    "LIN_protocol_version",
    "LIN_language_version",
    "LDF_file_revision",
    "LIN_speed",
    "Channel_name",
    "LIN_sig_byte_order_big_endian",
];

// ---- 섹션 -------------------------------------------------------------------------------------

pub(crate) fn nodes(m: &LdfModel) -> Option<String> {
    let mut body = String::new();
    for (_, n) in m.nodes.iter().filter(|(_, n)| n.role == NodeRole::Master) {
        let mut parts = vec![n.name.clone()];
        let j2602 = n.max_header_bits.is_some() || n.response_tolerance_pct.is_some();
        // 원문에 없던 값을 만들어 내지 않는다: 지터가 없으면 타임베이스만 쓴다
        if n.jitter_ms.is_some() || j2602 {
            parts.push(ms(n.timebase_ms.unwrap_or(0.0)));
            parts.push(ms(n.jitter_ms.unwrap_or(0.0)));
        } else if let Some(t) = n.timebase_ms {
            parts.push(ms(t));
        }
        if let Some(bits) = n.max_header_bits {
            parts.push(format!("{bits} bits"));
        }
        if let Some(p) = n.response_tolerance_pct {
            parts.push(format!("{} %", num(p)));
        }
        body.push_str(&format!("  Master: {};\n", parts.join(", ")));
    }
    let slaves: Vec<&str> = m
        .nodes
        .iter()
        .filter(|(_, n)| n.role == NodeRole::Slave)
        .map(|(_, n)| n.name.as_str())
        .collect();
    if !slaves.is_empty() {
        body.push_str(&format!("  Slaves: {};\n", slaves.join(", ")));
    }
    Some(section("Nodes", &body))
}

pub(crate) fn signals(m: &LdfModel, kind: SignalKind) -> Option<String> {
    let names = Names(m);
    let mut body = String::new();
    for (_, s) in m.signals.iter().filter(|(_, s)| s.kind == kind) {
        let mut parts = vec![s.size_bits.to_string(), init(&s.init)];
        if kind == SignalKind::Normal {
            if let Some(p) = &s.publisher {
                parts.push(names.node(p).to_owned());
                parts.extend(s.subscribers.iter().map(|r| names.node(r).to_owned()));
            } else if !s.subscribers.is_empty() {
                // 발행자 자리를 비울 수 없으므로 빈 이름으로 채운다(다시 읽으면 미해석 참조가 된다)
                parts.push(String::new());
                parts.extend(s.subscribers.iter().map(|r| names.node(r).to_owned()));
            }
        }
        body.push_str(&format!("  {}: {};\n", s.name, parts.join(", ")));
    }
    let name = if kind == SignalKind::Normal {
        "Signals"
    } else {
        "Diagnostic_signals"
    };
    Some(section(name, &body))
}

fn frame_signals(names: &Names<'_>, signals: &[FrameSignal]) -> String {
    signals
        .iter()
        .map(|s| format!("    {}, {};\n", names.signal(&s.signal), s.offset))
        .collect()
}

pub(crate) fn frames(m: &LdfModel, which: &str) -> Option<String> {
    let names = Names(m);
    let mut body = String::new();
    for (_, f) in m.frames.iter() {
        match (&f.kind, which) {
            (
                FrameKind::Unconditional {
                    id,
                    publisher,
                    size,
                    signals,
                },
                "Frames",
            ) => {
                body.push_str(&format!(
                    "  {}: {id}, {}, {size} {{\n{}  }}\n",
                    f.name,
                    names.node(publisher),
                    frame_signals(&names, signals)
                ));
            }
            (FrameKind::Sporadic { associated }, "Sporadic_frames") => {
                let list: Vec<&str> = associated.iter().map(|r| names.frame(r)).collect();
                body.push_str(&format!("  {}: {};\n", f.name, list.join(", ")));
            }
            (
                FrameKind::EventTriggered {
                    id,
                    collision_table,
                    associated,
                },
                "Event_triggered_frames",
            ) => {
                let mut parts = Vec::new();
                if let Some(t) = collision_table {
                    parts.push(names.schedule(t).to_owned());
                }
                parts.push(id.to_string());
                parts.extend(associated.iter().map(|r| names.frame(r).to_owned()));
                body.push_str(&format!("  {}: {};\n", f.name, parts.join(", ")));
            }
            (FrameKind::Diagnostic { id, signals }, "Diagnostic_frames") => {
                body.push_str(&format!(
                    "  {}: {} {{\n{}  }}\n",
                    f.name,
                    hex(*id),
                    frame_signals(&names, signals)
                ));
            }
            _ => {}
        }
    }
    Some(section(which, &body))
}

pub(crate) fn node_attributes(m: &LdfModel) -> Option<String> {
    let names = Names(m);
    let mut body = String::new();
    for a in &m.node_attrs {
        let mut b = String::new();
        if let Some(v) = &a.lin_protocol {
            b.push_str(&format!("    LIN_protocol = {};\n", quote(v)));
        }
        if let Some(v) = a.configured_nad {
            b.push_str(&format!("    configured_NAD = {};\n", hex(v)));
        }
        if let Some(v) = a.initial_nad {
            b.push_str(&format!("    initial_NAD = {};\n", hex(v)));
        }
        if let Some(p) = &a.product_id {
            let mut v = vec![hex(p.supplier), hex(p.function)];
            if let Some(var) = p.variant {
                v.push(var.to_string());
            }
            b.push_str(&format!("    product_id = {};\n", v.join(", ")));
        }
        if let Some(r) = &a.response_error {
            b.push_str(&format!("    response_error = {};\n", names.signal(r)));
        }
        if !a.fault_state_signals.is_empty() {
            let l: Vec<&str> = a
                .fault_state_signals
                .iter()
                .map(|r| names.signal(r))
                .collect();
            b.push_str(&format!("    fault_state_signals = {};\n", l.join(", ")));
        }
        for (key, v) in [
            ("P2_min", a.p2_min_ms),
            ("ST_min", a.st_min_ms),
            ("N_As_timeout", a.n_as_timeout_ms),
            ("N_Cr_timeout", a.n_cr_timeout_ms),
        ] {
            if let Some(v) = v {
                b.push_str(&format!("    {key} = {};\n", ms(v)));
            }
        }
        if !a.configurable_frames.is_empty() {
            b.push_str("    configurable_frames {\n");
            for c in &a.configurable_frames {
                match c.message_id {
                    Some(id) => {
                        b.push_str(&format!("      {} = {};\n", names.frame(&c.frame), hex(id)))
                    }
                    None => b.push_str(&format!("      {};\n", names.frame(&c.frame))),
                }
            }
            b.push_str("    }\n");
        }
        for e in &a.extra {
            b.push_str(&format!("    {}\n", verbatim_stmt(&e.text)));
        }
        let node = match &a.node {
            Ref::Resolved(id) => m.nodes.get(*id).map_or("", |n| n.name.as_str()),
            Ref::Unresolved(n) => n.as_str(),
        };
        body.push_str(&format!("  {node} {{\n{b}  }}\n"));
    }
    Some(section("Node_attributes", &body))
}

pub(crate) fn schedule_tables(m: &LdfModel) -> Option<String> {
    let names = Names(m);
    let mut body = String::new();
    for (_, t) in m.schedules.iter() {
        let mut b = String::new();
        for s in &t.slots {
            let delay = ms(s.delay_ms);
            let line = match &s.entry {
                SlotEntry::Frame(r) => format!("{} delay {delay};", names.frame(r)),
                SlotEntry::AssignNad { node } => {
                    format!("AssignNAD {{ {} }} delay {delay};", names.node(node))
                }
                SlotEntry::ConditionalChangeNad { args } => {
                    format!(
                        "ConditionalChangeNAD {{ {} }} delay {delay};",
                        join_nums(args)
                    )
                }
                SlotEntry::DataDump { node, data } => {
                    let mut v = vec![names.node(node).to_owned()];
                    v.extend(data.iter().map(|d| hex(*d)));
                    format!("DataDump {{ {} }} delay {delay};", v.join(", "))
                }
                SlotEntry::SaveConfiguration { node } => {
                    format!(
                        "SaveConfiguration {{ {} }} delay {delay};",
                        names.node(node)
                    )
                }
                SlotEntry::AssignFrameIdRange {
                    node,
                    start_index,
                    pids,
                } => {
                    let mut v = vec![names.node(node).to_owned(), start_index.to_string()];
                    v.extend(pids.iter().map(|p| hex(*p)));
                    format!("AssignFrameIdRange {{ {} }} delay {delay};", v.join(", "))
                }
                SlotEntry::AssignFrameId { node, frame } => {
                    format!(
                        "AssignFrameId {{ {}, {} }} delay {delay};",
                        names.node(node),
                        names.frame(frame)
                    )
                }
                SlotEntry::FreeFormat { data } => {
                    let v: Vec<String> = data.iter().map(|d| hex(*d)).collect();
                    format!("FreeFormat {{ {} }} delay {delay};", v.join(", "))
                }
                SlotEntry::Unknown(text) => verbatim_stmt(text),
            };
            b.push_str(&format!("    {line}\n"));
        }
        body.push_str(&format!("  {} {{\n{b}  }}\n", t.name));
    }
    Some(section("Schedule_tables", &body))
}

fn join_nums(v: &[u32]) -> String {
    v.iter().map(|d| hex(*d)).collect::<Vec<_>>().join(", ")
}

/// 노드 구성 섹션. 끝에 줄바꿈은 붙이지 않는다.
pub(crate) fn composition(c: &Composition) -> String {
    let mut s = format!("{} {{\n", c.keyword);
    for cfg in &c.configurations {
        s.push_str(&format!("  configuration {} {{\n", cfg.name));
        for node in &cfg.composites {
            let list = node.logical.join(", ");
            if c.colon_form {
                s.push_str(&format!("    {}: {list};\n", node.name));
            } else {
                s.push_str(&format!("    {} {{ {list}; }}\n", node.name));
            }
        }
        s.push_str("  }\n");
    }
    s.push_str("}\n");
    s.trim_end().to_owned()
}

pub(crate) fn encodings(m: &LdfModel) -> Option<String> {
    let mut body = String::new();
    for (_, e) in m.encodings.iter() {
        let mut b = String::new();
        for entry in &e.entries {
            let line = match entry {
                EncodingEntry::Logical { value, text } => {
                    format!("logical_value, {value}, {};", quote(text))
                }
                EncodingEntry::Physical {
                    min,
                    max,
                    scale,
                    offset,
                    unit,
                } => {
                    format!(
                        "physical_value, {min}, {max}, {}, {}, {};",
                        num(*scale),
                        num(*offset),
                        quote(unit)
                    )
                }
                EncodingEntry::Bcd => "bcd_value;".to_owned(),
                EncodingEntry::Ascii => "ascii_value;".to_owned(),
            };
            b.push_str(&format!("    {line}\n"));
        }
        body.push_str(&format!("  {} {{\n{b}  }}\n", e.name));
    }
    Some(section("Signal_encoding_types", &body))
}

/// `Signal_representation`. 시그널마다 인코딩 순서가 보존되도록 "순번별 라운드"로 출력한다:
/// 라운드 k는 각 인코딩에 대해 k번째 인코딩이 그것인 시그널을 모은다.
pub(crate) fn representation(m: &LdfModel) -> Option<String> {
    let names = Names(m);
    let rounds = m
        .signals
        .iter()
        .map(|(_, s)| s.encodings.len())
        .max()
        .unwrap_or(0);
    let mut body = String::new();
    for k in 0..rounds {
        // 같은 라운드 안에서 인코딩 이름별로 묶는다(이름 등장 순서 유지)
        let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
        for (_, s) in m.signals.iter() {
            let Some(enc) = s.encodings.get(k) else {
                continue;
            };
            let enc_name = names.encoding(enc);
            match groups.iter_mut().find(|(n, _)| *n == enc_name) {
                Some((_, v)) => v.push(s.name.as_str()),
                None => groups.push((enc_name, vec![s.name.as_str()])),
            }
        }
        for (enc, sigs) in groups {
            body.push_str(&format!("  {enc}: {};\n", sigs.join(", ")));
        }
    }
    Some(section("Signal_representation", &body))
}
