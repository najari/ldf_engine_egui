//! AST -> 모델 변환(lowering).
//!
//! 1) 섹션을 정해진 순서로 읽어 객체를 만들고(참조는 `Unresolved(이름)`),
//! 2) 마지막에 이름 -> ID 해석 패스로 참조를 `Resolved`로 바꾼다.
//!
//! 못 찾은 참조는 모델에 `Unresolved`로 남는다. 그 진단은 검사 규칙(`ldf-check`)의 몫이고,
//! 여기서는 구조 오류(숫자가 아님, 이름이 없음 등)만 진단한다. 오류가 있어도 모델은 계속 만든다.

use crate::arena::{EncodingId, FrameId, NodeId, ScheduleId, SignalId};
use crate::types::*;
use ldf_syntax::{Block, Diagnostic, Elem, File, Span, Stmt, Token, TokenKind, codes};
use std::collections::HashMap;
use std::collections::hash_map::Entry as MapEntry;

/// 읽기 결과: 모델과 진단.
#[derive(Clone, Debug)]
pub struct Loaded {
    /// 모델.
    pub model: LdfModel,
    /// 렉서·파서·lowering 진단.
    pub diagnostics: Vec<Diagnostic>,
}

/// 원문(이미 디코딩된 문자열)을 읽어 모델을 만든다.
pub fn load(src: &str) -> Loaded {
    let parsed = ldf_syntax::parse(src);
    let mut diagnostics = parsed.diagnostics;
    let (model, more) = lower(src, &parsed.file);
    diagnostics.extend(more);
    Loaded { model, diagnostics }
}

const CANONICAL: &[&str] = &[
    "Nodes",
    "Signals",
    "Diagnostic_signals",
    "Frames",
    "Sporadic_frames",
    "Event_triggered_frames",
    "Diagnostic_frames",
    "Node_attributes",
    "Schedule_tables",
    "Signal_encoding_types",
    "Signal_representation",
];

/// 실제 LDF 섹션이지만 아직 모델링하지 않아 원문으로 보관하는 섹션.
const KEPT_RAW: &[&str] = &[
    "Node_composition",
    "composite", // LIN 2.0 형식의 노드 구성
    "Signal_groups",
    "dynamic_frames",
    "Dynamic_frames",
    "Diagnostic_addresses",
];

/// 구문 트리를 모델로 바꾼다.
pub fn lower(src: &str, file: &File) -> (LdfModel, Vec<Diagnostic>) {
    let mut lw = Lowerer {
        src,
        model: LdfModel::default(),
        diags: Vec::new(),
        sym: Symbols::default(),
    };
    let mut sections: Vec<(&str, &Stmt, &Block)> = Vec::new();

    for stmt in &file.stmts {
        let Some(Elem::Token(first)) = stmt.elems.first() else {
            lw.bad(stmt.span, "문장이 이름으로 시작하지 않는다");
            continue;
        };
        if first.kind != TokenKind::Ident {
            lw.bad(stmt.span, "문장이 이름으로 시작하지 않는다");
            continue;
        }
        let name = first.span.text(src);
        lw.model.section_order.push(name.to_owned());

        match stmt.elems.as_slice() {
            [_] if name == "LIN_description_file" => lw.model.header.has_description_marker = true,
            [_] if name == "LIN_sig_byte_order_big_endian" => {
                lw.model.header.sig_byte_order_big_endian = true;
            }
            [_, Elem::Block(b)] => sections.push((name, stmt, b)),
            _ => match entry(stmt) {
                Some(e) if e.sep == Some('=') => lw.header_item(&e),
                _ => lw.bad(stmt.span, format!("해석할 수 없는 최상위 문장 `{name}`")),
            },
        }
    }

    for &canon in CANONICAL {
        let mut seen = 0;
        for &(name, stmt, block) in &sections {
            if name != canon {
                continue;
            }
            seen += 1;
            if seen == 2 {
                lw.diags.push(Diagnostic::warning(
                    codes::DUPLICATE_SECTION,
                    stmt.span,
                    format!("섹션 `{name}`이 두 번 이상 나온다"),
                ));
            }
            match canon {
                "Nodes" => lw.nodes(block),
                "Signals" => lw.signals(block, SignalKind::Normal),
                "Diagnostic_signals" => lw.signals(block, SignalKind::Diagnostic),
                "Frames" => lw.frames(block),
                "Sporadic_frames" => lw.sporadic_frames(block),
                "Event_triggered_frames" => lw.event_triggered_frames(block),
                "Diagnostic_frames" => lw.diagnostic_frames(block),
                "Node_attributes" => lw.node_attributes(block),
                "Schedule_tables" => lw.schedule_tables(block),
                "Signal_encoding_types" => lw.encodings(block),
                "Signal_representation" => lw.representation(block),
                _ => {}
            }
        }
    }

    for &(name, stmt, _) in &sections {
        if CANONICAL.contains(&name) {
            continue;
        }
        if !KEPT_RAW.contains(&name) {
            lw.diags.push(Diagnostic::warning(
                codes::UNKNOWN_SECTION,
                stmt.span,
                format!("알 수 없는 섹션 `{name}` (원문으로 보관)"),
            ));
        }
        lw.model.unknown_sections.push(RawSection {
            name: name.to_owned(),
            text: stmt.span.text(src).to_owned(),
            span: Some(stmt.span),
        });
    }

    lw.resolve();
    (lw.model, lw.diags)
}

