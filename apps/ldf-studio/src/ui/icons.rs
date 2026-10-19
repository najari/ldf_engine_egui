//! 앱 안의 아이콘. 그림 파일이나 글꼴 없이 `egui::Painter`로 직접 그려서 화면 배율(100~200%)과 다크/라이트 테마에 맞게 늘 선명하다.
//!
//! 아이콘마다 0..1 단위 정사각형 안의 좌표로 정의하고 [`paint`]가 주어진 영역에 맞춰 그린다.
//! 트리 아이콘은 종류별 색을, 도구 모음 아이콘은 글자색을, 심각도 아이콘은 오류·경고·참고 색을 쓴다.

use egui::epaint::PathStroke;
use egui::{
    Color32, FontId, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
    pos2, vec2,
};
use egui::{WidgetInfo, WidgetType};

/// 기본 아이콘 크기(점).
pub const SIZE: f32 = 16.0;

/// 그릴 수 있는 아이콘.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    /// 폴더(열기, 트리 그룹).
    Folder,
    /// 실행 취소.
    Undo,
    /// 다시 실행.
    Redo,
    /// 저장(디스켓).
    Save,
    /// 검사(원 안의 체크).
    Check,
    /// 내보내기(상자에서 나가는 화살표).
    Export,
    /// 리비전(시계).
    History,
    /// 검색(돋보기).
    Search,
    /// 닫기.
    Close,
    /// 삭제(휴지통).
    Trash,
    /// 프레임.
    Frame,
    /// 시그널(파형).
    Signal,
    /// 노드(칩).
    Node,
    /// 스케줄 테이블.
    Schedule,
    /// 인코딩 타입(중괄호).
    Encoding,
    /// 진단 명령(프롬프트).
    Command,
    /// 클러스터(버스에 매달린 노드들).
    Cluster,
    /// 오류.
    Error,
    /// 경고.
    Warning,
    /// 참고.
    Info,
}

impl Icon {
    /// 모든 아이콘(시험용).
    #[cfg(test)]
    pub const ALL: [Icon; 20] = [
        Icon::Folder,
        Icon::Undo,
        Icon::Redo,
        Icon::Save,
        Icon::Check,
        Icon::Export,
        Icon::History,
        Icon::Search,
        Icon::Close,
        Icon::Trash,
        Icon::Frame,
        Icon::Signal,
        Icon::Node,
        Icon::Schedule,
        Icon::Encoding,
        Icon::Command,
        Icon::Cluster,
        Icon::Error,
        Icon::Warning,
        Icon::Info,
    ];
}

/// 색조·채도·명도로 만든 색(테마별 명도는 호출하는 쪽이 정한다).
fn hsv(h: f32, s: f32, v: f32) -> Color32 {
    egui::ecolor::Hsva::new(h, s, v, 1.0).into()
}

/// 아이콘이 뜻으로 가지는 색. `None`이면 주변 글자색을 쓴다.
pub fn tint(icon: Icon, dark: bool) -> Option<Color32> {
    let (s, v) = if dark { (0.55, 0.85) } else { (0.65, 0.65) };
    Some(match icon {
        Icon::Frame | Icon::Cluster => hsv(0.58, s, v),
        Icon::Signal | Icon::Check => hsv(0.38, s, v),
        Icon::Node => hsv(0.07, s, v),
        Icon::Schedule => hsv(0.76, s * 0.8, v),
        Icon::Encoding => hsv(0.50, s, v),
        Icon::Folder => hsv(0.12, s * 0.7, v),
        Icon::Error => Color32::from_rgb(220, 70, 70),
        Icon::Warning => Color32::from_rgb(232, 170, 40),
        Icon::Info => Color32::from_rgb(70, 140, 225),
        _ => return None,
    })
}

