//! 렉서. 토큰 하나하나는 `winnow` 결합자로 인식하고, 위치는 `LocatingSlice`로 얻는다.
//!
//! 주석은 버리지 않고 `comments`에 모은다(왕복 출력용, FR-EXP-03).
//! 어떤 입력에도 패닉하지 않는다: 알 수 없는 문자는 `Unknown` 토큰과 진단이 된다.

use crate::{Diagnostic, Span, codes};
use serde::Serialize;
use std::ops::Range;
use winnow::ascii::{digit1, hex_digit1};
use winnow::combinator::{alt, opt};
use winnow::error::ModalResult;
use winnow::prelude::*;
use winnow::stream::{LocatingSlice, Stream};
use winnow::token::{any, one_of, take_till, take_until, take_while};

type Input<'i> = LocatingSlice<&'i str>;

/// 토큰 종류. 텍스트는 `Span`으로 원문에서 읽는다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum TokenKind {
    /// 식별자 (`Signal_11`, `ms`, `delay` 등 단위·키워드 포함).
    Ident,
    /// 숫자 (`19.2`, `0x3c`, `-5`).
    Number,
    /// 따옴표 문자열 (따옴표 포함).
    Str,
    /// 한 글자 기호 (`{ } ; : , = ( ) [ ] %`).
    Punct(char),
    /// 알 수 없는 문자.
    Unknown,
}

/// 토큰.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Token {
    /// 종류.
    pub kind: TokenKind,
    /// 원문 위치.
    pub span: Span,
}

/// 주석.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Comment {
    /// 원문 위치(`//`, `/* */` 포함).
    pub span: Span,
    /// `/* */` 형식이면 true.
    pub block: bool,
}

/// 렉서 결과.
#[derive(Clone, Debug, Default)]
pub struct Lexed {
    /// 토큰(주석·공백 제외).
    pub tokens: Vec<Token>,
    /// 주석.
    pub comments: Vec<Comment>,
    /// 진단.
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Copy)]
enum Kind {
    Line,
    Block,
    Str,
    Number,
    Ident,
    Punct(char),
}

fn line_comment(i: &mut Input<'_>) -> ModalResult<()> {
    ("//", take_till(0.., ['\n', '\r'])).void().parse_next(i)
}

fn block_comment(i: &mut Input<'_>) -> ModalResult<()> {
    ("/*", take_until(0.., "*/"), "*/").void().parse_next(i)
}

fn string(i: &mut Input<'_>) -> ModalResult<()> {
    ('"', take_till(0.., '"'), '"').void().parse_next(i)
}

fn hex(i: &mut Input<'_>) -> ModalResult<()> {
    (alt(("0x", "0X")), hex_digit1).void().parse_next(i)
}

fn decimal(i: &mut Input<'_>) -> ModalResult<()> {
    (
        digit1,
        opt(('.', digit1)),
        opt((one_of(['e', 'E']), opt(one_of(['+', '-'])), digit1)),
    )
        .void()
        .parse_next(i)
}

fn number(i: &mut Input<'_>) -> ModalResult<()> {
    (opt(one_of(['+', '-'])), alt((hex, decimal)))
        .void()
        .parse_next(i)
}

fn ident(i: &mut Input<'_>) -> ModalResult<()> {
    (
        one_of(('a'..='z', 'A'..='Z', '_')),
        take_while(0.., ('a'..='z', 'A'..='Z', '0'..='9', '_')),
    )
        .void()
        .parse_next(i)
}

fn punct(i: &mut Input<'_>) -> ModalResult<char> {
    one_of(['{', '}', ';', ':', ',', '=', '(', ')', '[', ']', '%']).parse_next(i)
}

