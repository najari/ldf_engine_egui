//! 한글 글꼴 설치. egui 기본 글꼴에는 한글이 없으므로 Windows 시스템 글꼴을 폴백으로 붙인다.

use egui::{FontData, FontDefinitions, FontFamily};
use std::path::PathBuf;
use std::sync::Arc;

/// 후보 글꼴 파일(앞쪽이 우선). 모두 Windows에 기본 설치된다.
const CANDIDATES: [&str; 4] = ["malgun.ttf", "NanumGothic.ttf", "gulim.ttc", "batang.ttc"];

fn windows_font_dir() -> PathBuf {
    let root =
        std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    root.join("Fonts")
}

/// 한글 글꼴을 찾아 egui에 등록한다. 찾으면 글꼴 파일 이름을 돌려준다.
pub fn install(ctx: &egui::Context) -> Option<&'static str> {
    let dir = windows_font_dir();
    for name in CANDIDATES {
        let Ok(bytes) = std::fs::read(dir.join(name)) else {
            continue;
        };
        let mut fonts = FontDefinitions::default();
        fonts
            .font_data
            .insert("korean".to_owned(), Arc::new(FontData::from_owned(bytes)));
        // 기본 글꼴 뒤에 폴백으로 붙인다: 영문·숫자는 기존 글꼴, 한글만 시스템 글꼴
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push("korean".to_owned());
        }
        ctx.set_fonts(fonts);
        return Some(name);
    }
    None
}
