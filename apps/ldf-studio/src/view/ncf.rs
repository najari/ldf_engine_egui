//! NCF 창에 보일 표·목록(egui 무관, 단위 테스트 대상). 값은 `ldf-ncf`의 `describe`가 만든 글을 쓴다.

use ldf_ncf::{
    Direction, EncodingRef, Item, NcfEncoding, NcfFrame, NcfNode, describe_entry, describe_init,
    format_ms, hex,
};

/// 줄의 이름: 화면 문구(번역한다)이거나 파일에 있던 키 그대로다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Label {
    /// 번역할 화면 문구.
    Text(&'static str),
    /// 파일의 키(번역하지 않는다).
    Raw(String),
}

/// `이름: 값` 한 줄.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// 이름.
    pub label: Label,
    /// 값.
    pub value: String,
}

fn row(label: &'static str, value: impl Into<String>) -> Row {
    Row {
        label: Label::Text(label),
        value: value.into(),
    }
}

fn raw_rows(items: &[Item]) -> impl Iterator<Item = Row> + '_ {
    items.iter().map(|i| Row {
        label: Label::Raw(i.key.clone()),
        value: i.value.clone(),
    })
}

/// `general`: 읽은 항목만 보이고, 해석하지 않은 항목은 키 그대로 뒤에 붙는다.
pub fn general_rows(n: &NcfNode) -> Vec<Row> {
    let g = &n.general;
    let mut rows = Vec::new();
    if let Some(v) = &g.protocol_version {
        rows.push(row("LIN 프로토콜 버전", v));
    }
    if let Some(v) = g.supplier {
        rows.push(row("공급자 ID", hex(v, 4)));
    }
    if let Some(v) = g.function {
        rows.push(row("기능 ID", hex(v, 4)));
    }
    if let Some(v) = g.variant {
        rows.push(row("변형", v.to_string()));
    }
    if let Some(b) = &g.bitrate {
        rows.push(row("비트레이트", b.describe()));
    }
    if let Some(v) = &g.sends_wake_up_signal {
        rows.push(row("웨이크업 신호 전송", v));
    }
    rows.extend(raw_rows(&g.extra));
    rows
}

/// `diagnostic`.
pub fn diagnostic_rows(n: &NcfNode) -> Vec<Row> {
    let d = &n.diagnostic;
    let mut rows = Vec::new();
    if let Some(v) = &d.nad {
        rows.push(row("NAD", v.describe()));
    }
    if let Some(v) = d.diagnostic_class {
        rows.push(row("진단 클래스", v.to_string()));
    }
    for (label, v) in [
        ("P2_min", d.p2_min_ms),
        ("ST_min", d.st_min_ms),
        ("N_As_timeout", d.n_as_timeout_ms),
        ("N_Cr_timeout", d.n_cr_timeout_ms),
    ] {
        if let Some(ms) = v {
            rows.push(Row {
                label: Label::Raw(label.to_owned()),
                value: format_ms(ms),
            });
        }
    }
    if let Some(v) = d.max_message_length {
        rows.push(row("최대 메시지 길이", v.to_string()));
    }
    if !d.support_sid.is_empty() {
        let sids: Vec<String> = d.support_sid.iter().map(|s| hex(*s, 2)).collect();
        rows.push(row("지원 서비스 ID", sids.join(", ")));
    }
    rows.extend(raw_rows(&d.extra));
    rows
}

/// `status_management`(파일의 항목 그대로).
pub fn status_rows(n: &NcfNode) -> Vec<Row> {
    raw_rows(&n.status_management).collect()
}

/// 노드 수준의 해석하지 않은 항목.
pub fn extra_rows(n: &NcfNode) -> Vec<Row> {
    raw_rows(&n.extra).collect()
}

/// 프레임 목록의 한 줄.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameRow {
    /// `발행`/`구독`(번역할 문구).
    pub direction: &'static str,
    /// 프레임 이름.
    pub name: String,
    /// 길이(바이트), 없으면 `-`.
    pub length: String,
    /// 메시지 ID, 없으면 `-`.
    pub message_id: String,
    /// 이벤트 트리거 연결, 없으면 `-`.
    pub event: String,
    /// 주기, 없으면 `-`.
    pub period: String,
    /// 시그널 개수.
    pub signals: usize,
}

fn or_dash(v: Option<String>) -> String {
    v.unwrap_or_else(|| "-".to_owned())
}

fn period(f: &NcfFrame) -> String {
    match (f.min_period_ms, f.max_period_ms) {
        (Some(lo), Some(hi)) => format!("{} – {}", format_ms(lo), format_ms(hi)),
        (Some(v), None) | (None, Some(v)) => format_ms(v),
        (None, None) => "-".to_owned(),
    }
}

/// `frames`.
pub fn frame_rows(n: &NcfNode) -> Vec<FrameRow> {
    n.frames
        .iter()
        .map(|f| FrameRow {
            direction: match f.direction {
                Direction::Publish => "발행",
                Direction::Subscribe => "구독",
            },
            name: f.name.clone(),
            length: or_dash(f.length.map(|v| v.to_string())),
            message_id: or_dash(f.message_id.map(|v| hex(v, 4))),
            event: or_dash(f.event_triggered.as_ref().map(|e| e.describe())),
            period: period(f),
            signals: f.signals.len(),
        })
        .collect()
}

