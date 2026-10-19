//! 값을 사람이 읽는 짧은 글로 쓴다(언어와 무관한 숫자·단위·LDF 낱말만 쓴다). CLI와 Studio가 같은 글을 보여 주려고 여기서 만든다.

use crate::{Bitrate, EventTriggered, Nad};
use ldf_model::{EncodingEntry, InitValue};

/// bit/s 값을 `19.2 kbps`처럼 쓴다. 1000 미만은 `bps`로 쓴다.
pub fn format_bps(bps: f64) -> String {
    if bps >= 1000.0 {
        format!("{} kbps", bps / 1000.0)
    } else {
        format!("{bps} bps")
    }
}

/// ms 값을 `50 ms`처럼 쓴다.
pub fn format_ms(ms: f64) -> String {
    format!("{ms} ms")
}

/// `0x1234`처럼 16진수로 쓴다(`digits`자리 이상).
pub fn hex(v: u32, digits: usize) -> String {
    format!("0x{v:0digits$X}")
}

impl Bitrate {
    /// `19.2 kbps`, `automatic 10 kbps – 20 kbps`, `select 2.4 kbps | 9.6 kbps`.
    pub fn describe(&self) -> String {
        match self {
            Bitrate::Fixed { bps } => format_bps(*bps),
            Bitrate::Automatic { min_bps, max_bps } => match (min_bps, max_bps) {
                (None, None) => "automatic".to_owned(),
                (min, max) => format!(
                    "automatic {} – {}",
                    min.map_or_else(|| "…".to_owned(), format_bps),
                    max.map_or_else(|| "…".to_owned(), format_bps)
                ),
            },
            Bitrate::Select { bps } => format!(
                "select {}",
                bps.iter()
                    .map(|b| format_bps(*b))
                    .collect::<Vec<_>>()
                    .join(" | ")
            ),
        }
    }
}

impl Nad {
    /// `0x05`, `0x01, 0x02`, `0x01 to 0x05`.
    pub fn describe(&self) -> String {
        match self {
            Nad::List(v) => v.iter().map(|n| hex(*n, 2)).collect::<Vec<_>>().join(", "),
            Nad::Range { from, to } => format!("{} to {}", hex(*from, 2), hex(*to, 2)),
        }
    }
}

impl EventTriggered {
    /// 프레임 이름이나 `0x4567`.
    pub fn describe(&self) -> String {
        match self {
            EventTriggered::Frame(name) => name.clone(),
            EventTriggered::MessageId(id) => hex(*id, 4),
        }
    }
}

/// 초기값: 숫자는 10진수, 바이트 배열은 `{5, 4, 3}`.
pub fn describe_init(v: &InitValue) -> String {
    match v {
        InitValue::Scalar(n) => n.to_string(),
        InitValue::Array(bytes) => format!(
            "{{{}}}",
            bytes
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// 인코딩 항목 한 줄: LDF 낱말을 그대로 쓴다.
pub fn describe_entry(e: &EncodingEntry) -> String {
    match e {
        EncodingEntry::Logical { value, text } if text.is_empty() => {
            format!("logical_value {value}")
        }
        EncodingEntry::Logical { value, text } => format!("logical_value {value} \"{text}\""),
        EncodingEntry::Physical {
            min,
            max,
            scale,
            offset,
            unit,
        } => {
            let unit = if unit.is_empty() {
                String::new()
            } else {
                format!(" [{unit}]")
            };
            format!("physical_value {min}..{max}, × {scale}, + {offset}{unit}")
        }
        EncodingEntry::Bcd => "bcd_value".to_owned(),
        EncodingEntry::Ascii => "ascii_value".to_owned(),
    }
}
