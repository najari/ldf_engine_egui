//! 텍스트 편집기: 줄 번호, LDF 구문 강조, 지정한 줄로 이동(FR-CHK-04, FR-CHK-08).

use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::{Align, Color32, FontId, Id, RichText, ScrollArea, TextEdit, TextStyle, Ui};
use ldf_syntax::{TokenKind, lex};

/// 강조할 LDF 키워드(섹션 이름, 헤더 항목, 스케줄 명령 등).
const KEYWORDS: &[&str] = &[
    "LIN_description_file",
    "LIN_protocol_version",
    "LIN_language_version",
    "LIN_speed",
    "LDF_file_revision",
    "LIN_sig_byte_order_big_endian",
    "Channel_name",
    "Nodes",
    "Master",
    "Slaves",
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
    "dynamic_frames",
    "Diagnostic_addresses",
    "delay",
    "physical_value",
    "logical_value",
    "bcd_value",
    "ascii_value",
    "configurable_frames",
    "AssignNAD",
    "AssignFrameId",
    "AssignFrameIdRange",
    "ConditionalChangeNAD",
    "DataDump",
    "SaveConfiguration",
    "FreeFormat",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Style {
    Plain,
    Keyword,
    Number,
    Str,
    Punct,
    Comment,
    Bad,
}

fn color(style: Style, dark: bool) -> Color32 {
    let (d, l) = match style {
        Style::Plain => ((212, 212, 212), (30, 30, 30)),
        Style::Keyword => ((86, 156, 214), (0, 0, 190)),
        Style::Number => ((181, 206, 168), (9, 120, 80)),
        Style::Str => ((206, 145, 120), (163, 21, 21)),
        Style::Punct => ((160, 160, 160), (90, 90, 90)),
        Style::Comment => ((106, 153, 85), (0, 120, 0)),
        Style::Bad => ((244, 71, 71), (205, 49, 49)),
    };
    let (r, g, b) = if dark { d } else { l };
    Color32::from_rgb(r, g, b)
}

/// 텍스트를 구문 강조한 `LayoutJob`으로 만든다. 줄바꿈은 하지 않는다.
/// 이어 붙인 조각은 항상 원문과 같다(공백·주석 포함).
pub fn highlight(text: &str, dark: bool, font: &FontId) -> LayoutJob {
    let lexed = lex(text);
    let mut segs: Vec<(usize, usize, Style)> =
        Vec::with_capacity(lexed.tokens.len() + lexed.comments.len());
    for t in &lexed.tokens {
        let style = match t.kind {
            TokenKind::Ident => {
                if KEYWORDS.contains(&t.span.text(text)) {
                    Style::Keyword
                } else {
                    Style::Plain
                }
            }
            TokenKind::Number => Style::Number,
            TokenKind::Str => Style::Str,
            TokenKind::Punct(_) => Style::Punct,
            TokenKind::Unknown => Style::Bad,
        };
        segs.push((t.span.start, t.span.end, style));
    }
    segs.extend(
        lexed
            .comments
            .iter()
            .map(|c| (c.span.start, c.span.end, Style::Comment)),
    );
    segs.sort_by_key(|s| s.0);

    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let fmt = |style: Style| TextFormat {
        font_id: font.clone(),
        color: color(style, dark),
        ..TextFormat::default()
    };
    let mut pos = 0usize;
    for (start, end, style) in segs {
        if start < pos {
            continue;
        }
        if let Some(gap) = text.get(pos..start).filter(|g| !g.is_empty()) {
            job.append(gap, 0.0, fmt(Style::Plain));
        }
        if let Some(piece) = text.get(start..end) {
            job.append(piece, 0.0, fmt(style));
            pos = end;
        } else {
            pos = start;
        }
    }
    if let Some(rest) = text.get(pos..).filter(|r| !r.is_empty()) {
        job.append(rest, 0.0, fmt(Style::Plain));
    }
    job
}

/// 1부터 시작하는 줄의 문자 범위 `[시작, 끝)`(줄바꿈 제외). 범위를 벗어나면 마지막 줄.
pub fn line_char_range(text: &str, line: usize) -> (usize, usize) {
    let mut start_byte = 0usize;
    let mut current = 1usize;
    let target = line.max(1);
    for (i, b) in text.bytes().enumerate() {
        if current == target {
            break;
        }
        if b == b'\n' {
            current += 1;
            start_byte = i + 1;
        }
    }
    let rest = text.get(start_byte..).unwrap_or("");
    let len = rest.find('\n').unwrap_or(rest.len());
    let line_text = rest.get(..len).unwrap_or("").trim_end_matches('\r');
    let start = text.get(..start_byte).map_or(0, |s| s.chars().count());
    (start, start + line_text.chars().count())
}

/// 편집기 상태.
#[derive(Default)]
pub struct EditorState {
    /// 편집 중인 전체 텍스트.
    pub text: String,
    /// 원본에서 바뀌었는지.
    pub dirty: bool,
    /// 다음 프레임에 이동할 줄(1부터).
    pub goto: Option<usize>,
}

impl EditorState {
    /// 텍스트를 새로 채우고 수정 표시를 지운다.
    pub fn reset(&mut self, text: String) {
        self.text = text;
        self.dirty = false;
        self.goto = None;
    }
}

const EDITOR_ID: &str = "checker-text-editor";

/// 편집기를 그린다. 텍스트가 바뀌면 `dirty`가 켜진다.
pub fn show(ui: &mut Ui, st: &mut EditorState) {
    let dark = ui.visuals().dark_mode;
    let font = TextStyle::Monospace.resolve(ui.style());
    let id = Id::new(EDITOR_ID);
    let weak = ui.visuals().weak_text_color();

    let lines = st.text.bytes().filter(|b| *b == b'\n').count() + 1;
    let width = lines.to_string().len();
    let gutter: String = (1..=lines)
        .map(|n| format!("{n:>width$}"))
        .collect::<Vec<_>>()
        .join("\n");

    ScrollArea::both()
        .id_salt("checker-editor-scroll")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.label(RichText::new(gutter).font(font.clone()).color(weak));

                let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, _wrap: f32| {
                    let job = highlight(text.as_str(), dark, &font);
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                let out = TextEdit::multiline(&mut st.text)
                    .id(id)
                    .code_editor()
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY)
                    .layouter(&mut layouter)
                    .show(ui);
                if out.response.changed() {
                    st.dirty = true;
                }

                if let Some(line) = st.goto.take() {
                    let (a, b) = line_char_range(&st.text, line);
                    let mut state = out.state.clone();
                    state
                        .cursor
                        .set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(b))));
                    state.store(ui.ctx(), id);
                    out.response.request_focus();
                    let rect = out
                        .galley
                        .pos_from_cursor(CCursor::new(a))
                        .translate(out.galley_pos.to_vec2());
                    ui.scroll_to_rect(rect.expand2(egui::vec2(0.0, 60.0)), Some(Align::Center));
                    ui.ctx().request_repaint();
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(job: &LayoutJob) -> String {
        job.sections
            .iter()
            .filter_map(|s| {
                job.text
                    .get(usize::from(s.byte_range.start)..usize::from(s.byte_range.end))
            })
            .collect()
    }

    #[test]
    fn highlight_preserves_every_character() {
        let src = "// c\nNodes {\n  Master: GW, 5 ms, 0.1 ms ;\n  \"x\" @ \u{d55c}\n}\n/* b */";
        let job = highlight(src, true, &FontId::monospace(12.0));
        assert_eq!(joined(&job), src);
        assert_eq!(job.text, src);
    }

    #[test]
    fn highlight_colors_keywords_numbers_strings_and_comments_differently() {
        let src = "Nodes { x: 19.2, \"s\" } // c";
        let job = highlight(src, true, &FontId::monospace(12.0));
        let color_of = |needle: &str| {
            let at = src.find(needle).expect("needle");
            job.sections
                .iter()
                .find(|s| {
                    usize::from(s.byte_range.start) <= at && at < usize::from(s.byte_range.end)
                })
                .map(|s| s.format.color)
                .expect("section")
        };
        let kw = color_of("Nodes");
        let num = color_of("19.2");
        let st = color_of("\"s\"");
        let cm = color_of("// c");
        let plain = color_of("x");
        let all = [kw, num, st, cm, plain];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn highlight_never_panics_on_odd_input() {
        for s in [
            "",
            "\u{feff}",
            "\"unterminated",
            "/* open",
            "{{{",
            "한글 주석 // 끝",
        ] {
            let job = highlight(s, false, &FontId::monospace(12.0));
            assert_eq!(job.text, s);
        }
    }

    #[test]
    fn line_ranges_are_in_chars_and_exclude_newlines() {
        let t = "ab\n한글 x\r\nlast";
        assert_eq!(line_char_range(t, 1), (0, 2));
        assert_eq!(line_char_range(t, 2), (3, 7));
        assert_eq!(line_char_range(t, 3), (9, 13));
        // 범위를 벗어난 줄은 마지막 줄
        assert_eq!(line_char_range(t, 99), (9, 13));
        assert_eq!(line_char_range("", 1), (0, 0));
        assert_eq!(line_char_range("a\n", 2), (2, 2));
    }
}
