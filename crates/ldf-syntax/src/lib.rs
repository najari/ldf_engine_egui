//! LDF 구문 계층. 문자를 토큰으로(`lexer`, winnow), 토큰을 문장·블록 트리로(`parser`) 바꾼다.
//!
//! 이 크레이트는 섹션의 의미를 모른다. 이름 해석과 타입은 `ldf-model`의 몫이다.
//! 내부 크레이트에 의존하지 않는다.

mod ast;
mod diag;
mod lexer;
mod parser;
mod span;

pub use ast::{Block, Elem, File, Stmt};
pub use diag::{Diagnostic, Severity, codes};
pub use lexer::{Comment, Lexed, Token, TokenKind, lex};
pub use parser::{Parsed, parse};
pub use span::{LineIndex, Span};
