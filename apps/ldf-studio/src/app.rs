//! 앱 본체: 엔진 이벤트 처리, 입력, 패널 배치.

use crate::fonts;
use crate::ui::{UiState, bits, context, nav, props, schedule, status};
use crate::view::{self, Selection};
use eframe::Storage;
use egui::{Key, Modifiers, ScrollArea, Ui};
use egui_extras::{Size, StripBuilder};
use ldf_engine::{Command, Engine, Event, ModelSnapshot, RevisionInfo};
use ldf_model::FrameId;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

const SETTINGS_KEY: &str = "ldf-studio-settings";
const MAX_RECENT: usize = 10;

/// 재시작해도 유지하는 설정(FR-IMP-08). LDF 데이터는 여기에 저장하지 않는다.
#[derive(Serialize, Deserialize, Clone, Debug)]
struct Settings {
    hex: bool,
    dark: bool,
    recent: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hex: false,
            dark: true,
            recent: Vec::new(),
        }
    }
}

enum Picked {
    Ldf(PathBuf),
    Project(PathBuf),
}

/// LDF Studio 앱.
pub struct StudioApp {
    engine: Engine,
    snapshot: Option<Arc<ModelSnapshot>>,
    revisions: Vec<RevisionInfo>,
    project_path: Option<PathBuf>,
    st: UiState,
    settings: Settings,
    notice: Option<String>,
    fatal: Option<String>,
    busy: bool,
    picker: Option<Receiver<Option<Picked>>>,
    title: String,
    /// LDF_STUDIO_PERF=1이면 상태 바에 프레임당 CPU 시간을 보여준다(NFR-01 확인용).
    perf: Option<f32>,
    show_perf: bool,
}

/// 프로젝트 DB의 기본 위치: `%LOCALAPPDATA%\LDFStudio\default.ldfdb`.
/// 환경 변수 `LDF_STUDIO_PROJECT`로 바꿀 수 있다(시험·시연용).
fn default_project_path() -> PathBuf {
    if let Some(p) = std::env::var_os("LDF_STUDIO_PROJECT") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("LOCALAPPDATA").map_or_else(std::env::temp_dir, PathBuf::from);
    base.join("LDFStudio").join("default.ldfdb")
}

fn spawn_engine(ctx: &egui::Context, project: PathBuf) -> Engine {
    let ctx = ctx.clone();
    Engine::spawn(Some(project), move || ctx.request_repaint())
}

