//! 구문 트리(`ldf-syntax`)를 NCF 구조체로 바꾼다. 읽을 수 없는 항목은 진단하고 원문(`Item`)으로 남긴다.

use crate::codes;
use crate::{
    Bitrate, DiagnosticInfo, Direction, EncodingRef, EventTriggered, General, Item, Nad,
    NcfEncoding, NcfFile, NcfFrame, NcfNode, NcfSignal,
};
use ldf_model::{EntryError, InitValue, parse_encoding_entry};
use ldf_syntax::{Block, Diagnostic, Elem, File, Span, Stmt, Token, TokenKind};

/// 읽기 결과.
#[derive(Clone, Debug, Default)]
pub struct Parsed {
    /// 읽은 NCF.
    pub file: NcfFile,
    /// 렉서·파서와 NCF 진단.
    pub diagnostics: Vec<Diagnostic>,
}

/// 원문이 NCF인지(첫 토큰이 `node_capability_file`). 가볍게 토큰만 본다.
pub fn is_ncf(src: &str) -> bool {
    ldf_syntax::lex(src)
        .tokens
        .first()
        .is_some_and(|t| t.kind == TokenKind::Ident && t.span.text(src) == "node_capability_file")
}

/// 원문을 NCF로 읽는다. 절대 패닉하지 않는다.
pub fn parse(src: &str) -> Parsed {
    let parsed = ldf_syntax::parse(src);
    let mut p = Reader {
        src,
        diags: parsed.diagnostics,
    };
    let file = p.file(&parsed.file);
    Parsed {
        file,
        diagnostics: p.diags,
    }
}

/// `키 [= 값]` 형태로 본 문장.
struct Entry<'a> {
    key: &'a str,
    sep: Option<char>,
    rest: &'a [Elem],
    span: Span,
}

struct Reader<'a> {
    src: &'a str,
    diags: Vec<Diagnostic>,
}

