//! LDF Studio 진입점.

// 릴리스 빌드는 콘솔 창 없이 GUI만 띄운다(디버그 빌드는 로그를 보도록 콘솔을 유지한다).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod fonts;
mod logging;
mod ui;
mod view;

use eframe::Renderer;

/// 렌더러를 고른다. 기본은 OpenGL(glow)이다: 메모리를 훨씬 적게 쓴다(1 MB LDF에서 약 200 MB, wgpu는 약 450 MB).
/// 그래픽 드라이버가 없는 가상 머신처럼 OpenGL을 쓸 수 없으면 wgpu(DX12 소프트웨어 렌더링 포함)로 다시 시도한다.
/// 환경 변수 `LDF_STUDIO_RENDERER=glow|wgpu`로 하나만 고를 수 있고,
/// `LDF_STUDIO_FORCE_GLOW_FAIL=1`은 glow 시작 실패를 흉내 내 대체 경로를 시험하는 데 쓴다.
fn renderers() -> Vec<Renderer> {
    match std::env::var("LDF_STUDIO_RENDERER").as_deref() {
        Ok("glow") => vec![Renderer::Glow],
        Ok("wgpu") => vec![Renderer::Wgpu],
        _ => vec![Renderer::Glow, Renderer::Wgpu],
    }
}

fn run(renderer: Renderer, fail: bool) -> eframe::Result {
    let options = eframe::NativeOptions {
        renderer,
        viewport: egui::ViewportBuilder::default()
            .with_title("LDF Studio")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([960.0, 600.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "LDF Studio",
        options,
        Box::new(move |cc| {
            if fail {
                return Err("glow 시작 실패(시험용)".into());
            }
            Ok(Box::new(app::StudioApp::new(cc)))
        }),
    )
}

fn main() -> eframe::Result {
    // 로그 가드는 앱이 끝날 때까지 들고 있어야 마지막 줄까지 기록된다
    let _log = logging::init();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "LDF Studio 시작");

    let list = renderers();
    let last = list.len().saturating_sub(1);
    let force_fail = std::env::var_os("LDF_STUDIO_FORCE_GLOW_FAIL").is_some();
    let mut result = Ok(());
    for (i, r) in list.into_iter().enumerate() {
        let fail = force_fail && matches!(r, Renderer::Glow);
        result = run(r, fail);
        match &result {
            Ok(()) => break,
            Err(_) if i < last => {
                tracing::warn!("렌더러 시작에 실패해 다른 렌더러로 다시 시도한다");
            }
            Err(_) => {}
        }
    }
    result
}