// ---------------------------------------------------------------------------------------------
// 문장·그룹 도우미
// ---------------------------------------------------------------------------------------------

/// `이름 [: | =] 나머지` 형태로 본 문장.
struct Entry<'a> {
    name: Token,
    sep: Option<char>,
    rest: &'a [Elem],
    span: Span,
}

fn entry(stmt: &Stmt) -> Option<Entry<'_>> {
    let Some(Elem::Token(name)) = stmt.elems.first() else {
        return None;
    };
    if name.kind != TokenKind::Ident {
        return None;
    }
    let (sep, rest) = match stmt.elems.get(1) {
        Some(Elem::Token(Token {
            kind: TokenKind::Punct(c @ (':' | '=')),
            ..
        })) => (Some(*c), stmt.elems.get(2..).unwrap_or(&[])),
        _ => (None, stmt.elems.get(1..).unwrap_or(&[])),
    };
    Some(Entry {
        name: *name,
        sep,
        rest,
        span: stmt.span,
    })
}

/// 쉼표로 나눈 그룹. 빈 입력이면 그룹이 없다.
fn split_commas(elems: &[Elem]) -> Vec<&[Elem]> {
    if elems.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = 0;
    for (i, e) in elems.iter().enumerate() {
        if matches!(
            e,
            Elem::Token(Token {
                kind: TokenKind::Punct(','),
                ..
            })
        ) {
            out.push(elems.get(start..i).unwrap_or(&[]));
            start = i + 1;
        }
    }
    out.push(elems.get(start..).unwrap_or(&[]));
    out
}

fn split_trailing_block(rest: &[Elem]) -> (&[Elem], Option<&Block>) {
    match rest.split_last() {
        Some((Elem::Block(b), head)) => (head, Some(b)),
        _ => (rest, None),
    }
}

fn ident<'s>(src: &'s str, g: &[Elem]) -> Option<&'s str> {
    match g {
        [Elem::Token(t)] if t.kind == TokenKind::Ident => Some(t.span.text(src)),
        _ => None,
    }
}

/// 따옴표 문자열. 따옴표 없는 식별자도 관대하게 받는다(`Channel_name = Postfix`).
fn string_lit(src: &str, g: &[Elem]) -> Option<String> {
    match g {
        [Elem::Token(t)] if t.kind == TokenKind::Str => {
            let s = t.span.text(src);
            Some(s.trim_matches('"').to_owned())
        }
        [Elem::Token(t)] if t.kind == TokenKind::Ident => Some(t.span.text(src).to_owned()),
        _ => None,
    }
}

fn parse_number(text: &str) -> Option<f64> {
    let (neg, body) = match text.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let v = match body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        Some(h) => u64::from_str_radix(h, 16).ok()? as f64,
        None => body.parse::<f64>().ok()?,
    };
    Some(if neg { -v } else { v })
}

fn parse_u64(text: &str) -> Option<u64> {
    let body = text.strip_prefix('+').unwrap_or(text);
    match body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        Some(h) => u64::from_str_radix(h, 16).ok(),
        None => body.parse::<u64>().ok(),
    }
}

/// `[숫자]` 또는 `[숫자, 단위]`.
fn number(src: &str, g: &[Elem]) -> Option<f64> {
    match g {
        [Elem::Token(n)] | [Elem::Token(n), Elem::Token(_)] if n.kind == TokenKind::Number => {
            parse_number(n.span.text(src))
        }
        _ => None,
    }
}

fn to_u32(v: f64) -> Option<u32> {
    (v.is_finite() && v >= 0.0 && v.fract() == 0.0 && v <= f64::from(u32::MAX)).then_some(v as u32)
}

fn uint(src: &str, g: &[Elem]) -> Option<u32> {
    number(src, g).and_then(to_u32)
}

/// 시간 값(ms). 단위가 없으면 ms로 본다.
fn quantity_ms(src: &str, g: &[Elem]) -> Option<f64> {
    let n = number(src, g)?;
    let unit = match g {
        [_, Elem::Token(u)] if u.kind == TokenKind::Ident => u.span.text(src),
        _ => "ms",
    };
    Some(match unit {
        "s" => n * 1000.0,
        "us" => n / 1000.0,
        _ => n,
    })
}

