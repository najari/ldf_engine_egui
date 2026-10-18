//! 문자 인코딩 판별과 디코딩(FR-IMP-03). 원본 바이트는 DB에 그대로 보관되므로 판별이 틀려도 손실은 없다.

/// 인코딩 이름: UTF-8.
pub const UTF8: &str = "utf-8";
/// 인코딩 이름: BOM이 있는 UTF-8.
pub const UTF8_BOM: &str = "utf-8-bom";
/// 인코딩 이름: Windows-1252(서유럽).
pub const CP1252: &str = "windows-1252";

/// CP1252의 0x80–0x9F 구간. 정의되지 않은 5곳은 같은 코드의 C1 제어 문자로 둔다.
const CP1252_HIGH: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}',
    '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
];

/// 바이트의 인코딩을 판별한다: UTF-8 BOM → UTF-8 유효성 → CP1252 폴백.
pub fn detect(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        UTF8_BOM
    } else if std::str::from_utf8(bytes).is_ok() {
        UTF8
    } else {
        CP1252
    }
}

/// 이름이 가리키는 인코딩으로 디코딩한다. BOM은 떼어 낸다. 모르는 이름은 UTF-8(손실 변환)로 본다.
pub fn decode(bytes: &[u8], encoding: &str) -> String {
    match encoding {
        CP1252 => bytes
            .iter()
            .map(|&b| match b {
                0x80..=0x9f => CP1252_HIGH[usize::from(b - 0x80)],
                _ => char::from(b),
            })
            .collect(),
        _ => {
            let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
            String::from_utf8_lossy(body).into_owned()
        }
    }
}

/// 판별과 디코딩을 한 번에.
pub fn decode_auto(bytes: &[u8]) -> (String, &'static str) {
    let enc = detect(bytes);
    (decode(bytes, enc), enc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_and_bom() {
        assert_eq!(detect("한글 abc".as_bytes()), UTF8);
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice("abc".as_bytes());
        assert_eq!(detect(&with_bom), UTF8_BOM);
        assert_eq!(decode(&with_bom, UTF8_BOM), "abc");
    }

    #[test]
    fn german_comments_in_cp1252_are_not_garbled() {
        // "Türschloß – Größe €" 를 CP1252 로 인코딩한 바이트
        let bytes = [
            b'T', 0xFC, b'r', b's', b'c', b'h', b'l', b'o', 0xDF, b' ', 0x96, b' ', b'G', b'r',
            0xF6, 0xDF, b'e', b' ', 0x80,
        ];
        assert_eq!(detect(&bytes), CP1252);
        assert_eq!(decode(&bytes, CP1252), "Türschloß – Größe €");
    }

    #[test]
    fn ascii_is_utf8() {
        assert_eq!(detect(b"LIN_speed = 19.2 kbps;"), UTF8);
        assert_eq!(decode_auto(b"abc"), ("abc".to_owned(), UTF8));
    }
}