fn is_ws(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// 원문을 토큰으로 나눈다.
pub fn lex(src: &str) -> Lexed {
    let mut input = Input::new(src);
    let mut out = Lexed::default();

    loop {
        let skipped: ModalResult<&str> = take_while(0.., is_ws).parse_next(&mut input);
        drop(skipped);
        if input.eof_offset() == 0 {
            break;
        }

        let parsed: ModalResult<(Kind, Range<usize>)> = alt((
            line_comment.value(Kind::Line),
            block_comment.value(Kind::Block),
            string.value(Kind::Str),
            number.value(Kind::Number),
            ident.value(Kind::Ident),
            punct.map(Kind::Punct),
        ))
        .with_span()
        .parse_next(&mut input);

        match parsed {
            Ok((kind, r)) => {
                let span = Span::new(r.start, r.end);
                match kind {
                    Kind::Line => out.comments.push(Comment { span, block: false }),
                    Kind::Block => out.comments.push(Comment { span, block: true }),
                    Kind::Str => out.tokens.push(Token {
                        kind: TokenKind::Str,
                        span,
                    }),
                    Kind::Number => out.tokens.push(Token {
                        kind: TokenKind::Number,
                        span,
                    }),
                    Kind::Ident => out.tokens.push(Token {
                        kind: TokenKind::Ident,
                        span,
                    }),
                    Kind::Punct(c) => out.tokens.push(Token {
                        kind: TokenKind::Punct(c),
                        span,
                    }),
                }
            }
            Err(_) => recover(&mut input, &mut out),
        }
    }
    out
}

/// 어떤 토큰으로도 읽히지 않았을 때: 닫히지 않은 문자열·주석이면 끝까지 먹고,
/// 아니면 문자 하나를 `Unknown`으로 넘긴다. 반드시 입력을 한 글자 이상 소비한다.
fn recover(input: &mut Input<'_>, out: &mut Lexed) {
    let rest: &&str = input.as_ref();
    let rest = *rest;
    if rest.starts_with('"') {
        let r: ModalResult<((), Range<usize>)> = ('"', take_till(0.., ['\n', '\r']))
            .void()
            .with_span()
            .parse_next(input);
        if let Ok((_, r)) = r {
            let span = Span::new(r.start, r.end);
            out.tokens.push(Token {
                kind: TokenKind::Str,
                span,
            });
            out.diagnostics.push(Diagnostic::error(
                codes::UNTERMINATED_STRING,
                span,
                "닫히지 않은 문자열",
            ));
            return;
        }
    } else if rest.starts_with("/*") {
        let r: ModalResult<((), Range<usize>)> = ("/*", take_till(0.., |_| false))
            .void()
            .with_span()
            .parse_next(input);
        if let Ok((_, r)) = r {
            let span = Span::new(r.start, r.end);
            out.comments.push(Comment { span, block: true });
            out.diagnostics.push(Diagnostic::error(
                codes::UNTERMINATED_COMMENT,
                span,
                "닫히지 않은 블록 주석",
            ));
            return;
        }
    }
    let one: ModalResult<(char, Range<usize>)> = any.with_span().parse_next(input);
    if let Ok((c, r)) = one {
        let span = Span::new(r.start, r.end);
        out.tokens.push(Token {
            kind: TokenKind::Unknown,
            span,
        });
        out.diagnostics.push(Diagnostic::error(
            codes::UNEXPECTED_CHAR,
            span,
            format!("알 수 없는 문자 {c:?}"),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(TokenKind, &str)> {
        lex(src)
            .tokens
            .iter()
            .map(|t| (t.kind, t.span.text(src)))
            .collect()
    }

    #[test]
    fn lexes_header_line() {
        let src = "LIN_speed = 19.2 kbps;";
        assert_eq!(
            kinds(src),
            vec![
                (TokenKind::Ident, "LIN_speed"),
                (TokenKind::Punct('='), "="),
                (TokenKind::Number, "19.2"),
                (TokenKind::Ident, "kbps"),
                (TokenKind::Punct(';'), ";"),
            ]
        );
    }

    #[test]
    fn lexes_hex_negative_and_strings() {
        let src = r#"a: 0x3c, -5, +2.5e3, "x y";"#;
        let k = kinds(src);
        assert!(k.contains(&(TokenKind::Number, "0x3c")));
        assert!(k.contains(&(TokenKind::Number, "-5")));
        assert!(k.contains(&(TokenKind::Number, "+2.5e3")));
        assert!(k.contains(&(TokenKind::Str, "\"x y\"")));
    }

    #[test]
    fn keeps_comments_out_of_tokens() {
        let src = "a // line\n /* blk\n */ b";
        let l = lex(src);
        assert_eq!(l.tokens.len(), 2);
        assert_eq!(l.comments.len(), 2);
        assert!(!l.comments[0].block);
        assert!(l.comments[1].block);
        assert!(l.diagnostics.is_empty());
    }

    #[test]
    fn unterminated_string_and_comment_are_diagnosed() {
        let l = lex("a \"abc\n b");
        assert!(
            l.diagnostics
                .iter()
                .any(|d| d.code == codes::UNTERMINATED_STRING)
        );
        assert_eq!(
            l.tokens.last().map(|t| t.span.text("a \"abc\n b")),
            Some("b")
        );

        let l = lex("a /* never closed");
        assert!(
            l.diagnostics
                .iter()
                .any(|d| d.code == codes::UNTERMINATED_COMMENT)
        );
    }

    #[test]
    fn unknown_chars_never_panic_or_loop() {
        let src = "a @ # 한 $ b";
        let l = lex(src);
        assert_eq!(
            l.diagnostics
                .iter()
                .filter(|d| d.code == codes::UNEXPECTED_CHAR)
                .count(),
            4
        );
        assert_eq!(
            l.tokens
                .iter()
                .filter(|t| t.kind == TokenKind::Ident)
                .count(),
            2
        );
    }

    #[test]
    fn bom_and_crlf_are_whitespace() {
        let l = lex("\u{feff}a\r\nb");
        assert_eq!(l.tokens.len(), 2);
    }
}