/// `{ a, b, c }` 같은 블록의 쉼표 그룹(모든 문장을 이어 붙인다).
fn block_groups(b: &Block) -> Vec<&[Elem]> {
    b.stmts
        .iter()
        .flat_map(|s| split_commas(&s.elems))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// 이름 표
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Symbols {
    nodes: HashMap<String, NodeId>,
    signals: HashMap<String, SignalId>,
    frames: HashMap<String, FrameId>,
    encodings: HashMap<String, EncodingId>,
    schedules: HashMap<String, ScheduleId>,
}

/// 이름을 등록한다. 이미 있으면 첫 번째를 유지하고 `false`.
fn register<T>(table: &mut HashMap<String, T>, name: &str, id: T) -> bool {
    match table.entry(name.to_owned()) {
        MapEntry::Occupied(_) => false,
        MapEntry::Vacant(v) => {
            v.insert(id);
            true
        }
    }
}

fn fix<T: Copy>(r: &mut Ref<T>, table: &HashMap<String, T>) {
    if let Ref::Unresolved(name) = r
        && let Some(&id) = table.get(name.as_str())
    {
        *r = Ref::Resolved(id);
    }
}

fn unresolved<T>(name: &str) -> Ref<T> {
    Ref::Unresolved(name.to_owned())
}

// ---------------------------------------------------------------------------------------------
// 변환기
// ---------------------------------------------------------------------------------------------

struct Lowerer<'s> {
    src: &'s str,
    model: LdfModel,
    diags: Vec<Diagnostic>,
    sym: Symbols,
}

impl<'s> Lowerer<'s> {
    fn bad(&mut self, span: Span, msg: impl Into<String>) {
        self.diags
            .push(Diagnostic::error(codes::BAD_STATEMENT, span, msg));
    }

    fn dup(&mut self, span: Span, kind: &str, name: &str) {
        self.diags.push(Diagnostic::warning(
            codes::DUPLICATE_NAME,
            span,
            format!("{kind} 이름 `{name}`이 중복된다"),
        ));
    }

    /// 필수 숫자 필드. 없거나 형식이 틀리면 진단하고 0.
    fn need_uint(&mut self, g: Option<&&[Elem]>, span: Span, what: &str) -> u32 {
        match g.and_then(|g| uint(self.src, g)) {
            Some(v) => v,
            None => {
                self.diags.push(Diagnostic::error(
                    codes::EXPECTED_NUMBER,
                    span,
                    format!("{what}: 0 이상의 정수가 와야 한다"),
                ));
                0
            }
        }
    }

