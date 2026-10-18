//! 소스 위치. 모든 위치는 원문 UTF-8 바이트 오프셋이다.

use serde::Serialize;

/// 원문의 바이트 범위 `[start, end)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize)]
pub struct Span {
    /// 시작 바이트 오프셋(포함).
    pub start: usize,
    /// 끝 바이트 오프셋(미포함).
    pub end: usize,
}

impl Span {
    /// 새 범위를 만든다.
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// 범위 길이(바이트).
    pub const fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// 빈 범위인지.
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// 두 범위를 모두 덮는 가장 작은 범위.
    pub fn join(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }

    /// 원문에서 이 범위의 텍스트. 범위가 문자 경계가 아니면 빈 문자열.
    pub fn text<'a>(&self, src: &'a str) -> &'a str {
        src.get(self.start..self.end).unwrap_or("")
    }
}

/// 바이트 오프셋을 줄·열로 바꾸는 색인.
#[derive(Clone, Debug)]
pub struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    /// 원문의 줄 시작 위치를 모은다.
    pub fn new(src: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(src.match_indices('\n').map(|(i, _)| i + 1));
        Self { starts }
    }

    /// 1부터 시작하는 (줄, 열). 열은 문자(코드포인트) 단위다.
    pub fn line_col(&self, src: &str, offset: usize) -> (usize, usize) {
        let offset = offset.min(src.len());
        let line = self.starts.partition_point(|&s| s <= offset).max(1);
        let line_start = self.starts.get(line - 1).copied().unwrap_or(0);
        let col = src.get(line_start..offset).map_or(0, |s| s.chars().count()) + 1;
        (line, col)
    }

    /// 전체 줄 수.
    pub fn line_count(&self) -> usize {
        self.starts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_counts_chars_not_bytes() {
        let src = "ab\n한글 x\n";
        let idx = LineIndex::new(src);
        assert_eq!(idx.line_col(src, 0), (1, 1));
        assert_eq!(idx.line_col(src, 3), (2, 1));
        // '한글 ' 은 바이트 7개(3+3+1), 문자 3개
        assert_eq!(idx.line_col(src, 3 + 7), (2, 4));
        assert_eq!(idx.line_count(), 3);
    }

    #[test]
    fn span_text_is_safe_on_bad_ranges() {
        let src = "한";
        assert_eq!(Span::new(0, 1).text(src), "");
        assert_eq!(Span::new(0, 3).text(src), "한");
        assert_eq!(Span::new(2, 99).text(src), "");
    }
}