/// 영역 안에 아이콘을 그린다. `color`는 선(채우는 아이콘은 바탕)의 색이다.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let (w, h) = (rect.width(), rect.height());
    let at = |x: f32, y: f32| pos2(rect.left() + x * w, rect.top() + y * h);
    let stroke = Stroke::new((w.min(h) / SIZE * 1.4).max(1.0), color);
    let line = |pts: &[(f32, f32)]| {
        painter.add(Shape::line(
            pts.iter().map(|&(x, y)| at(x, y)).collect(),
            stroke,
        ));
    };
    let closed = |pts: &[(f32, f32)]| {
        painter.add(Shape::closed_line(
            pts.iter().map(|&(x, y)| at(x, y)).collect(),
            stroke,
        ));
    };
    let boxed = |x0: f32, y0: f32, x1: f32, y1: f32, round: f32| {
        painter.rect_stroke(
            Rect::from_min_max(at(x0, y0), at(x1, y1)),
            round * w.min(h),
            stroke,
            StrokeKind::Middle,
        );
    };
    let circle = |cx: f32, cy: f32, r: f32| {
        painter.circle_stroke(at(cx, cy), r * w.min(h), stroke);
    };
    let dot = |cx: f32, cy: f32, r: f32, fill: Color32| {
        painter.circle_filled(at(cx, cy), r * w.min(h), fill);
    };
    let white = Color32::WHITE;

    match icon {
        Icon::Folder => closed(&[
            (0.08, 0.22),
            (0.38, 0.22),
            (0.46, 0.34),
            (0.92, 0.34),
            (0.92, 0.82),
            (0.08, 0.82),
        ]),
        Icon::Undo | Icon::Redo => {
            // 오른쪽으로 꺾여 돌아오는 화살표. 다시 실행은 좌우를 뒤집는다
            let m = |x: f32| if icon == Icon::Undo { x } else { 1.0 - x };
            line(&[
                (m(0.84), 0.82),
                (m(0.84), 0.58),
                (m(0.70), 0.42),
                (m(0.28), 0.42),
            ]);
            line(&[(m(0.30), 0.22), (m(0.12), 0.42), (m(0.30), 0.62)]);
        }
        Icon::Save => {
            closed(&[
                (0.12, 0.12),
                (0.74, 0.12),
                (0.88, 0.26),
                (0.88, 0.88),
                (0.12, 0.88),
            ]);
            line(&[(0.28, 0.12), (0.28, 0.36), (0.64, 0.36), (0.64, 0.12)]);
            boxed(0.26, 0.56, 0.74, 0.88, 0.0);
        }
        Icon::Check => {
            circle(0.5, 0.5, 0.40);
            line(&[(0.30, 0.52), (0.45, 0.67), (0.72, 0.36)]);
        }
        Icon::Export => {
            line(&[(0.12, 0.62), (0.12, 0.86), (0.88, 0.86), (0.88, 0.62)]);
            line(&[(0.5, 0.70), (0.5, 0.14)]);
            line(&[(0.30, 0.34), (0.5, 0.14), (0.70, 0.34)]);
        }
        Icon::History => {
            circle(0.5, 0.5, 0.40);
            line(&[(0.5, 0.26), (0.5, 0.5), (0.68, 0.62)]);
        }
        Icon::Search => {
            circle(0.42, 0.42, 0.28);
            line(&[(0.63, 0.63), (0.88, 0.88)]);
        }
        Icon::Close => {
            line(&[(0.22, 0.22), (0.78, 0.78)]);
            line(&[(0.78, 0.22), (0.22, 0.78)]);
        }
        Icon::Trash => {
            line(&[(0.14, 0.26), (0.86, 0.26)]);
            line(&[(0.38, 0.26), (0.38, 0.14), (0.62, 0.14), (0.62, 0.26)]);
            closed(&[(0.24, 0.26), (0.30, 0.88), (0.70, 0.88), (0.76, 0.26)]);
            line(&[(0.43, 0.42), (0.43, 0.74)]);
            line(&[(0.57, 0.42), (0.57, 0.74)]);
        }
        Icon::Frame => {
            boxed(0.10, 0.20, 0.90, 0.80, 0.08);
            line(&[(0.10, 0.42), (0.90, 0.42)]);
            line(&[(0.34, 0.42), (0.34, 0.80)]);
            line(&[(0.62, 0.42), (0.62, 0.80)]);
        }
        Icon::Signal => {
            let pts: Vec<(f32, f32)> = (0..=16)
                .map(|i| {
                    let t = i as f32 / 16.0;
                    (
                        0.08 + 0.84 * t,
                        0.5 - 0.30 * (std::f32::consts::TAU * t).sin(),
                    )
                })
                .collect();
            line(&pts);
        }
        Icon::Node => {
            boxed(0.27, 0.27, 0.73, 0.73, 0.06);
            for p in [0.40, 0.60] {
                line(&[(0.10, p), (0.27, p)]);
                line(&[(0.73, p), (0.90, p)]);
                line(&[(p, 0.10), (p, 0.27)]);
                line(&[(p, 0.73), (p, 0.90)]);
            }
        }
        Icon::Schedule => {
            boxed(0.12, 0.18, 0.88, 0.88, 0.06);
            line(&[(0.12, 0.40), (0.88, 0.40)]);
            line(&[(0.12, 0.64), (0.88, 0.64)]);
            line(&[(0.50, 0.40), (0.50, 0.88)]);
            line(&[(0.32, 0.08), (0.32, 0.26)]);
            line(&[(0.68, 0.08), (0.68, 0.26)]);
        }
        Icon::Encoding => {
            for mirror in [false, true] {
                let m = |x: f32| if mirror { 1.0 - x } else { x };
                line(&[
                    (m(0.42), 0.14),
                    (m(0.30), 0.20),
                    (m(0.30), 0.44),
                    (m(0.16), 0.50),
                    (m(0.30), 0.56),
                    (m(0.30), 0.80),
                    (m(0.42), 0.86),
                ]);
            }
        }
        Icon::Command => {
            line(&[(0.14, 0.28), (0.46, 0.50), (0.14, 0.72)]);
            line(&[(0.54, 0.76), (0.88, 0.76)]);
        }
        Icon::Cluster => {
            line(&[(0.06, 0.5), (0.94, 0.5)]);
            for x in [0.2, 0.5, 0.8] {
                line(&[(x, 0.5), (x, 0.32)]);
                circle(x, 0.24, 0.09);
            }
            line(&[(0.5, 0.5), (0.5, 0.70)]);
            circle(0.5, 0.78, 0.09);
        }
        Icon::Error => {
            dot(0.5, 0.5, 0.44, color);
            let x = Stroke::new(stroke.width, white);
            painter.line_segment([at(0.35, 0.35), at(0.65, 0.65)], x);
            painter.line_segment([at(0.65, 0.35), at(0.35, 0.65)], x);
        }
        Icon::Warning => {
            painter.add(Shape::convex_polygon(
                vec![at(0.5, 0.10), at(0.94, 0.88), at(0.06, 0.88)],
                color,
                PathStroke::NONE,
            ));
            let dark = Color32::from_rgb(40, 30, 10);
            painter.line_segment(
                [at(0.5, 0.36), at(0.5, 0.62)],
                Stroke::new(stroke.width * 1.1, dark),
            );
            dot(0.5, 0.75, 0.055, dark);
        }
        Icon::Info => {
            dot(0.5, 0.5, 0.44, color);
            dot(0.5, 0.30, 0.06, white);
            painter.line_segment(
                [at(0.5, 0.45), at(0.5, 0.72)],
                Stroke::new(stroke.width * 1.1, white),
            );
        }
    }
}

