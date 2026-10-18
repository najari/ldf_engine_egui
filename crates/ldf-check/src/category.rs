//! 검사 카테고리(코드 대역).

/// 검사 카테고리. Vector Consistency Checker의 다섯 분류와 같다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    /// 00xx 기본 검사.
    Basic,
    /// 01xx LDF 형식(버전별 허용 섹션·속성).
    Format,
    /// 02xx 모델링.
    Modelling,
    /// 03xx 파라미터 범위.
    Parameter,
    /// 04xx 매핑(시그널·프레임 배치).
    Mapping,
}

impl Category {
    /// 규칙 코드의 카테고리. 코드가 대역 밖이면 `None`.
    pub fn of(code: u16) -> Option<Category> {
        match code / 100 {
            0 => Some(Category::Basic),
            1 => Some(Category::Format),
            2 => Some(Category::Modelling),
            3 => Some(Category::Parameter),
            4 => Some(Category::Mapping),
            _ => None,
        }
    }

    /// 화면 표시 이름.
    pub fn label(self) -> &'static str {
        match self {
            Category::Basic => "Basic checks",
            Category::Format => "LDF Format checks",
            Category::Modelling => "Modelling checks",
            Category::Parameter => "Parameter checks",
            Category::Mapping => "Mapping checks",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands() {
        assert_eq!(Category::of(10), Some(Category::Basic));
        assert_eq!(Category::of(101), Some(Category::Format));
        assert_eq!(Category::of(203), Some(Category::Modelling));
        assert_eq!(Category::of(302), Some(Category::Parameter));
        assert_eq!(Category::of(406), Some(Category::Mapping));
        assert_eq!(Category::of(9001), None);
    }
}
