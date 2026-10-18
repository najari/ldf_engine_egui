//! 일반 구문 트리. 섹션의 의미는 모른다 — `ldf-model::lower()`가 해석한다.
//!
//! LDF 문법은 균일하다: 파일은 문장의 나열이고, 문장은 토큰과 `{ }` 블록의 나열이며
//! `;`(또는 블록, 또는 둘러싼 `}`)로 끝난다.

use crate::{Comment, Span, Token};
use serde::Serialize;

/// 문장의 구성 요소.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Elem {
    /// 토큰 하나.
    Token(Token),
    /// `{ ... }` 블록.
    Block(Block),
}

impl Elem {
    /// 원문 위치.
    pub fn span(&self) -> Span {
        match self {
            Elem::Token(t) => t.span,
            Elem::Block(b) => b.span,
        }
    }
}

/// `{ ... }` 블록.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Block {
    /// 블록 안의 문장.
    pub stmts: Vec<Stmt>,
    /// 여는 `{`부터 닫는 `}`까지(닫는 괄호가 없으면 마지막 내용까지).
    pub span: Span,
}

/// 문장.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Stmt {
    /// 구성 요소. 끝의 `;`는 포함하지 않는다.
    pub elems: Vec<Elem>,
    /// 첫 요소부터 끝(`;` 포함)까지.
    pub span: Span,
}

/// 파일 전체.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct File {
    /// 최상위 문장.
    pub stmts: Vec<Stmt>,
    /// 주석(원문 순서).
    pub comments: Vec<Comment>,
}