/// 시그널 목록의 한 줄.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignalRow {
    /// 이름.
    pub name: String,
    /// 초기값.
    pub init: String,
    /// 크기(비트).
    pub size: String,
    /// 시작 비트.
    pub offset: String,
    /// 인코딩 이름(그 자리에서 정의했으면 이름 뒤에 `*`).
    pub encoding: String,
}

/// 프레임의 시그널들.
pub fn signal_rows(f: &NcfFrame) -> Vec<SignalRow> {
    f.signals
        .iter()
        .map(|s| SignalRow {
            name: s.name.clone(),
            init: or_dash(s.init.as_ref().map(describe_init)),
            size: or_dash(s.size.map(|v| v.to_string())),
            offset: or_dash(s.offset.map(|v| v.to_string())),
            encoding: match &s.encoding {
                Some(EncodingRef::Named(n)) => n.clone(),
                Some(EncodingRef::Inline(e)) => format!("{}*", e.name),
                None => "-".to_owned(),
            },
        })
        .collect()
}

/// 인코딩 하나의 항목 줄들.
pub fn encoding_lines(e: &NcfEncoding) -> Vec<String> {
    e.entries.iter().map(describe_entry).collect()
}

/// 노드에서 보여 줄 인코딩 전부: `encoding` 섹션의 것과 시그널 안에서 정의한 것.
pub fn all_encodings(n: &NcfNode) -> Vec<&NcfEncoding> {
    n.all_encodings()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_ncf::parse;

    const NCF: &str = "node_capability_file;\n\
node N {\n\
 general { LIN_protocol_version = \"2.1\"; supplier = 0x1e; function = 1; variant = 0; bitrate = 19.2 kbps; response_tolerance = 40 %; }\n\
 diagnostic { NAD = 5; P2_min = 50 ms; support_sid { 0xB0; } }\n\
 frames {\n\
  publish A { length = 2; message_ID = 0x10; event_triggered_frame = B; min_period = 5 ms; max_period = 20 ms;\n\
    signals { S1 { init_value = 0; size = 8; offset = 0; enc; } S2 { init_value = {1, 2}; size = 8; offset = 8; encoding inl { logical_value, 1, \"x\"; }; } } }\n\
  subscribe B { length = 1; }\n\
 }\n\
 encoding { enc { logical_value, 0, \"off\"; } }\n\
 status_management { response_error = S1; }\n\
}\n";

    fn node() -> NcfNode {
        let p = parse(NCF);
        assert!(
            p.diagnostics.iter().all(|d| d.code != 9203),
            "{:?}",
            p.diagnostics
        );
        p.file.nodes.into_iter().next().expect("node")
    }

    fn text(rows: &[Row]) -> Vec<(String, String)> {
        rows.iter()
            .map(|r| {
                let l = match &r.label {
                    Label::Text(t) => (*t).to_owned(),
                    Label::Raw(s) => s.clone(),
                };
                (l, r.value.clone())
            })
            .collect()
    }

    #[test]
    fn general_and_diagnostic_rows_follow_the_file() {
        let n = node();
        assert_eq!(
            text(&general_rows(&n)),
            [
                ("LIN 프로토콜 버전".to_owned(), "2.1".to_owned()),
                ("공급자 ID".to_owned(), "0x001E".to_owned()),
                ("기능 ID".to_owned(), "0x0001".to_owned()),
                ("변형".to_owned(), "0".to_owned()),
                ("비트레이트".to_owned(), "19.2 kbps".to_owned()),
                ("response_tolerance".to_owned(), "40 %".to_owned()),
            ]
        );
        assert_eq!(
            text(&diagnostic_rows(&n)),
            [
                ("NAD".to_owned(), "0x05".to_owned()),
                ("P2_min".to_owned(), "50 ms".to_owned()),
                ("지원 서비스 ID".to_owned(), "0xB0".to_owned()),
            ]
        );
        assert_eq!(
            text(&status_rows(&n)),
            [("response_error".to_owned(), "S1".to_owned())]
        );
    }

    #[test]
    fn frame_and_signal_rows() {
        let n = node();
        let frames = frame_rows(&n);
        assert_eq!(
            frames[0],
            FrameRow {
                direction: "발행",
                name: "A".into(),
                length: "2".into(),
                message_id: "0x0010".into(),
                event: "B".into(),
                period: "5 ms – 20 ms".into(),
                signals: 2,
            }
        );
        assert_eq!(
            (
                frames[1].direction,
                frames[1].event.as_str(),
                frames[1].period.as_str()
            ),
            ("구독", "-", "-")
        );
        let sigs = signal_rows(&n.frames[0]);
        assert_eq!(
            (
                sigs[0].encoding.as_str(),
                sigs[1].init.as_str(),
                sigs[1].encoding.as_str()
            ),
            ("enc", "{1, 2}", "inl*")
        );
    }

    #[test]
    fn all_encodings_include_inline_ones() {
        let n = node();
        let all = all_encodings(&n);
        assert_eq!(
            all.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["enc", "inl"]
        );
        assert_eq!(encoding_lines(all[1]), ["logical_value 1 \"x\""]);
    }
}