impl StudioApp {
    /// 앱을 만든다. 글꼴, 설정 복원, 엔진 시작, 명령줄 인자로 받은 파일 임포트.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        fonts::install(&cc.egui_ctx);
        let settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, SETTINGS_KEY))
            .unwrap_or_default();
        cc.egui_ctx.set_theme(if settings.dark {
            egui::ThemePreference::Dark
        } else {
            egui::ThemePreference::Light
        });

        let project = default_project_path();
        let engine = spawn_engine(&cc.egui_ctx, project.clone());
        let mut app = Self {
            engine,
            snapshot: None,
            revisions: Vec::new(),
            project_path: Some(project),
            st: UiState::default(),
            settings,
            notice: None,
            fatal: None,
            busy: false,
            picker: None,
            title: String::new(),
            perf: None,
            show_perf: std::env::var_os("LDF_STUDIO_PERF").is_some(),
        };
        if let Some(arg) = std::env::args_os().nth(1) {
            app.import(PathBuf::from(arg));
        }
        app
    }

    fn send(&mut self, cmd: Command) {
        if self.engine.send(cmd).is_err() {
            self.fatal = Some("엔진 작업자가 종료되었다".to_owned());
            self.busy = false;
        }
    }

    fn import(&mut self, path: PathBuf) {
        self.remember_recent(&path);
        self.busy = true;
        self.send(Command::Import(path));
    }

    fn remember_recent(&mut self, path: &Path) {
        self.settings.recent.retain(|p| p != path);
        self.settings.recent.insert(0, path.to_owned());
        self.settings.recent.truncate(MAX_RECENT);
    }

    /// 파일 대화상자는 별도 스레드에서 띄워 UI 프레임을 막지 않는다.
    fn pick(&mut self, ctx: &egui::Context, project: bool) {
        if self.picker.is_some() {
            return;
        }
        let (tx, rx) = channel();
        self.picker = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let dialog = rfd::FileDialog::new();
            let picked = if project {
                dialog
                    .add_filter("LDF Studio 프로젝트", &["ldfdb"])
                    .set_title("프로젝트 열기 또는 만들기")
                    .save_file()
                    .map(Picked::Project)
            } else {
                dialog
                    .add_filter("LIN Description File", &["ldf"])
                    .add_filter("모든 파일", &["*"])
                    .set_title("LDF 열기")
                    .pick_file()
                    .map(Picked::Ldf)
            };
            let _ = tx.send(picked);
            ctx.request_repaint();
        });
    }

    fn poll_picker(&mut self) {
        let Some(rx) = &self.picker else { return };
        match rx.try_recv() {
            Ok(picked) => {
                self.picker = None;
                match picked {
                    Some(Picked::Ldf(p)) => self.import(p),
                    Some(Picked::Project(p)) => {
                        self.send(Command::OpenProject(p));
                        self.snapshot = None;
                        self.st.reset_for_new_model();
                    }
                    None => {}
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.picker = None,
        }
    }

    fn poll_engine(&mut self, ctx: &egui::Context) {
        while let Some(ev) = self.engine.try_recv() {
            match ev {
                Event::ProjectOpened { path, revisions } => {
                    self.project_path = path;
                    self.revisions = revisions;
                }
                Event::Loaded {
                    snapshot,
                    revisions,
                    reused,
                } => {
                    self.busy = false;
                    self.revisions = revisions;
                    self.st.reset_for_new_model();
                    self.set_title(ctx, &snapshot);
                    if reused {
                        self.notice =
                            Some("같은 내용의 리비전이 이미 있어 그것을 열었다".to_owned());
                    }
                    self.snapshot = Some(snapshot);
                }
                Event::Revisions(r) => self.revisions = r,
                Event::Failed(msg) => {
                    self.busy = false;
                    self.notice = Some(msg);
                }
                Event::Fatal(msg) => {
                    self.busy = false;
                    self.fatal = Some(msg);
                }
            }
        }
    }

    fn set_title(&mut self, ctx: &egui::Context, snap: &ModelSnapshot) {
        let name = snap
            .revision
            .source_name
            .clone()
            .unwrap_or_else(|| snap.revision.label.clone());
        let title = format!("LDF Studio — {name} (리비전 {})", snap.revision.id);
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn handle_input(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::O)) {
            self.pick(&ctx, false);
        }
        if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F)) {
            self.st.focus_search = true;
        }
        let dropped: Vec<PathBuf> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        for p in dropped {
            self.import(p);
        }
    }

    fn menu_bar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("파일", |ui| {
                if ui.button("LDF 열기…    Ctrl+O").clicked() {
                    self.pick(&ctx, false);
                    ui.close();
                }
                if ui.button("프로젝트 열기/만들기…").clicked() {
                    self.pick(&ctx, true);
                    ui.close();
                }
                ui.add_enabled_ui(!self.settings.recent.is_empty(), |ui| {
                    ui.menu_button("최근 파일", |ui| {
                        let recent = self.settings.recent.clone();
                        for p in recent {
                            let label = p.file_name().map_or_else(
                                || p.display().to_string(),
                                |n| n.to_string_lossy().into_owned(),
                            );
                            if ui
                                .button(label)
                                .on_hover_text(p.display().to_string())
                                .clicked()
                            {
                                self.import(p);
                                ui.close();
                            }
                        }
                    });
                });
                ui.separator();
                if ui.button("종료").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("보기", |ui| {
                ui.checkbox(&mut self.settings.hex, "16진수로 표시");
                if ui.checkbox(&mut self.settings.dark, "다크 테마").changed() {
                    ctx.set_theme(if self.settings.dark {
                        egui::ThemePreference::Dark
                    } else {
                        egui::ThemePreference::Light
                    });
                }
            });
            ui.menu_button("도움말", |ui| {
                ui.label("LDF Studio — LIN Description File 뷰어·편집기");
                ui.weak("M1: 읽기 전용 뷰어");
                if let Some(p) = &self.project_path {
                    ui.separator();
                    ui.weak(format!("프로젝트: {}", p.display()));
                }
            });
        });
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            if ui.button("LDF 열기").clicked() {
                self.pick(&ctx, false);
            }
            ui.separator();
            ui.toggle_value(&mut self.settings.hex, "hex")
                .on_hover_text("숫자를 16진수로 표시");
            ui.separator();
            self.revision_combo(ui);
        });
    }

    fn revision_combo(&mut self, ui: &mut Ui) {
        if self.revisions.is_empty() {
            return;
        }
        let current = self.snapshot.as_ref().map(|s| s.revision.id);
        let label = |r: &RevisionInfo| format!("#{} {} ({})", r.id, r.label, r.created_at);
        let selected = self
            .revisions
            .iter()
            .find(|r| Some(r.id) == current)
            .map_or_else(|| "리비전".to_owned(), label);
        let mut open: Option<i64> = None;
        egui::ComboBox::from_id_salt("revision-combo")
            .selected_text(selected)
            .width(280.0)
            .show_ui(ui, |ui| {
                for r in self.revisions.iter().rev() {
                    if ui
                        .selectable_label(Some(r.id) == current, label(r))
                        .clicked()
                    {
                        open = Some(r.id);
                    }
                }
            });
        if let Some(id) = open
            && Some(id) != current
        {
            self.busy = true;
            self.send(Command::OpenRevision(id));
        }
    }

    fn notice_bar(&mut self, ui: &mut Ui) {
        let Some(msg) = self.notice.clone() else {
            return;
        };
        egui::Panel::top("notice").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.colored_label(ui.visuals().warn_fg_color, msg);
                if ui.small_button("닫기").clicked() {
                    self.notice = None;
                }
            });
        });
    }

    fn welcome(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.3);
                ui.heading("LDF Studio");
                ui.add_space(8.0);
                ui.label("LDF 파일을 이 창에 끌어다 놓거나 아래 버튼으로 여세요.");
                ui.add_space(12.0);
                if ui.button("LDF 열기…").clicked() {
                    self.pick(&ctx, false);
                }
                if !self.revisions.is_empty() {
                    ui.add_space(16.0);
                    ui.weak("이 프로젝트에 저장된 리비전:");
                    let revs: Vec<RevisionInfo> = self.revisions.iter().rev().cloned().collect();
                    for r in revs {
                        if ui
                            .link(format!("#{} {}  ({})", r.id, r.label, r.created_at))
                            .clicked()
                        {
                            self.busy = true;
                            self.send(Command::OpenRevision(r.id));
                        }
                    }
                }
            });
        });
    }

    fn fatal_screen(&mut self, ui: &mut Ui, msg: &str) {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.3);
                ui.heading("엔진이 중단되었다");
                ui.colored_label(ui.visuals().error_fg_color, msg);
                ui.add_space(8.0);
                if ui.button("엔진 다시 시작").clicked() {
                    self.engine = spawn_engine(
                        &ctx,
                        self.project_path
                            .clone()
                            .unwrap_or_else(default_project_path),
                    );
                    self.fatal = None;
                }
            });
        });
    }

    fn main_view(&mut self, ui: &mut Ui, snap: &Arc<ModelSnapshot>) {
        let hex = self.settings.hex;

        egui::Panel::left("nav")
            .default_size(290.0)
            .size_range(200.0..=560.0)
            .show(ui, |ui| {
                nav::show(ui, snap, &mut self.st, hex);
            });

        if let Selection::Schedule(s) | Selection::Slot(s, _) = &self.st.selection {
            self.st.last_schedule = Some(*s);
        }

        egui::CentralPanel::default().show(ui, |ui| {
            // 속성 창 내용은 패널을 그리기 전에 계산해 두어, 표가 선택 상태를 바꿀 수 있게 한다
            let sched_props = {
                let sel = match (&self.st.selection, self.st.last_schedule) {
                    (s @ (Selection::Schedule(_) | Selection::Slot(..)), _) => s.clone(),
                    (_, Some(last)) => Selection::Schedule(last),
                    _ => Selection::None,
                };
                view::props::schedule_props(&snap.model, &sel)
            };
            let height = ui.available_height();

            egui::Panel::top("schedule-region")
                .resizable(true)
                .default_size(height * 0.42)
                .size_range(110.0..=height.max(200.0))
                .show(ui, |ui| {
                    split(
                        ui,
                        |ui| schedule::show(ui, snap, &mut self.st),
                        |ui| match &sched_props {
                            Some(sec) => props::section(ui, "sched-props", sec),
                            None => props::placeholder(
                                ui,
                                "Schedule Properties",
                                "스케줄 테이블이나 슬롯을 선택하세요",
                            ),
                        },
                    );
                });

            let obj_props = view::props::object_props(&snap.model, &self.st.selection, hex);
            let frame_for_bits: Option<FrameId> = match &self.st.selection {
                Selection::Frame(f) => Some(*f),
                Selection::Slot(s, i) => view::props::slot_frame(&snap.model, *s, *i),
                _ => None,
            };
            split(
                ui,
                |ui| context::show(ui, snap, &mut self.st, hex),
                |ui| {
                    match &obj_props {
                        Some(sec) => props::section(ui, "obj-props", sec),
                        None => props::placeholder(ui, "Properties", "항목을 선택하세요"),
                    }
                    if let Some(f) = frame_for_bits {
                        bits::show(ui, &snap.model, f);
                    }
                },
            );
        });
    }
}