impl<'a> Reader<'a> {
    fn text(&self, t: &Token) -> &'a str {
        t.span.text(self.src)
    }

    fn is_ident(&self, e: &Elem, word: &str) -> bool {
        matches!(e, Elem::Token(t) if t.kind == TokenKind::Ident && self.text(t) == word)
    }

    fn ident(&self, e: &Elem) -> Option<&'a str> {
        match e {
            Elem::Token(t) if t.kind == TokenKind::Ident => Some(self.text(t)),
            _ => None,
        }
    }

    fn punct(e: &Elem) -> Option<char> {
        match e {
            Elem::Token(Token {
                kind: TokenKind::Punct(c),
                ..
            }) => Some(*c),
            _ => None,
        }
    }

    /// 요소들의 원문(공백은 하나로 줄인다).
    fn raw(&self, elems: &[Elem]) -> String {
        let (Some(first), Some(last)) = (elems.first(), elems.last()) else {
            return String::new();
        };
        first
            .span()
            .join(last.span())
            .text(self.src)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn entry(&self, stmt: &'a Stmt) -> Option<Entry<'a>> {
        let key = self.ident(stmt.elems.first()?)?;
        let (sep, rest) = match stmt.elems.get(1).and_then(Self::punct) {
            Some(c @ ('=' | ':')) => (Some(c), stmt.elems.get(2..).unwrap_or(&[])),
            _ => (None, stmt.elems.get(1..).unwrap_or(&[])),
        };
        Some(Entry {
            key,
            sep,
            rest,
            span: stmt.span,
        })
    }

    /// 해석하지 못한 문장을 원문 항목으로 남긴다.
    fn item(&self, stmt: &'a Stmt) -> Item {
        match self.entry(stmt) {
            Some(e) => Item {
                key: e.key.to_owned(),
                value: self.raw(e.rest),
                span: Some(stmt.span),
            },
            None => Item {
                key: String::new(),
                value: self.raw(&stmt.elems),
                span: Some(stmt.span),
            },
        }
    }

    fn unknown(&mut self, stmt: &'a Stmt, what: &str) -> Item {
        let item = self.item(stmt);
        let key = if item.key.is_empty() {
            &item.value
        } else {
            &item.key
        };
        self.diags.push(Diagnostic::info(
            codes::UNKNOWN_ITEM,
            stmt.span,
            format!("{what}: 알 수 없는 항목 `{key}`(원문으로 보관)"),
        ));
        item
    }

    fn bad_value(&mut self, e: &Entry<'a>) {
        self.diags.push(Diagnostic::warning(
            codes::BAD_VALUE,
            e.span,
            format!("{}: 값을 읽을 수 없다", e.key),
        ));
    }

    fn bad_form(&mut self, span: Span, what: &str, form: &str) {
        self.diags.push(Diagnostic::warning(
            codes::BAD_STRUCTURE,
            span,
            format!("{what}: {form} 형식이어야 한다"),
        ));
    }

    // ---- 값 읽기 -----------------------------------------------------------------------------

    fn number(&self, t: &Token) -> Option<f64> {
        if t.kind != TokenKind::Number {
            return None;
        }
        let text = self.text(t);
        let (neg, body) = match text.strip_prefix('-') {
            Some(r) => (true, r),
            None => (false, text.strip_prefix('+').unwrap_or(text)),
        };
        let v = match body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
            Some(h) => u64::from_str_radix(h, 16).ok()? as f64,
            None => body.parse::<f64>().ok().filter(|v| v.is_finite())?,
        };
        Some(if neg { -v } else { v })
    }

    fn uint_token(&self, t: &Token) -> Option<u32> {
        let v = self.number(t)?;
        (v >= 0.0 && v.fract() == 0.0 && v <= f64::from(u32::MAX)).then_some(v as u32)
    }

    fn uint(&self, g: &[Elem]) -> Option<u32> {
        match g {
            [Elem::Token(t)] => self.uint_token(t),
            _ => None,
        }
    }

    fn string(&self, g: &[Elem]) -> Option<String> {
        match g {
            [Elem::Token(t)] if t.kind == TokenKind::Str => {
                Some(self.text(t).trim_matches('"').to_owned())
            }
            [Elem::Token(t)] if t.kind == TokenKind::Ident => Some(self.text(t).to_owned()),
            _ => None,
        }
    }

    /// `[숫자]` 또는 `[숫자, 단위]`.
    fn quantity(&self, g: &[Elem]) -> Option<(f64, Option<&'a str>)> {
        match g {
            [Elem::Token(n)] => Some((self.number(n)?, None)),
            [Elem::Token(n), Elem::Token(u)] if u.kind == TokenKind::Ident => {
                Some((self.number(n)?, Some(self.text(u))))
            }
            _ => None,
        }
    }

    /// 시간 값(ms). 단위가 없으면 ms다.
    fn millis(&self, g: &[Elem]) -> Option<f64> {
        let (v, unit) = self.quantity(g)?;
        match unit {
            None | Some("ms") => Some(v),
            Some("s") => Some(v * 1000.0),
            Some("us") => Some(v / 1000.0),
            _ => None,
        }
    }

    /// 비트레이트 값(bit/s). 단위가 없으면 1000 미만은 kbps(LIN 2.2A의 `19.2 kbps` 표기), 그 이상은 bit/s로 읽는다(`19200`).
    fn bps(&self, g: &[Elem], default_unit: Option<&str>) -> Option<f64> {
        let (v, unit) = self.quantity(g)?;
        match unit.or(default_unit) {
            Some("kbps") => Some(v * 1000.0),
            Some("bps") => Some(v),
            Some("mbps") => Some(v * 1_000_000.0),
            None if v < 1000.0 => Some(v * 1000.0),
            None => Some(v),
            _ => None,
        }
    }

    /// 쉼표로 나눈 그룹.
    fn commas(elems: &[Elem]) -> Vec<&[Elem]> {
        if elems.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut start = 0;
        for (i, e) in elems.iter().enumerate() {
            if Self::punct(e) == Some(',') {
                out.push(elems.get(start..i).unwrap_or(&[]));
                start = i + 1;
            }
        }
        out.push(elems.get(start..).unwrap_or(&[]));
        out
    }

    /// 블록 안의 모든 정수 토큰.
    fn numbers_in(&self, b: &Block) -> Option<Vec<u32>> {
        let mut out = Vec::new();
        for s in &b.stmts {
            for e in &s.elems {
                match e {
                    Elem::Token(t) if t.kind == TokenKind::Number => out.push(self.uint_token(t)?),
                    Elem::Token(Token {
                        kind: TokenKind::Punct(',' | ';'),
                        ..
                    }) => {}
                    _ => return None,
                }
            }
        }
        Some(out)
    }

    // ---- 파일·노드 ---------------------------------------------------------------------------

    fn file(&mut self, f: &'a File) -> NcfFile {
        let mut out = NcfFile::default();
        let mut marker = false;
        for stmt in &f.stmts {
            let elems = stmt.elems.as_slice();
            match elems {
                [one] if self.is_ident(one, "node_capability_file") => marker = true,
                [Elem::Token(n), Elem::Token(name), Elem::Block(b)]
                    if n.kind == TokenKind::Ident
                        && self.text(n) == "node"
                        && name.kind == TokenKind::Ident =>
                {
                    let node = self.node(self.text(name), b, stmt.span);
                    out.nodes.push(node);
                }
                _ => match self.entry(stmt) {
                    Some(e) if e.key == "LIN_language_version" && e.sep == Some('=') => {
                        match self.string(e.rest) {
                            Some(v) => out.language_version = Some(v),
                            None => self.bad_value(&e),
                        }
                    }
                    _ => {
                        self.unknown(stmt, "NCF");
                    }
                },
            }
        }
        if !marker {
            let at = f.stmts.first().map_or(Span::default(), |s| s.span);
            self.diags.push(Diagnostic::warning(
                codes::NOT_NCF,
                at,
                "`node_capability_file;`로 시작하는 NCF가 아니다",
            ));
        }
        if out.nodes.is_empty() {
            self.diags.push(Diagnostic::warning(
                codes::NO_NODE,
                Span::default(),
                "`node` 블록이 없다",
            ));
        }
        out
    }

    fn node(&mut self, name: &str, b: &'a Block, span: Span) -> NcfNode {
        let mut n = NcfNode {
            name: name.to_owned(),
            span: Some(span),
            ..NcfNode::default()
        };
        for stmt in &b.stmts {
            let section = match stmt.elems.as_slice() {
                [k, Elem::Block(body)] => self.ident(k).map(|k| (k, body)),
                _ => None,
            };
            match section {
                Some(("general", body)) => self.general(body, &mut n.general),
                Some(("diagnostic", body)) => self.diagnostic(body, &mut n.diagnostic),
                Some(("frames", body)) => self.frames(body, &mut n.frames),
                Some(("encoding", body)) => self.encodings(body, &mut n.encodings),
                Some(("status_management", body)) => {
                    n.status_management
                        .extend(body.stmts.iter().map(|s| self.item(s)));
                }
                Some(("free_text", body)) => n.free_text = self.free_text(body),
                _ => {
                    let item = self.unknown(stmt, "node");
                    n.extra.push(item);
                }
            }
        }
        n
    }

    fn free_text(&self, b: &Block) -> Option<String> {
        let lines: Vec<String> = b
            .stmts
            .iter()
            .flat_map(|s| &s.elems)
            .filter_map(|e| match e {
                Elem::Token(t) if t.kind == TokenKind::Str => {
                    Some(self.text(t).trim_matches('"').to_owned())
                }
                _ => None,
            })
            .collect();
        (!lines.is_empty()).then(|| lines.join("\n"))
    }

    // ---- general ----------------------------------------------------------------------------

    fn general(&mut self, b: &'a Block, g: &mut General) {
        for stmt in &b.stmts {
            let Some(e) = self.entry(stmt).filter(|e| e.sep == Some('=')) else {
                let item = self.unknown(stmt, "general");
                g.extra.push(item);
                continue;
            };
            let ok = match e.key {
                "LIN_protocol_version" => self.string(e.rest).map(|v| g.protocol_version = Some(v)),
                "sends_wake_up_signal" => self
                    .string(e.rest)
                    .map(|v| g.sends_wake_up_signal = Some(v)),
                "supplier" => self.uint(e.rest).map(|v| g.supplier = Some(v)),
                "function" => self.uint(e.rest).map(|v| g.function = Some(v)),
                "variant" => self.uint(e.rest).map(|v| g.variant = Some(v)),
                "bitrate" => self.bitrate(e.rest).map(|v| g.bitrate = Some(v)),
                _ => {
                    let item = self.unknown(stmt, "general");
                    g.extra.push(item);
                    continue;
                }
            };
            if ok.is_none() {
                self.bad_value(&e);
                g.extra.push(self.item(stmt));
            }
        }
    }

    /// `19200`, `19.2 kbps`, `automatic [min A] [max B]`, `select { a, b }`.
    fn bitrate(&self, rest: &[Elem]) -> Option<Bitrate> {
        match rest {
            [kw, tail @ ..] if self.is_ident(kw, "automatic") => {
                let (mut min_bps, mut max_bps) = (None, None);
                let mut i = 0;
                while i < tail.len() {
                    let key = self.ident(tail.get(i)?)?;
                    let start = i + 1;
                    let mut end = start + 1;
                    if tail
                        .get(end)
                        .and_then(|e| self.ident(e))
                        .is_some_and(|w| w != "min" && w != "max")
                    {
                        end += 1;
                    }
                    let v = self.bps(tail.get(start..end)?, None)?;
                    match key {
                        "min" => min_bps = Some(v),
                        "max" => max_bps = Some(v),
                        _ => return None,
                    }
                    i = end;
                }
                Some(Bitrate::Automatic { min_bps, max_bps })
            }
            [kw, Elem::Block(b)] if self.is_ident(kw, "select") => {
                let mut bps = Vec::new();
                for s in &b.stmts {
                    for g in Self::commas(&s.elems) {
                        bps.push(self.bps(g, None)?);
                    }
                }
                (!bps.is_empty()).then_some(Bitrate::Select { bps })
            }
            _ => Some(Bitrate::Fixed {
                bps: self.bps(rest, None)?,
            }),
        }
    }

    // ---- diagnostic -------------------------------------------------------------------------

    fn diagnostic(&mut self, b: &'a Block, d: &mut DiagnosticInfo) {
        for stmt in &b.stmts {
            let Some(e) = self.entry(stmt) else {
                let item = self.unknown(stmt, "diagnostic");
                d.extra.push(item);
                continue;
            };
            let ok = match (e.key, e.sep) {
                ("NAD", Some('=')) => self.nad(e.rest).map(|v| d.nad = Some(v)),
                ("diagnostic_class", Some('=')) => {
                    self.uint(e.rest).map(|v| d.diagnostic_class = Some(v))
                }
                ("P2_min", Some('=')) => self.millis(e.rest).map(|v| d.p2_min_ms = Some(v)),
                ("ST_min", Some('=')) => self.millis(e.rest).map(|v| d.st_min_ms = Some(v)),
                ("N_As_timeout", Some('=')) => {
                    self.millis(e.rest).map(|v| d.n_as_timeout_ms = Some(v))
                }
                ("N_Cr_timeout", Some('=')) => {
                    self.millis(e.rest).map(|v| d.n_cr_timeout_ms = Some(v))
                }
                ("max_message_length", Some('=')) => {
                    self.uint(e.rest).map(|v| d.max_message_length = Some(v))
                }
                ("support_sid", None) => match e.rest {
                    [Elem::Block(sids)] => self.numbers_in(sids).map(|v| d.support_sid.extend(v)),
                    _ => None,
                },
                _ => {
                    let item = self.unknown(stmt, "diagnostic");
                    d.extra.push(item);
                    continue;
                }
            };
            if ok.is_none() {
                self.bad_value(&e);
                d.extra.push(self.item(stmt));
            }
        }
    }

    /// `NAD = 1;`, `NAD = 1, 2, 3;`, `NAD = 1 to 5;`.
    fn nad(&self, rest: &[Elem]) -> Option<Nad> {
        if let [Elem::Token(a), kw, Elem::Token(b)] = rest
            && self.is_ident(kw, "to")
        {
            return Some(Nad::Range {
                from: self.uint_token(a)?,
                to: self.uint_token(b)?,
            });
        }
        let list: Option<Vec<u32>> = Self::commas(rest)
            .into_iter()
            .map(|g| self.uint(g))
            .collect();
        list.filter(|l| !l.is_empty()).map(Nad::List)
    }

    // ---- frames -----------------------------------------------------------------------------

    fn frames(&mut self, b: &'a Block, out: &mut Vec<NcfFrame>) {
        for stmt in &b.stmts {
            let [d, Elem::Token(name), Elem::Block(body)] = stmt.elems.as_slice() else {
                self.bad_form(
                    stmt.span,
                    "frames",
                    "`publish 이름` 또는 `subscribe 이름` 블록",
                );
                continue;
            };
            let direction = match self.ident(d) {
                Some("publish") => Direction::Publish,
                Some("subscribe") => Direction::Subscribe,
                _ => {
                    self.bad_form(
                        stmt.span,
                        "frames",
                        "`publish 이름` 또는 `subscribe 이름` 블록",
                    );
                    continue;
                }
            };
            out.push(self.frame(direction, self.text(name), body, stmt.span));
        }
    }

    fn frame(&mut self, direction: Direction, name: &str, b: &'a Block, span: Span) -> NcfFrame {
        let mut f = NcfFrame {
            direction,
            name: name.to_owned(),
            length: None,
            message_id: None,
            min_period_ms: None,
            max_period_ms: None,
            event_triggered: None,
            signals: Vec::new(),
            extra: Vec::new(),
            span: Some(span),
        };
        for stmt in &b.stmts {
            let Some(e) = self.entry(stmt) else {
                let item = self.unknown(stmt, "frames");
                f.extra.push(item);
                continue;
            };
            let ok = match (e.key, e.sep) {
                ("length", Some('=')) => self.uint(e.rest).map(|v| f.length = Some(v)),
                ("message_ID", Some('=')) => self.uint(e.rest).map(|v| f.message_id = Some(v)),
                ("min_period", Some('=')) => self.millis(e.rest).map(|v| f.min_period_ms = Some(v)),
                ("max_period", Some('=')) => self.millis(e.rest).map(|v| f.max_period_ms = Some(v)),
                ("event_triggered_frame", Some('=')) => self
                    .string(e.rest)
                    .map(|v| f.event_triggered = Some(EventTriggered::Frame(v))),
                ("event_triggered_message_ID", Some('=')) => self
                    .uint(e.rest)
                    .map(|v| f.event_triggered = Some(EventTriggered::MessageId(v))),
                ("signals", None) => {
                    if let [Elem::Block(sigs)] = e.rest {
                        self.signals(sigs, &mut f.signals);
                        Some(())
                    } else {
                        None
                    }
                }
                _ => {
                    let item = self.unknown(stmt, "frames");
                    f.extra.push(item);
                    continue;
                }
            };
            if ok.is_none() {
                self.bad_value(&e);
                f.extra.push(self.item(stmt));
            }
        }
        f
    }

    fn signals(&mut self, b: &'a Block, out: &mut Vec<NcfSignal>) {
        for stmt in &b.stmts {
            let [Elem::Token(name), Elem::Block(body)] = stmt.elems.as_slice() else {
                self.bad_form(stmt.span, "signals", "`시그널 이름` 블록");
                continue;
            };
            if name.kind != TokenKind::Ident {
                self.bad_form(stmt.span, "signals", "`시그널 이름` 블록");
                continue;
            }
            out.push(self.signal(self.text(name), body, stmt.span));
        }
    }

    fn signal(&mut self, name: &str, b: &'a Block, span: Span) -> NcfSignal {
        let mut s = NcfSignal {
            name: name.to_owned(),
            init: None,
            size: None,
            offset: None,
            encoding: None,
            extra: Vec::new(),
            span: Some(span),
        };
        for stmt in &b.stmts {
            // `인코딩이름;`(참조)와 `encoding 이름 블록;`(그 자리에서 정의)
            match stmt.elems.as_slice() {
                [one] if self.ident(one).is_some() => {
                    s.encoding = self.ident(one).map(|n| EncodingRef::Named(n.to_owned()));
                    continue;
                }
                [kw, Elem::Token(en), Elem::Block(body)]
                    if self.is_ident(kw, "encoding") && en.kind == TokenKind::Ident =>
                {
                    let enc = self.encoding(self.text(en), body, stmt.span);
                    s.encoding = Some(EncodingRef::Inline(enc));
                    continue;
                }
                _ => {}
            }
            let Some(e) = self.entry(stmt).filter(|e| e.sep == Some('=')) else {
                let item = self.unknown(stmt, "signals");
                s.extra.push(item);
                continue;
            };
            let ok = match e.key {
                "init_value" => self.init(e.rest).map(|v| s.init = Some(v)),
                "size" => self.uint(e.rest).map(|v| s.size = Some(v)),
                "offset" => self.uint(e.rest).map(|v| s.offset = Some(v)),
                _ => {
                    let item = self.unknown(stmt, "signals");
                    s.extra.push(item);
                    continue;
                }
            };
            if ok.is_none() {
                self.bad_value(&e);
                s.extra.push(self.item(stmt));
            }
        }
        s
    }

    /// 숫자 하나, 또는 `{ 바이트, ... }`.
    fn init(&self, rest: &[Elem]) -> Option<InitValue> {
        match rest {
            [Elem::Token(t)] => {
                let v = self.number(t)?;
                (v >= 0.0 && v.fract() == 0.0 && v <= u64::MAX as f64)
                    .then_some(InitValue::Scalar(v as u64))
            }
            [Elem::Block(b)] => {
                let bytes: Option<Vec<u8>> = self
                    .numbers_in(b)?
                    .into_iter()
                    .map(|v| u8::try_from(v).ok())
                    .collect();
                bytes.filter(|v| !v.is_empty()).map(InitValue::Array)
            }
            _ => None,
        }
    }

    // ---- encoding ---------------------------------------------------------------------------

    fn encodings(&mut self, b: &'a Block, out: &mut Vec<NcfEncoding>) {
        for stmt in &b.stmts {
            let [Elem::Token(name), Elem::Block(body)] = stmt.elems.as_slice() else {
                self.bad_form(stmt.span, "encoding", "`인코딩 이름` 블록");
                continue;
            };
            if name.kind != TokenKind::Ident {
                self.bad_form(stmt.span, "encoding", "`인코딩 이름` 블록");
                continue;
            }
            out.push(self.encoding(self.text(name), body, stmt.span));
        }
    }

    fn encoding(&mut self, name: &str, b: &'a Block, span: Span) -> NcfEncoding {
        let mut entries = Vec::new();
        for stmt in &b.stmts {
            match parse_encoding_entry(self.src, stmt) {
                Ok(e) => entries.push(e),
                Err(err) => {
                    let msg = match err {
                        EntryError::Logical => {
                            format!(
                                "인코딩 `{name}`: `logical_value, 값, \"설명\"` 형식이어야 한다"
                            )
                        }
                        EntryError::Physical => format!(
                            "인코딩 `{name}`: `physical_value, min, max, scale, offset, \"단위\"` 형식이어야 한다"
                        ),
                        EntryError::Unknown => format!("인코딩 `{name}`: 알 수 없는 항목"),
                    };
                    self.diags
                        .push(Diagnostic::warning(codes::BAD_STRUCTURE, stmt.span, msg));
                }
            }
        }
        NcfEncoding {
            name: name.to_owned(),
            entries,
            span: Some(span),
        }
    }
}