/// 글줄 높이에 맞춘 작은 아이콘을 그 자리에 놓는다(트리·상태 표시줄용). 색은 뜻에 맞는 색, 없으면 글자색이다.
pub fn inline(ui: &mut Ui, icon: Icon) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(SIZE), Sense::hover());
    if ui.is_rect_visible(rect) {
        let color = tint(icon, ui.visuals().dark_mode).unwrap_or(ui.visuals().text_color());
        paint(ui.painter(), rect.shrink(1.0), icon, color);
    }
    response
}

/// 아이콘과 글자가 함께 있는 버튼. 접근성 라벨은 글자 그대로다.
pub fn button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    let font: FontId = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, ui.visuals().text_color());
    let pad = ui.spacing().button_padding;
    let gap = if label.is_empty() { 0.0 } else { 6.0 };
    let content = vec2(SIZE + gap + galley.size().x, SIZE.max(galley.size().y));
    let size = (content + pad * 2.0).max(ui.spacing().interact_size);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));
    if ui.is_rect_visible(rect) {
        let v = ui.style().interact(&response);
        ui.painter().rect(
            rect.expand(v.expansion),
            v.corner_radius,
            v.weak_bg_fill,
            v.bg_stroke,
            StrokeKind::Inside,
        );
        let icon_rect = Rect::from_min_size(
            pos2(rect.left() + pad.x, rect.center().y - SIZE / 2.0),
            Vec2::splat(SIZE),
        );
        let color = tint(icon, ui.visuals().dark_mode).unwrap_or(v.text_color());
        paint(ui.painter(), icon_rect.shrink(1.0), icon, color);
        if !label.is_empty() {
            let text_pos: Pos2 = pos2(
                icon_rect.right() + gap,
                rect.center().y - galley.size().y / 2.0,
            );
            ui.painter().galley(text_pos, galley, v.text_color());
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 한 아이콘을 `size` 정사각형에 그려 모양 목록을 돌려준다.
    fn shapes_of(icon: Icon, size: f32, dark: bool) -> Vec<egui::epaint::ClippedShape> {
        let ctx = egui::Context::default();
        let mut visuals = if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.dark_mode = dark;
        ctx.set_visuals(visuals);
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.painter().clone();
            let rect = Rect::from_min_size(pos2(20.0, 20.0), Vec2::splat(size));
            let color = tint(icon, dark).unwrap_or(Color32::GRAY);
            paint(&painter, rect, icon, color);
        });
        // 그리기만 보므로 글꼴 변경분은 버린다(버리지 않으면 떨어뜨릴 때 경고한다)
        out.textures_delta.clear();
        out.shapes
    }

    #[test]
    fn every_icon_draws_something_inside_its_box() {
        for dark in [true, false] {
            for size in [8.0_f32, 12.0, 16.0, 24.0, 32.0, 64.0] {
                for icon in Icon::ALL {
                    let shapes = shapes_of(icon, size, dark);
                    assert!(!shapes.is_empty(), "{icon:?}가 아무것도 그리지 않았다");
                    // 선 굵기만큼은 영역 밖으로 번져도 된다
                    let allowed = Rect::from_min_size(pos2(20.0, 20.0), Vec2::splat(size))
                        .expand(size / SIZE * 1.5 + 1.0);
                    for cs in &shapes {
                        let b = cs.shape.visual_bounding_rect();
                        assert!(
                            b.min.x >= allowed.min.x
                                && b.min.y >= allowed.min.y
                                && b.max.x <= allowed.max.x
                                && b.max.y <= allowed.max.y,
                            "{icon:?} {size}pt: 모양 {b:?}이(가) 영역 {allowed:?} 밖이다"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn icons_are_distinct_from_each_other() {
        // 같은 크기에서 모양(점 수와 영역)이 서로 같은 아이콘은 없어야 한다: 복사해 붙이다 틀린 것을 잡는다
        let mut seen: Vec<(String, Icon)> = Vec::new();
        for icon in Icon::ALL {
            let sig = format!(
                "{:?}",
                shapes_of(icon, 32.0, true)
                    .iter()
                    .map(|c| format!("{:?}", c.shape))
                    .collect::<Vec<_>>()
            );
            if let Some((_, other)) = seen.iter().find(|(s, _)| *s == sig) {
                panic!("{icon:?}와 {other:?}가 똑같이 그려진다");
            }
            seen.push((sig, icon));
        }
    }

    #[test]
    fn semantic_icons_have_their_own_colors() {
        for icon in [Icon::Error, Icon::Warning, Icon::Info] {
            assert!(tint(icon, true).is_some() && tint(icon, false).is_some());
        }
        assert_eq!(
            tint(Icon::Undo, true),
            None,
            "도구 모음 아이콘은 글자색을 따른다"
        );
    }
}