/// 왼쪽에 본 뷰, 오른쪽에 속성 창을 두는 가로 분할.
fn split(ui: &mut Ui, left: impl FnOnce(&mut Ui), right: impl FnOnce(&mut Ui)) {
    StripBuilder::new(ui)
        .size(Size::remainder().at_least(220.0))
        .size(Size::exact(310.0))
        .horizontal(|mut strip| {
            strip.cell(left);
            strip.cell(|ui| {
                ScrollArea::vertical().auto_shrink(false).show(ui, right);
            });
        });
}

impl eframe::App for StudioApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let started = std::time::Instant::now();
        let ctx = ui.ctx().clone();
        self.poll_engine(&ctx);
        self.poll_picker();
        self.handle_input(ui);

        egui::Panel::top("menu").show(ui, |ui| {
            self.menu_bar(ui);
            self.toolbar(ui);
        });
        self.notice_bar(ui);

        let snap = self.snapshot.clone();
        egui::Panel::bottom("status").show(ui, |ui| {
            status::show(ui, snap.as_deref(), self.busy, self.perf);
        });

        if let Some(msg) = self.fatal.clone() {
            self.fatal_screen(ui, &msg);
        } else if let Some(snap) = snap {
            self.main_view(ui, &snap);
        } else {
            self.welcome(ui);
        }
        if self.show_perf {
            let ms = started.elapsed().as_secs_f32() * 1000.0;
            self.perf = Some(self.perf.map_or(ms, |p| p * 0.9 + ms * 0.1));
            ctx.request_repaint();
        }
    }

    fn save(&mut self, storage: &mut dyn Storage) {
        eframe::set_value(storage, SETTINGS_KEY, &self.settings);
    }
}
