//! 구조 파서: 토큰 열을 문장·블록 트리로 만든다. 오류가 나도 끝까지 읽는다(FR-IMP-04).
//!
//! 문장 종료 규칙:
//! - `;` 를 만나면 종료(소비).
//! - 둘러싼 `}` 또는 파일 끝을 만나면 종료(소비하지 않음).
//! - 블록이 끝난 뒤에는, 다음 토큰이 `;` 면 종료, `,` 또는 `delay` 면 같은 문장이 계속되고,
//!   그 외에는 블록에서 문장이 끝난다. (`Frame_1: 0, M, 2 { ... }` 와
//!   `AssignNAD { N } delay 10 ms ;` 를 모두 다루는 LDF 고유 규칙)

use crate::{Block, Diagnostic, Elem, File, Span, Stmt, Token, TokenKind, codes, lexer};

/// 중첩 한도. 넘으면 진단하고 건너뛴다(스택 오버플로 방지, NFR-02).
const MAX_DEPTH: usize = 64;

/// LDF 최상위 섹션 이름. `}` 가 빠진 파일에서 다음 섹션이 앞 섹션 안으로 삼켜지지 않도록,
/// 첫 단계 블록 안에서 `이름 {` 로 나타나면 앞 블록이 닫힌 것으로 복구한다.
const SECTIONS: &[&str] = &[
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

/// 파싱 결과.
#[derive(Clone, Debug, Default)]
pub struct Parsed {
    /// 구문 트리.
    pub file: File,
    /// 렉서·파서 진단.
    pub diagnostics: Vec<Diagnostic>,
}

/// 원문을 구문 트리로 만든다. 절대 패닉하지 않는다.
pub fn parse(src: &str) -> Parsed {
    let lexed = lexer::lex(src);
    let mut p = Parser {
        src,
        toks: &lexed.tokens,
        pos: 0,
        diags: lexed.diagnostics,
        depth: 0,
    };
    let stmts = p.stmts(true);
    Parsed {
        file: File {
            stmts,
            comments: lexed.comments,
        },
        diagnostics: p.diags,
    }
}

struct Parser<'a> {
    src: &'a str,
    toks: &'a [Token],
    pos: usize,
    diags: Vec<Diagnostic>,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<Token> {
        self.toks.get(self.pos).copied()
    }

    fn peek_punct(&self) -> Option<char> {
        match self.peek() {
            Some(Token {
                kind: TokenKind::Punct(c),
                ..
            }) => Some(c),
            _ => None,
        }
    }

    fn is_delay(&self) -> bool {
        matches!(self.peek(), Some(t) if t.kind == TokenKind::Ident && t.span.text(self.src) == "delay")
    }

    /// 현재 위치가 최상위 섹션의 시작(`Signals {` 등)인지.
    fn at_section_start(&self) -> bool {
        match (self.peek(), self.toks.get(self.pos + 1)) {
            (Some(a), Some(b)) if a.kind == TokenKind::Ident && b.kind == TokenKind::Punct('{') => {
                SECTIONS.contains(&a.span.text(self.src))
            }
            _ => false,
        }
    }

    fn stmts(&mut self, top: bool) -> Vec<Stmt> {
        let mut out = Vec::new();
        while let Some(t) = self.peek() {
            match t.kind {
                TokenKind::Punct('}') if top => {
                    self.diags.push(Diagnostic::error(
                        codes::UNEXPECTED_CLOSE_BRACE,
                        t.span,
                        "짝이 없는 '}'",
                    ));
                    self.pos += 1;
                }
                TokenKind::Punct('}') => break,
                TokenKind::Punct(';') => self.pos += 1, // 빈 문장
                _ if !top && self.depth == 1 && self.at_section_start() => break,
                _ => out.push(self.stmt()),
            }
        }
        out
    }

    fn stmt(&mut self) -> Stmt {
        let mut elems: Vec<Elem> = Vec::new();
        let mut span = self.peek().map_or(Span::default(), |t| t.span);

        while let Some(t) = self.peek() {
            span = span.join(t.span);
            match t.kind {
                TokenKind::Punct(';') => {
                    self.pos += 1;
                    break;
                }
                TokenKind::Punct('}') => break,
                TokenKind::Punct('{') => {
                    let block = self.block();
                    span = span.join(block.span);
                    elems.push(Elem::Block(block));
                    match self.peek_punct() {
                        Some(';') => {
                            if let Some(semi) = self.peek() {
                                span = span.join(semi.span);
                            }
                            self.pos += 1;
                            break;
                        }
                        Some(',') => {}
                        _ if self.is_delay() => {}
                        _ => break,
                    }
                }
                _ => {
                    elems.push(Elem::Token(t));
                    self.pos += 1;
                }
            }
        }
        Stmt { elems, span }
    }

    /// 현재 토큰이 `{` 일 때 호출한다.
    fn block(&mut self) -> Block {
        let open = self.peek().map_or(Span::default(), |t| t.span);
        self.pos += 1;

        if self.depth >= MAX_DEPTH {
            self.diags.push(Diagnostic::error(
                codes::NESTING_TOO_DEEP,
                open,
                format!("블록 중첩이 {MAX_DEPTH}단계를 넘는다"),
            ));
            let end = self.skip_balanced();
            return Block {
                stmts: Vec::new(),
                span: open.join(end),
            };
        }

        self.depth += 1;
        let stmts = self.stmts(false);
        self.depth -= 1;

        let mut span = open;
        if let Some(last) = stmts.last() {
            span = span.join(last.span);
        }
        match self.peek() {
            Some(t) if t.kind == TokenKind::Punct('}') => {
                span = span.join(t.span);
                self.pos += 1;
            }
            _ => self.diags.push(Diagnostic::error(
                codes::UNCLOSED_BLOCK,
                open,
                "닫히지 않은 '{'",
            )),
        }
        Block { stmts, span }
    }

    /// 이미 `{` 를 소비한 상태에서 짝이 맞는 `}` 까지 반복문으로 건너뛴다.
    fn skip_balanced(&mut self) -> Span {
        let mut level = 1usize;
        let mut last = self
            .toks
            .get(self.pos.saturating_sub(1))
            .map_or(Span::default(), |t| t.span);
        while let Some(t) = self.peek() {
            self.pos += 1;
            last = t.span;
            match t.kind {
                TokenKind::Punct('{') => level += 1,
                TokenKind::Punct('}') => {
                    level -= 1;
                    if level == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(src: &str, s: &Stmt) -> Vec<String> {
        s.elems
            .iter()
            .map(|e| match e {
                Elem::Token(t) => t.span.text(src).to_owned(),
                Elem::Block(_) => "{}".to_owned(),
            })
            .collect()
    }

    #[test]
    fn parses_header_and_section() {
        let src = "LIN_description_file;\nLIN_speed = 19.2 kbps;\nNodes { Master: M, 1 ms, 0 ms; Slaves: A, B; }";
        let p = parse(src);
        assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
        assert_eq!(p.file.stmts.len(), 3);
        assert_eq!(
            texts(src, &p.file.stmts[1]),
            ["LIN_speed", "=", "19.2", "kbps"]
        );
        let Elem::Block(b) = &p.file.stmts[2].elems[1] else {
            panic!("block expected")
        };
        assert_eq!(b.stmts.len(), 2);
    }

    #[test]
    fn block_terminated_statement_without_semicolon() {
        let src = "Frames { F1: 0, M, 2 { S, 0; } F2: 1, M, 1 { T, 0; } }";
        let p = parse(src);
        assert!(p.diagnostics.is_empty());
        let Elem::Block(frames) = &p.file.stmts[0].elems[1] else {
            panic!()
        };
        assert_eq!(frames.stmts.len(), 2);
        assert_eq!(
            texts(src, &frames.stmts[0]),
            ["F1", ":", "0", ",", "M", ",", "2", "{}"]
        );
    }

    #[test]
    fn inline_array_and_delay_continue_the_statement() {
        let src = "Signals { P: 32, {0, 0, 0, 0}, A, B; }\nSchedule_tables { T { AssignNAD { N } delay 10 ms; F delay 5 ms; } }";
        let p = parse(src);
        assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
        let Elem::Block(sig) = &p.file.stmts[0].elems[1] else {
            panic!()
        };
        assert_eq!(sig.stmts.len(), 1);
        let Elem::Block(sched) = &p.file.stmts[1].elems[1] else {
            panic!()
        };
        let Elem::Block(table) = &sched.stmts[0].elems[1] else {
            panic!()
        };
        assert_eq!(table.stmts.len(), 2);
        assert_eq!(
            texts(src, &table.stmts[0]),
            ["AssignNAD", "{}", "delay", "10", "ms"]
        );
    }

    #[test]
    fn unclosed_block_and_stray_brace_are_recovered() {
        let p = parse("A { B: 1;");
        assert!(
            p.diagnostics
                .iter()
                .any(|d| d.code == codes::UNCLOSED_BLOCK)
        );
        assert_eq!(p.file.stmts.len(), 1);

        let p = parse("} A;");
        assert!(
            p.diagnostics
                .iter()
                .any(|d| d.code == codes::UNEXPECTED_CLOSE_BRACE)
        );
        assert_eq!(p.file.stmts.len(), 1);
    }

    #[test]
    fn a_broken_section_does_not_swallow_the_next_one() {
        let src = "Nodes { Master: M, 1 ms ;\nSignals { S: 1, 0, M, A; }";
        let p = parse(src);
        // 첫 블록이 닫히지 않았다는 진단 하나, 그러나 Signals는 최상위 섹션으로 살아남는다
        assert_eq!(
            p.diagnostics
                .iter()
                .filter(|d| d.code == codes::UNCLOSED_BLOCK)
                .count(),
            1
        );
        assert_eq!(p.file.stmts.len(), 2);
        assert_eq!(p.file.stmts[1].elems[0].span().text(src), "Signals");
    }

    #[test]
    fn section_names_deeper_than_first_level_are_ordinary_statements() {
        let src = "Node_attributes { N { Signals { x; } } }";
        let p = parse(src);
        assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
        assert_eq!(p.file.stmts.len(), 1);
    }

    #[test]
    fn deep_nesting_does_not_overflow_the_stack() {
        let src = "{".repeat(100_000);
        let p = parse(&src);
        assert!(
            p.diagnostics
                .iter()
                .any(|d| d.code == codes::NESTING_TOO_DEEP)
        );

        let src = format!("{}{}", "A {".repeat(5_000), "}".repeat(5_000));
        let p = parse(&src);
        assert!(
            p.diagnostics
                .iter()
                .any(|d| d.code == codes::NESTING_TOO_DEEP)
        );
    }

    #[test]
    fn spans_cover_statement_text() {
        let src = "LIN_speed = 19.2 kbps;";
        let p = parse(src);
        assert_eq!(p.file.stmts[0].span.text(src), src);
    }

    #[test]
    fn empty_and_garbage_inputs() {
        assert!(parse("").file.stmts.is_empty());
        let _ = parse("@@@ ;;; }}} {{{ \"");
    }
}