    fn need_name(&mut self, g: Option<&&[Elem]>, span: Span, what: &str) -> Option<&'s str> {
        let src = self.src;
        let name = g.and_then(|g| ident(src, g));
        if name.is_none() {
            self.diags.push(Diagnostic::error(
                codes::EXPECTED_NAME,
                span,
                format!("{what}: 이름이 와야 한다"),
            ));
        }
        name
    }

    // ---- 헤더 ------------------------------------------------------------------------------

    fn header_item(&mut self, e: &Entry<'_>) {
        let src = self.src;
        let key = e.name.span.text(src);
        match key {
            "LIN_protocol_version" => self.model.header.protocol_version = self.header_text(e),
            "LIN_language_version" => self.model.header.language_version = self.header_text(e),
            "Channel_name" => self.model.header.channel_name = self.header_text(e),
            "LDF_file_revision" => self.model.header.ldf_file_revision = self.header_text(e),
            "LIN_speed" => {
                let unit = match e.rest {
                    [_, Elem::Token(u)] if u.kind == TokenKind::Ident => u.span.text(src),
                    _ => "kbps",
                };
                let factor = match unit {
                    "bps" => 1.0,
                    "mbps" => 1_000_000.0,
                    _ => 1000.0,
                };
                match number(src, e.rest) {
                    Some(v) if v.is_finite() && v >= 0.0 => {
                        self.model.header.speed_bps = Some((v * factor).round() as u32);
                    }
                    _ => self.diags.push(Diagnostic::error(
                        codes::BAD_HEADER,
                        e.span,
                        "LIN_speed: `19.2 kbps` 형식이어야 한다",
                    )),
                }
            }
            _ => self.bad(e.span, format!("알 수 없는 헤더 항목 `{key}`")),
        }
    }

    fn header_text(&mut self, e: &Entry<'_>) -> Option<String> {
        let v = string_lit(self.src, e.rest);
        if v.is_none() {
            self.diags.push(Diagnostic::error(
                codes::BAD_HEADER,
                e.span,
                format!("{}: 문자열 값이 필요하다", e.name.span.text(self.src)),
            ));
        }
        v
    }

    // ---- Nodes -----------------------------------------------------------------------------

    fn add_node(&mut self, node: Node) {
        let name = node.name.clone();
        let span = node.span.unwrap_or_default();
        let id = self.model.nodes.push(node);
        if !register(&mut self.sym.nodes, &name, id) {
            self.dup(span, "노드", &name);
        }
    }

    fn nodes(&mut self, b: &Block) {
        let src = self.src;
        for stmt in &b.stmts {
            let Some(e) = entry(stmt).filter(|e| e.sep == Some(':')) else {
                self.bad(stmt.span, "Nodes: `Master:` 또는 `Slaves:` 문장이어야 한다");
                continue;
            };
            let groups = split_commas(e.rest);
            match e.name.span.text(src) {
                "Master" => {
                    let Some(name) = self.need_name(groups.first(), e.span, "Master 이름") else {
                        continue;
                    };
                    let timebase_ms = groups.get(1).and_then(|g| quantity_ms(src, g));
                    let jitter_ms = groups.get(2).and_then(|g| quantity_ms(src, g));
                    // SAE J2602: `Master: N, 1 ms, 0 ms, 48 bits, 40 %;`
                    let max_header_bits = groups.get(3).and_then(|g| uint(src, g));
                    let response_tolerance_pct = groups.get(4).and_then(|g| number(src, g));
                    self.add_node(Node {
                        name: name.to_owned(),
                        role: NodeRole::Master,
                        timebase_ms,
                        jitter_ms,
                        max_header_bits,
                        response_tolerance_pct,
                        span: Some(e.span),
                    });
                }
                "Slaves" => {
                    for g in &groups {
                        match ident(src, g) {
                            Some(name) => self.add_node(Node {
                                name: name.to_owned(),
                                role: NodeRole::Slave,
                                timebase_ms: None,
                                jitter_ms: None,
                                max_header_bits: None,
                                response_tolerance_pct: None,
                                span: Some(e.span),
                            }),
                            None => self.diags.push(Diagnostic::error(
                                codes::EXPECTED_NAME,
                                e.span,
                                "Slaves: 이름이 와야 한다",
                            )),
                        }
                    }
                }
                other => self.bad(e.span, format!("Nodes: 알 수 없는 항목 `{other}`")),
            }
        }
    }

    // ---- Signals ---------------------------------------------------------------------------

    fn init_value(&mut self, g: Option<&&[Elem]>, span: Span) -> InitValue {
        let src = self.src;
        match g.copied() {
            None => InitValue::Scalar(0),
            Some([Elem::Block(b)]) => {
                let mut bytes = Vec::new();
                for g in block_groups(b) {
                    match number(src, g)
                        .and_then(to_u32)
                        .and_then(|v| u8::try_from(v).ok())
                    {
                        Some(v) => bytes.push(v),
                        None => self.diags.push(Diagnostic::error(
                            codes::VALUE_OUT_OF_RANGE,
                            span,
                            "바이트 배열 초기값은 0–255여야 한다",
                        )),
                    }
                }
                InitValue::Array(bytes)
            }
            Some([Elem::Token(t)]) if t.kind == TokenKind::Number => {
                match parse_u64(t.span.text(src)) {
                    Some(v) => InitValue::Scalar(v),
                    None => {
                        self.diags.push(Diagnostic::error(
                            codes::VALUE_OUT_OF_RANGE,
                            span,
                            "초기값은 0 이상의 정수여야 한다",
                        ));
                        InitValue::Scalar(0)
                    }
                }
            }
            Some(_) => {
                self.diags.push(Diagnostic::error(
                    codes::EXPECTED_NUMBER,
                    span,
                    "초기값은 숫자 또는 `{...}` 배열이어야 한다",
                ));
                InitValue::Scalar(0)
            }
        }
    }

    fn signals(&mut self, b: &Block, kind: SignalKind) {
        let src = self.src;
        for stmt in &b.stmts {
            let Some(e) = entry(stmt).filter(|e| e.sep == Some(':')) else {
                self.bad(
                    stmt.span,
                    "Signals: `이름: 길이, 초기값, ...` 형식이어야 한다",
                );
                continue;
            };
            let name = e.name.span.text(src).to_owned();
            let groups = split_commas(e.rest);
            let size_bits = self.need_uint(groups.first(), e.span, "시그널 길이");
            let init = self.init_value(groups.get(1), e.span);
            let (publisher, subscribers) = if kind == SignalKind::Normal {
                let publisher = self
                    .need_name(groups.get(2), e.span, "발행자")
                    .map(unresolved);
                let subs = groups
                    .iter()
                    .skip(3)
                    .filter_map(|g| ident(src, g))
                    .map(unresolved)
                    .collect();
                (publisher, subs)
            } else {
                (None, Vec::new())
            };
            let id = self.model.signals.push(Signal {
                name: name.clone(),
                size_bits,
                init,
                publisher,
                subscribers,
                encodings: Vec::new(),
                kind,
                span: Some(e.span),
            });
            if !register(&mut self.sym.signals, &name, id) {
                self.dup(e.span, "시그널", &name);
            }
        }
    }

    // ---- Frames ----------------------------------------------------------------------------

    fn frame_signals(&mut self, b: &Block) -> Vec<FrameSignal> {
        let mut out = Vec::new();
        for stmt in &b.stmts {
            let groups = split_commas(&stmt.elems);
            let Some(name) = self.need_name(groups.first(), stmt.span, "프레임 시그널")
            else {
                continue;
            };
            let offset = self.need_uint(groups.get(1), stmt.span, "시작 비트");
            out.push(FrameSignal {
                signal: unresolved(name),
                offset,
            });
        }
        out
    }

    fn add_frame(&mut self, frame: Frame) {
        let name = frame.name.clone();
        let span = frame.span.unwrap_or_default();
        let id = self.model.frames.push(frame);
        if !register(&mut self.sym.frames, &name, id) {
            self.dup(span, "프레임", &name);
        }
    }

    fn frames(&mut self, b: &Block) {
        for stmt in &b.stmts {
            let Some(e) = entry(stmt).filter(|e| e.sep == Some(':')) else {
                self.bad(
                    stmt.span,
                    "Frames: `이름: ID, 발행자, 크기 { ... }` 형식이어야 한다",
                );
                continue;
            };
            let (head, block) = split_trailing_block(e.rest);
            let groups = split_commas(head);
            let id = self.need_uint(groups.first(), e.span, "프레임 ID");
            let publisher = self
                .need_name(groups.get(1), e.span, "프레임 발행자")
                .map(unresolved);
            let size = self.need_uint(groups.get(2), e.span, "프레임 크기");
            let signals = block.map(|b| self.frame_signals(b)).unwrap_or_default();
            self.add_frame(Frame {
                name: e.name.span.text(self.src).to_owned(),
                kind: FrameKind::Unconditional {
                    id,
                    publisher: publisher.unwrap_or_else(|| unresolved("")),
                    size,
                    signals,
                },
                span: Some(e.span),
            });
        }
    }

    fn name_list(&self, groups: &[&[Elem]]) -> Vec<&'s str> {
        groups.iter().filter_map(|g| ident(self.src, g)).collect()
    }

    fn sporadic_frames(&mut self, b: &Block) {
        for stmt in &b.stmts {
            let Some(e) = entry(stmt).filter(|e| e.sep == Some(':')) else {
                self.bad(
                    stmt.span,
                    "Sporadic_frames: `이름: 프레임, ...` 형식이어야 한다",
                );
                continue;
            };
            let associated = self
                .name_list(&split_commas(e.rest))
                .into_iter()
                .map(unresolved)
                .collect();
            self.add_frame(Frame {
                name: e.name.span.text(self.src).to_owned(),
                kind: FrameKind::Sporadic { associated },
                span: Some(e.span),
            });
        }
    }

    fn event_triggered_frames(&mut self, b: &Block) {
        let src = self.src;
        for stmt in &b.stmts {
            let Some(e) = entry(stmt).filter(|e| e.sep == Some(':')) else {
                self.bad(
                    stmt.span,
                    "Event_triggered_frames: `이름: [테이블,] ID, 프레임, ...` 형식이어야 한다",
                );
                continue;
            };
            let groups = split_commas(e.rest);
            // 충돌 해결 테이블은 생략할 수 있다(첫 항목이 숫자면 ID).
            let first_is_number = groups.first().is_some_and(|g| number(src, g).is_some());
            let (collision_table, id_idx) = if first_is_number {
                (None, 0)
            } else {
                (
                    groups.first().and_then(|g| ident(src, g)).map(unresolved),
                    1,
                )
            };
            let id = self.need_uint(groups.get(id_idx), e.span, "이벤트 트리거 프레임 ID");
            let rest = groups.get(id_idx + 1..).unwrap_or(&[]);
            let associated = self.name_list(rest).into_iter().map(unresolved).collect();
            self.add_frame(Frame {
                name: e.name.span.text(src).to_owned(),
                kind: FrameKind::EventTriggered {
                    id,
                    collision_table,
                    associated,
                },
                span: Some(e.span),
            });
        }
    }

    fn diagnostic_frames(&mut self, b: &Block) {
        for stmt in &b.stmts {
            let Some(e) = entry(stmt).filter(|e| e.sep == Some(':')) else {
                self.bad(
                    stmt.span,
                    "Diagnostic_frames: `이름: ID { ... }` 형식이어야 한다",
                );
                continue;
            };
            let (head, block) = split_trailing_block(e.rest);
            let groups = split_commas(head);
            let id = self.need_uint(groups.first(), e.span, "진단 프레임 ID");
            let signals = block.map(|b| self.frame_signals(b)).unwrap_or_default();
            self.add_frame(Frame {
                name: e.name.span.text(self.src).to_owned(),
                kind: FrameKind::Diagnostic { id, signals },
                span: Some(e.span),
            });
        }
    }

    // ---- Node_attributes -------------------------------------------------------------------

    fn node_attributes(&mut self, b: &Block) {
        let src = self.src;
        for stmt in &b.stmts {
            let (Some(e), [_, Elem::Block(body)]) = (entry(stmt), stmt.elems.as_slice()) else {
                self.bad(stmt.span, "Node_attributes: `노드 { ... }` 형식이어야 한다");
                continue;
            };
            let mut na = NodeAttrs {
                node: unresolved(e.name.span.text(src)),
                lin_protocol: None,
                configured_nad: None,
                initial_nad: None,
                product_id: None,
                response_error: None,
                fault_state_signals: Vec::new(),
                p2_min_ms: None,
                st_min_ms: None,
                n_as_timeout_ms: None,
                n_cr_timeout_ms: None,
                configurable_frames: Vec::new(),
                extra: Vec::new(),
                span: Some(e.span),
            };
            for s in &body.stmts {
                self.node_attr_item(&mut na, s);
            }
            self.model.node_attrs.push(na);
        }
    }

    fn node_attr_item(&mut self, na: &mut NodeAttrs, stmt: &Stmt) {
        let src = self.src;
        let Some(e) = entry(stmt) else {
            self.bad(stmt.span, "Node_attributes: 항목 형식이 맞지 않는다");
            return;
        };
        let key = e.name.span.text(src);
        let groups = split_commas(e.rest);
        let opt_uint = |g: &[Elem]| uint(src, g);
        match key {
            "LIN_protocol" => na.lin_protocol = string_lit(src, e.rest),
            "configured_NAD" => na.configured_nad = opt_uint(e.rest),
            "initial_NAD" => na.initial_nad = opt_uint(e.rest),
            "product_id" => {
                let supplier = groups.first().and_then(|g| opt_uint(g));
                let function = groups.get(1).and_then(|g| opt_uint(g));
                if let (Some(supplier), Some(function)) = (supplier, function) {
                    na.product_id = Some(ProductId {
                        supplier,
                        function,
                        variant: groups.get(2).and_then(|g| opt_uint(g)),
                    });
                } else {
                    self.diags.push(Diagnostic::error(
                        codes::EXPECTED_NUMBER,
                        e.span,
                        "product_id: 공급자 ID와 기능 ID가 필요하다",
                    ));
                }
            }
            "response_error" => na.response_error = ident(src, e.rest).map(unresolved),
            "fault_state_signals" => {
                na.fault_state_signals = self
                    .name_list(&groups)
                    .into_iter()
                    .map(unresolved)
                    .collect();
            }
            "P2_min" => na.p2_min_ms = quantity_ms(src, e.rest),
            "ST_min" => na.st_min_ms = quantity_ms(src, e.rest),
            "N_As_timeout" => na.n_as_timeout_ms = quantity_ms(src, e.rest),
            "N_Cr_timeout" => na.n_cr_timeout_ms = quantity_ms(src, e.rest),
            "configurable_frames" => {
                if let [Elem::Block(b)] = e.rest {
                    for s in &b.stmts {
                        let Some(fe) = entry(s) else {
                            self.bad(s.span, "configurable_frames: 프레임 이름이 와야 한다");
                            continue;
                        };
                        na.configurable_frames.push(ConfigurableFrame {
                            frame: unresolved(fe.name.span.text(src)),
                            message_id: if fe.sep == Some('=') {
                                opt_uint(fe.rest)
                            } else {
                                None
                            },
                        });
                    }
                }
            }
            _ => na.extra.push(RawAttr {
                key: key.to_owned(),
                text: stmt.span.text(src).to_owned(),
            }),
        }
    }

    // ---- Schedule_tables -------------------------------------------------------------------

    fn schedule_tables(&mut self, b: &Block) {
        let src = self.src;
        for stmt in &b.stmts {
            let (Some(e), [_, Elem::Block(body)]) = (entry(stmt), stmt.elems.as_slice()) else {
                self.bad(
                    stmt.span,
                    "Schedule_tables: `테이블 { ... }` 형식이어야 한다",
                );
                continue;
            };
            let slots = body.stmts.iter().filter_map(|s| self.slot(s)).collect();
            let name = e.name.span.text(src).to_owned();
            let id = self.model.schedules.push(ScheduleTable {
                name: name.clone(),
                slots,
                span: Some(e.span),
            });
            if !register(&mut self.sym.schedules, &name, id) {
                self.dup(e.span, "스케줄 테이블", &name);
            }
        }
    }

    fn slot(&mut self, stmt: &Stmt) -> Option<Slot> {
        let src = self.src;
        let delay_at = stmt.elems.iter().position(|e| {
            matches!(e, Elem::Token(t) if t.kind == TokenKind::Ident && t.span.text(src) == "delay")
        });
        let Some(di) = delay_at else {
            self.bad(stmt.span, "스케줄 슬롯: `delay`가 없다");
            return None;
        };
        let head = stmt.elems.get(..di).unwrap_or(&[]);
        let tail = stmt.elems.get(di + 1..).unwrap_or(&[]);
        let delay_ms = quantity_ms(src, tail).unwrap_or_else(|| {
            self.diags.push(Diagnostic::error(
                codes::EXPECTED_NUMBER,
                stmt.span,
                "delay: `10 ms` 형식이어야 한다",
            ));
            0.0
        });
        let (name, block) = match head {
            [Elem::Token(n)] if n.kind == TokenKind::Ident => (n.span.text(src), None),
            [Elem::Token(n), Elem::Block(b)] if n.kind == TokenKind::Ident => {
                (n.span.text(src), Some(b))
            }
            _ => {
                self.bad(stmt.span, "스케줄 슬롯: 프레임 이름 또는 명령이어야 한다");
                return None;
            }
        };

        let args = block.map(block_groups).unwrap_or_default();
        let name_at = |i: usize| args.get(i).and_then(|g| ident(src, g)).unwrap_or("");
        let nums = |from: usize| -> Vec<u32> {
            args.iter()
                .skip(from)
                .filter_map(|g| uint(src, g))
                .collect()
        };
        let entry = match (name, block.is_some()) {
            ("AssignNAD", true) => SlotEntry::AssignNad {
                node: unresolved(name_at(0)),
            },
            ("ConditionalChangeNAD", true) => SlotEntry::ConditionalChangeNad { args: nums(0) },
            ("DataDump", true) => SlotEntry::DataDump {
                node: unresolved(name_at(0)),
                data: nums(1),
            },
            ("SaveConfiguration", true) => SlotEntry::SaveConfiguration {
                node: unresolved(name_at(0)),
            },
            ("AssignFrameIdRange", true) => SlotEntry::AssignFrameIdRange {
                node: unresolved(name_at(0)),
                start_index: args.get(1).and_then(|g| uint(src, g)).unwrap_or(0),
                pids: nums(2),
            },
            ("AssignFrameId", true) => SlotEntry::AssignFrameId {
                node: unresolved(name_at(0)),
                frame: unresolved(name_at(1)),
            },
            ("FreeFormat", true) => SlotEntry::FreeFormat { data: nums(0) },
            (_, false) => SlotEntry::Frame(unresolved(name)),
            (_, true) => SlotEntry::Unknown(stmt.span.text(src).to_owned()),
        };
        Some(Slot {
            entry,
            delay_ms,
            span: Some(stmt.span),
        })
    }

    // ---- 인코딩 ----------------------------------------------------------------------------

    fn encodings(&mut self, b: &Block) {
        let src = self.src;
        for stmt in &b.stmts {
            let (Some(e), [_, Elem::Block(body)]) = (entry(stmt), stmt.elems.as_slice()) else {
                self.bad(
                    stmt.span,
                    "Signal_encoding_types: `이름 { ... }` 형식이어야 한다",
                );
                continue;
            };
            let entries = body
                .stmts
                .iter()
                .filter_map(|s| self.encoding_entry(s))
                .collect();
            let name = e.name.span.text(src).to_owned();
            let id = self.model.encodings.push(EncodingType {
                name: name.clone(),
                entries,
                span: Some(e.span),
            });
            if !register(&mut self.sym.encodings, &name, id) {
                self.dup(e.span, "인코딩", &name);
            }
        }
    }

    fn encoding_entry(&mut self, stmt: &Stmt) -> Option<EncodingEntry> {
        let src = self.src;
        let g = split_commas(&stmt.elems);
        let kind = g.first().and_then(|g| ident(src, g));
        let num = |i: usize| g.get(i).and_then(|g| number(src, g));
        match kind {
            Some("logical_value") => {
                let value = g.get(1).and_then(|g| match g {
                    [Elem::Token(t)] if t.kind == TokenKind::Number => parse_u64(t.span.text(src)),
                    _ => None,
                });
                let text = g.get(2).and_then(|g| string_lit(src, g));
                if let (Some(value), Some(text)) = (value, text) {
                    Some(EncodingEntry::Logical { value, text })
                } else {
                    self.bad(
                        stmt.span,
                        "logical_value: `logical_value, 값, \"설명\"` 형식이어야 한다",
                    );
                    None
                }
            }
            Some("physical_value") => {
                let (Some(min), Some(max), Some(scale), Some(offset)) =
                    (num(1), num(2), num(3), num(4))
                else {
                    self.bad(
                        stmt.span,
                        "physical_value: `physical_value, min, max, scale, offset, \"단위\"` 형식이어야 한다",
                    );
                    return None;
                };
                Some(EncodingEntry::Physical {
                    min: min as i64,
                    max: max as i64,
                    scale,
                    offset,
                    unit: g
                        .get(5)
                        .and_then(|g| string_lit(src, g))
                        .unwrap_or_default(),
                })
            }
            Some("bcd_value") => Some(EncodingEntry::Bcd),
            Some("ascii_value") => Some(EncodingEntry::Ascii),
            _ => {
                self.bad(stmt.span, "Signal_encoding_types: 알 수 없는 항목");
                None
            }
        }
    }

    fn representation(&mut self, b: &Block) {
        let src = self.src;
        for stmt in &b.stmts {
            let Some(e) = entry(stmt).filter(|e| e.sep == Some(':')) else {
                self.bad(
                    stmt.span,
                    "Signal_representation: `인코딩: 시그널, ...` 형식이어야 한다",
                );
                continue;
            };
            let enc = e.name.span.text(src);
            for g in split_commas(e.rest) {
                let Some(sig_name) = ident(src, g) else {
                    continue;
                };
                match self.sym.signals.get(sig_name).copied() {
                    Some(sid) => {
                        if let Some(sig) = self.model.signals.get_mut(sid) {
                            sig.encodings.push(unresolved(enc));
                        }
                    }
                    None => self.diags.push(Diagnostic::warning(
                        codes::UNRESOLVED_REPRESENTATION,
                        e.span,
                        format!("Signal_representation: 시그널 `{sig_name}`이 없어 항목을 버렸다"),
                    )),
                }
            }
        }
    }

    // ---- 이름 해석 -------------------------------------------------------------------------

    fn resolve(&mut self) {
        let sym = &self.sym;
        let m = &mut self.model;

        for (_, s) in m.signals.iter_mut() {
            if let Some(p) = &mut s.publisher {
                fix(p, &sym.nodes);
            }
            s.subscribers.iter_mut().for_each(|r| fix(r, &sym.nodes));
            s.encodings.iter_mut().for_each(|r| fix(r, &sym.encodings));
        }

        for (_, f) in m.frames.iter_mut() {
            match &mut f.kind {
                FrameKind::Unconditional {
                    publisher, signals, ..
                } => {
                    fix(publisher, &sym.nodes);
                    signals
                        .iter_mut()
                        .for_each(|fs| fix(&mut fs.signal, &sym.signals));
                }
                FrameKind::Sporadic { associated } => {
                    associated.iter_mut().for_each(|r| fix(r, &sym.frames));
                }
                FrameKind::EventTriggered {
                    collision_table,
                    associated,
                    ..
                } => {
                    if let Some(t) = collision_table {
                        fix(t, &sym.schedules);
                    }
                    associated.iter_mut().for_each(|r| fix(r, &sym.frames));
                }
                FrameKind::Diagnostic { signals, .. } => {
                    signals
                        .iter_mut()
                        .for_each(|fs| fix(&mut fs.signal, &sym.signals));
                }
            }
        }

        for na in &mut m.node_attrs {
            fix(&mut na.node, &sym.nodes);
            if let Some(r) = &mut na.response_error {
                fix(r, &sym.signals);
            }
            na.fault_state_signals
                .iter_mut()
                .for_each(|r| fix(r, &sym.signals));
            na.configurable_frames
                .iter_mut()
                .for_each(|c| fix(&mut c.frame, &sym.frames));
        }

        for (_, sch) in m.schedules.iter_mut() {
            for slot in &mut sch.slots {
                match &mut slot.entry {
                    SlotEntry::Frame(r) => fix(r, &sym.frames),
                    SlotEntry::AssignNad { node }
                    | SlotEntry::DataDump { node, .. }
                    | SlotEntry::SaveConfiguration { node }
                    | SlotEntry::AssignFrameIdRange { node, .. } => fix(node, &sym.nodes),
                    SlotEntry::AssignFrameId { node, frame } => {
                        fix(node, &sym.nodes);
                        fix(frame, &sym.frames);
                    }
                    SlotEntry::ConditionalChangeNad { .. }
                    | SlotEntry::FreeFormat { .. }
                    | SlotEntry::Unknown(_) => {}
                }
            }
        }
    }
}
