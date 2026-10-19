//! 앱 본체: 엔진 이벤트 처리, 입력, 패널 배치.

use crate::fonts;
use crate::i18n::{self, Lang, t, tr};
use crate::ui::checker::CheckerAction;
use crate::ui::edit::{DeleteUi, Dialog, Outcome};
use crate::ui::export::{Blocked, ExportAction};
use crate::ui::revisions::{DiffShown, RevAction};
use crate::ui::{UiState, bits, context, nav, props, schedule, status};
use crate::view::edit::{
    CompositionUi, EncodingUi, FrameUi, InlineScope, NodeUi, ScheduleUi, SignalUi,
};
use crate::view::{self, SelKey, Selection};
use eframe::Storage;
use egui::{Key, Modifiers, ScrollArea, Ui};
use egui_extras::{Size, StripBuilder};
use ldf_engine::{
    Command, EditOp, Engine, Event, ExportFormat, ExportOptions, ModelSnapshot, RevisionInfo,
    RuleSettings,
};
use ldf_model::FrameId;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

const SETTINGS_KEY: &str = "ldf-studio-settings";
const MAX_RECENT: usize = 10;

/// 재시작해도 유지하는 설정(FR-IMP-08). LDF 데이터는 여기에 저장하지 않는다.
#[derive(Serialize, Deserialize, Clone, Debug)]
struct Settings {
    hex: bool,
    dark: bool,
    /// 화면 언어. 옛 설정에는 없으므로 없으면 한국어다.
    #[serde(default)]
    lang: Lang,
    recent: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hex: false,
            dark: true,
            lang: Lang::default(),
            recent: Vec::new(),
        }
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Open,
    Project,
    Export,
}

enum Picked {
    Ldf(PathBuf),
    Project(PathBuf),
    /// 내보낼 파일 경로(옵션은 xport_pending).
    Export(PathBuf),
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
    /// 파일 선택을 기다리는 내보내기 옵션.
    export_pending: Option<ExportOptions>,
    /// 백그라운드 작업(CSV 저장 등)의 결과 메시지.
    msg_tx: Sender<String>,
    msg_rx: Receiver<String>,
    title: String,
    /// LDF_STUDIO_PERF=1이면 상태 바에 프레임당 CPU 시간을 보여준다(NFR-01 확인용).
    perf: Option<f32>,
    show_perf: bool,
    /// 편집이 끝나면 선택할 객체(새로 만든 항목).
    select_after_edit: Option<SelKey>,
    /// 저장 안 한 변경이 있어 확인을 기다리는 동작(FR-EDT-09).
    unsaved: Option<Pending>,
    /// 저장이 끝나면 이어서 할 동작.
    after_save: Option<Pending>,
    /// 저장 라벨 입력 대화상자(열려 있으면 입력 중인 라벨).
    save_label: Option<String>,
    /// 변경을 버리고 닫기로 확정했는지.
    allow_close: bool,
    /// 이벤트 처리 밖(동작 수행)에서 창 명령을 보내기 위한 핸들.
    ctx: Option<egui::Context>,
    /// 프로젝트의 규칙 설정.
    rules: RuleSettings,
}

/// 새로 만들 수 있는 객체 종류.
#[derive(Clone, Copy)]
enum NewKind {
    Unconditional,
    EventTriggered,
    Sporadic,
    Signal,
    Slave,
    Schedule,
    Encoding,
}

/// 저장 안 한 변경이 있으면 확인을 거쳐야 하는 동작.
#[derive(Clone, Debug)]
enum Pending {
    /// LDF 파일을 연다.
    Import(PathBuf),
    /// 프로젝트 파일을 연다.
    OpenProject(PathBuf),
    /// 리비전을 연다.
    OpenRevision(i64),
    /// 앱을 닫는다.
    Quit,
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
        i18n::set_lang(settings.lang);
        cc.egui_ctx.set_theme(if settings.dark {
            egui::ThemePreference::Dark
        } else {
            egui::ThemePreference::Light
        });

        let (msg_tx, msg_rx) = channel();
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
            export_pending: None,
            msg_tx,
            msg_rx,
            title: String::new(),
            perf: None,
            show_perf: std::env::var_os("LDF_STUDIO_PERF").is_some(),
            select_after_edit: None,
            unsaved: None,
            after_save: None,
            save_label: None,
            allow_close: false,
            ctx: Some(cc.egui_ctx.clone()),
            rules: RuleSettings::default(),
        };
        if let Some(arg) = std::env::args_os().nth(1) {
            app.import(PathBuf::from(arg));
        }
        app
    }

    fn send(&mut self, cmd: Command) {
        if self.engine.send(cmd).is_err() {
            self.fatal = Some(t("엔진 작업자가 종료되었다").to_owned());
            self.busy = false;
        }
    }

    fn import(&mut self, path: PathBuf) {
        self.remember_recent(&path);
        self.busy = true;
        self.send(Command::Import(path));
    }

    fn dirty(&self) -> bool {
        self.snapshot.as_ref().is_some_and(|s| s.dirty)
    }

    /// 변경이 있으면 확인을 받은 뒤, 없으면 바로 `act`를 한다.
    fn request(&mut self, act: Pending) {
        if self.dirty() {
            self.unsaved = Some(act);
        } else {
            self.perform(act);
        }
    }

    fn perform(&mut self, act: Pending) {
        match act {
            Pending::Import(p) => self.import(p),
            Pending::OpenProject(p) => {
                self.send(Command::OpenProject(p));
                self.snapshot = None;
                self.st.reset_for_new_model();
            }
            Pending::OpenRevision(id) => {
                self.busy = true;
                self.send(Command::OpenRevision(id));
            }
            Pending::Quit => {
                self.allow_close = true;
                if let Some(ctx) = self.ctx.clone() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    fn edit(&mut self, op: EditOp) {
        self.busy = true;
        self.send(Command::Edit(op));
    }

    /// 선택한 항목의 편집 대화상자를 연다.
    fn open_edit(&mut self) {
        let Some(snap) = self.snapshot.clone() else {
            return;
        };
        let m = &snap.model;
        let dlg = match &self.st.selection {
            Selection::Frame(id) => match FrameUi::from_frame(m, *id) {
                Some(d) => Some(Dialog::Frame(d)),
                None => {
                    self.notice = Some(t("진단 프레임은 편집할 수 없다").to_owned());
                    None
                }
            },
            Selection::Signal(id) => SignalUi::from_signal(m, *id).map(Dialog::Signal),
            Selection::Node(id) => NodeUi::from_node(m, *id).map(Dialog::Node),
            Selection::Schedule(id) | Selection::Slot(id, _) => {
                ScheduleUi::from_schedule(m, *id).map(Dialog::Schedule)
            }
            _ => None,
        };
        if dlg.is_some() {
            self.st.edit.dialog = dlg;
        }
    }

    /// 선택한 항목의 삭제 확인을 연다(함께 지워지는 항목을 미리 보여 준다).
    fn open_delete(&mut self) {
        let Some(snap) = self.snapshot.clone() else {
            return;
        };
        let m = &snap.model;
        let target = match &self.st.selection {
            Selection::Frame(id) => m.frames.get(*id).map(|f| {
                (
                    tf!("프레임 `{}`", f.name),
                    EditOp::DeleteFrame(f.name.clone()),
                )
            }),
            Selection::Signal(id) => m.signals.get(*id).map(|s| {
                (
                    tf!("시그널 `{}`", s.name),
                    EditOp::DeleteSignal(s.name.clone()),
                )
            }),
            Selection::Node(id) => m
                .nodes
                .get(*id)
                .filter(|n| n.role == ldf_model::NodeRole::Slave)
                .map(|n| (tf!("노드 `{}`", n.name), EditOp::DeleteNode(n.name.clone()))),
            Selection::Schedule(id) | Selection::Slot(id, _) => m.schedules.get(*id).map(|t| {
                (
                    tf!("스케줄 테이블 `{}`", t.name),
                    EditOp::DeleteSchedule(t.name.clone()),
                )
            }),
            _ => None,
        };
        if let Some((what, op)) = target
            && let Some(d) = DeleteUi::new(m, what, op)
        {
            self.st.edit.dialog = Some(Dialog::Delete(d));
        }
    }

    /// 노드 구성(composite) 대화상자를 연다.
    fn open_composition(&mut self) {
        if let Some(snap) = &self.snapshot {
            self.st.edit.dialog = Some(Dialog::Composition(CompositionUi::from_model(&snap.model)));
        }
    }

    /// 새 객체를 만드는 대화상자를 연다(FR-EDT-04).
    fn open_new(&mut self, kind: NewKind) {
        let Some(snap) = self.snapshot.clone() else {
            return;
        };
        let m = &snap.model;
        match kind {
            NewKind::Unconditional => {
                self.st.edit.dialog = Some(Dialog::Frame(FrameUi::new_unconditional(m)));
            }
            NewKind::EventTriggered => {
                self.st.edit.dialog = Some(Dialog::Frame(FrameUi::new_event_triggered(m)));
            }
            NewKind::Sporadic => {
                self.st.edit.dialog = Some(Dialog::Frame(FrameUi::new_sporadic(m)));
            }
            NewKind::Signal => self.st.edit.dialog = Some(Dialog::Signal(SignalUi::new(m))),
            NewKind::Slave => self.st.edit.dialog = Some(Dialog::Node(NodeUi::new_slave(m))),
            NewKind::Schedule => {
                self.st.edit.dialog = Some(Dialog::Schedule(ScheduleUi::new(m)));
            }
            NewKind::Encoding => self.st.edit.encoding = Some(EncodingUi::new(m)),
        }
    }

    fn edit_window(&mut self, ctx: &egui::Context, snap: &Arc<ModelSnapshot>) {
        if let Outcome::Apply { op, select } = crate::ui::edit::show(ctx, snap, &mut self.st.edit) {
            self.select_after_edit = select;
            self.edit(*op);
        }
    }

    fn undo(&mut self) {
        if self.snapshot.as_ref().is_some_and(|s| s.can_undo) {
            self.send(Command::Undo);
        }
    }

    fn redo(&mut self) {
        if self.snapshot.as_ref().is_some_and(|s| s.can_redo) {
            self.send(Command::Redo);
        }
    }

    /// 저장 라벨을 묻는 대화상자를 연다(FR-REV-02). 확인하면 새 리비전으로 저장한다.
    fn save_edits(&mut self) {
        if self.dirty() {
            self.save_label = Some(String::new());
        }
    }

    fn save_window(&mut self, ctx: &egui::Context) {
        let Some(mut label) = self.save_label.take() else {
            return;
        };
        let mut keep = true;
        egui::Window::new(t("새 리비전으로 저장"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(t("이 리비전의 라벨(비우면 출처 파일 이름)"));
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut label)
                        .hint_text(t("예: 공급사 회신 반영"))
                        .desired_width(280.0),
                );
                edit.request_focus();
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(t("저장")).clicked() || enter {
                        let text = label.trim();
                        self.busy = true;
                        self.send(Command::SaveEdits {
                            label: (!text.is_empty()).then(|| text.to_owned()),
                        });
                        keep = false;
                    }
                    if ui.button(t("취소")).clicked() {
                        self.after_save = None;
                        keep = false;
                    }
                });
            });
        if keep {
            self.save_label = Some(label);
        }
    }

    fn remember_recent(&mut self, path: &Path) {
        self.settings.recent.retain(|p| p != path);
        self.settings.recent.insert(0, path.to_owned());
        self.settings.recent.truncate(MAX_RECENT);
    }

    /// 파일 대화상자는 별도 스레드에서 띄워 UI 프레임을 막지 않는다.
    fn pick(&mut self, ctx: &egui::Context, project: bool) {
        self.pick_with(ctx, if project { Mode::Project } else { Mode::Open });
    }

    fn pick_with(&mut self, ctx: &egui::Context, mode: Mode) {
        if self.picker.is_some() {
            return;
        }
        let (tx, rx) = channel();
        self.picker = Some(rx);
        let ctx = ctx.clone();
        let json = self
            .export_pending
            .as_ref()
            .is_some_and(|o| o.format == ExportFormat::Json);
        std::thread::spawn(move || {
            let dialog = rfd::FileDialog::new();
            let picked = match mode {
                Mode::Project => dialog
                    .add_filter(t("LDF Studio 프로젝트"), &["ldfdb"])
                    .set_title(t("프로젝트 열기 또는 만들기"))
                    .save_file()
                    .map(Picked::Project),
                Mode::Open => dialog
                    .add_filter("LIN Description File", &["ldf"])
                    .add_filter(t("모든 파일"), &["*"])
                    .set_title(t("LDF 열기"))
                    .pick_file()
                    .map(Picked::Ldf),
                Mode::Export if json => dialog
                    .add_filter("JSON", &["json"])
                    .set_title(t("모델을 JSON으로 내보내기"))
                    .save_file()
                    .map(Picked::Export),
                Mode::Export => dialog
                    .add_filter("LIN Description File", &["ldf"])
                    .set_title(t("LDF로 내보내기"))
                    .save_file()
                    .map(Picked::Export),
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
                    Some(Picked::Ldf(p)) => self.request(Pending::Import(p)),
                    Some(Picked::Project(p)) => self.request(Pending::OpenProject(p)),
                    Some(Picked::Export(path)) => self.start_export(path),
                    None => self.export_pending = None,
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.picker = None,
        }
    }

    fn start_export(&mut self, path: PathBuf) {
        let (Some(options), Some(snap)) = (self.export_pending.take(), self.snapshot.as_ref())
        else {
            return;
        };
        let revision = snap.revision.id;
        self.busy = true;
        self.send(Command::Export {
            revision,
            path,
            options,
        });
    }

    fn poll_engine(&mut self, ctx: &egui::Context) {
        while let Some(ev) = self.engine.try_recv() {
            match ev {
                Event::ProjectOpened {
                    path,
                    revisions,
                    rules,
                } => {
                    self.project_path = path;
                    self.revisions = revisions;
                    self.rules = rules;
                }
                Event::Loaded {
                    snapshot,
                    revisions,
                    reused,
                } => {
                    self.busy = false;
                    self.revisions = revisions;
                    self.st.reset_for_new_model();
                    self.select_after_edit = None;
                    self.set_title(ctx, &snapshot);
                    if reused {
                        self.notice =
                            Some(t("같은 내용의 리비전이 이미 있어 그것을 열었다").to_owned());
                    }
                    self.snapshot = Some(snapshot);
                }
                Event::Revisions(r) => self.revisions = r,
                Event::RuleSettings(rules) => self.rules = rules,
                Event::Diffed {
                    before,
                    after,
                    diff,
                } => {
                    self.busy = false;
                    self.st.revisions.diff = Some(DiffShown::new(before, after, diff));
                }
                Event::RevisionDeleted {
                    closed, revisions, ..
                } => {
                    self.busy = false;
                    self.revisions = revisions;
                    self.st.revisions.diff = None;
                    if closed {
                        self.snapshot = None;
                        self.title.clear();
                        self.st.reset_for_new_model();
                        if let Some(ctx) = &self.ctx {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Title(
                                "LDF Studio".to_owned(),
                            ));
                        }
                    }
                    self.notice = Some(t("리비전을 삭제했다").to_owned());
                }
                Event::Edited(snapshot) => {
                    self.busy = false;
                    self.adopt_edit(ctx, snapshot);
                }
                Event::Saved {
                    snapshot,
                    revisions,
                } => {
                    self.busy = false;
                    self.revisions = revisions;
                    self.adopt_edit(ctx, snapshot);
                    self.notice = Some(t("편집 내용을 새 리비전으로 저장했다").to_owned());
                    if let Some(next) = self.after_save.take() {
                        self.perform(next);
                    }
                }
                Event::Exported(r) => {
                    self.busy = false;
                    let mut msg = tf!(
                        "내보냈다: {} ({} 바이트, {})",
                        r.path.display(),
                        r.bytes,
                        r.encoding
                    );
                    if !r.verified && r.warnings.is_empty() {
                        msg.push_str(t(" — 다시 읽은 결과가 원본과 다르다"));
                    }
                    if !r.warnings.is_empty() {
                        let warnings: Vec<_> = r.warnings.iter().map(|w| tr(w)).collect();
                        msg.push_str(&tf!(" — 경고: {}", warnings.join("; ")));
                    }
                    self.notice = Some(msg);
                }
                Event::ExportBlocked {
                    revision,
                    path,
                    options,
                    errors,
                } => {
                    self.busy = false;
                    self.st.export.blocked = Some(Blocked {
                        revision,
                        path,
                        options,
                        errors,
                    });
                }
                Event::Failed(msg) => {
                    self.busy = false;
                    self.notice = Some(tr(&msg).into_owned());
                }
                Event::Fatal(msg) => {
                    self.busy = false;
                    self.fatal = Some(tr(&msg).into_owned());
                }
            }
        }
    }

    /// 편집·Undo·Redo·저장으로 새 스냅샷이 오면 보던 화면(선택, 트리)을 이름으로 이어 간다.
    fn adopt_edit(&mut self, ctx: &egui::Context, snapshot: Arc<ModelSnapshot>) {
        let key = self.select_after_edit.take().or_else(|| {
            self.snapshot
                .as_ref()
                .map(|old| self.st.selection.key(&old.model))
        });
        if let Some(key) = key {
            self.st.selection = key.resolve(&snapshot.model);
        }
        self.st.after_model_edit(&snapshot.model);
        self.set_title(ctx, &snapshot);
        self.snapshot = Some(snapshot);
    }

    fn set_title(&mut self, ctx: &egui::Context, snap: &ModelSnapshot) {
        let name = snap
            .revision
            .source_name
            .clone()
            .unwrap_or_else(|| snap.revision.label.clone());
        let mark = if snap.dirty { "*" } else { "" };
        let title = tf!("{mark}LDF Studio — {name} (리비전 {})", snap.revision.id);
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
        // 입력칸에 포커스가 있으면 Ctrl+Z 등은 그 칸의 몫이다
        if !ctx.egui_wants_keyboard_input() {
            if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)) {
                self.redo();
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Z)) {
                self.undo();
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Y)) {
                self.redo();
            }
        }
        if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S)) {
            self.save_edits();
        }
        if !ctx.egui_wants_keyboard_input()
            && self.st.edit.dialog.is_none()
            && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Delete))
        {
            self.open_delete();
        }
        if std::mem::take(&mut self.st.activate) && self.st.edit.dialog.is_none() {
            self.open_edit();
        }
        if std::mem::take(&mut self.st.delete) && self.st.edit.dialog.is_none() {
            self.open_delete();
        }
        let dropped: Vec<PathBuf> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        for p in dropped {
            self.request(Pending::Import(p));
        }
    }

    fn menu_bar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let mut lang_changed = false;
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(t("파일"), |ui| {
                if ui.button(t("LDF 열기…    Ctrl+O")).clicked() {
                    self.pick(&ctx, false);
                    ui.close();
                }
                if ui.button(t("프로젝트 열기/만들기…")).clicked() {
                    self.pick(&ctx, true);
                    ui.close();
                }
                ui.add_enabled_ui(!self.settings.recent.is_empty(), |ui| {
                    ui.menu_button(t("최근 파일"), |ui| {
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
                                self.request(Pending::Import(p));
                                ui.close();
                            }
                        }
                    });
                });
                if ui
                    .add_enabled(
                        !self.revisions.is_empty(),
                        egui::Button::new(t("리비전·비교…")),
                    )
                    .clicked()
                {
                    self.st.revisions.open = true;
                    ui.close();
                }
                ui.separator();
                if ui
                    .add_enabled(self.snapshot.is_some(), egui::Button::new(t("내보내기…")))
                    .clicked()
                {
                    self.st.export.open = true;
                    ui.close();
                }
                ui.separator();
                if ui.button(t("종료")).clicked() {
                    self.request(Pending::Quit);
                }
            });
            ui.menu_button(t("편집"), |ui| {
                let (can_undo, can_redo, dirty) = self
                    .snapshot
                    .as_ref()
                    .map_or((false, false, false), |s| (s.can_undo, s.can_redo, s.dirty));
                if ui
                    .add_enabled(can_undo, egui::Button::new(t("실행 취소    Ctrl+Z")))
                    .clicked()
                {
                    self.undo();
                    ui.close();
                }
                if ui
                    .add_enabled(can_redo, egui::Button::new(t("다시 실행    Ctrl+Y")))
                    .clicked()
                {
                    self.redo();
                    ui.close();
                }
                ui.separator();
                let has = self.snapshot.is_some();
                if ui
                    .add_enabled(has, egui::Button::new(t("선택 항목 편집…    Enter")))
                    .clicked()
                {
                    self.open_edit();
                    ui.close();
                }
                if ui
                    .add_enabled(has, egui::Button::new(t("선택 항목 삭제…    Delete")))
                    .clicked()
                {
                    self.open_delete();
                    ui.close();
                }
                ui.add_enabled_ui(has, |ui| {
                    ui.menu_button(t("새로 만들기"), |ui| {
                        for (label, kind) in [
                            (t("무조건 프레임"), NewKind::Unconditional),
                            (t("이벤트 트리거 프레임"), NewKind::EventTriggered),
                            (t("스포라딕 프레임"), NewKind::Sporadic),
                            (t("시그널"), NewKind::Signal),
                            (t("슬레이브 노드"), NewKind::Slave),
                            (t("스케줄 테이블"), NewKind::Schedule),
                            (t("인코딩 타입"), NewKind::Encoding),
                        ] {
                            if ui.button(label).clicked() {
                                self.open_new(kind);
                                ui.close();
                            }
                        }
                    });
                });
                if ui
                    .add_enabled(has, egui::Button::new(t("노드 구성…")))
                    .clicked()
                {
                    self.open_composition();
                    ui.close();
                }
                ui.separator();
                if ui
                    .add_enabled(dirty, egui::Button::new(t("편집 저장    Ctrl+S")))
                    .on_hover_text(t("편집한 내용을 새 리비전으로 저장한다"))
                    .clicked()
                {
                    self.save_edits();
                    ui.close();
                }
            });
            ui.menu_button(t("보기"), |ui| {
                ui.checkbox(&mut self.settings.hex, t("16진수로 표시"));
                if ui
                    .checkbox(&mut self.settings.dark, t("다크 테마"))
                    .changed()
                {
                    ctx.set_theme(if self.settings.dark {
                        egui::ThemePreference::Dark
                    } else {
                        egui::ThemePreference::Light
                    });
                }
                ui.menu_button(t("언어 / Language"), |ui| {
                    for lang in [Lang::Ko, Lang::En] {
                        if ui
                            .selectable_value(&mut self.settings.lang, lang, lang.native_name())
                            .clicked()
                        {
                            i18n::set_lang(lang);
                            lang_changed = true;
                            ui.close();
                        }
                    }
                });
            });
            ui.menu_button(t("검사"), |ui| {
                if ui.button(t("Checker 열기")).clicked() {
                    self.st.checker.open = true;
                    ui.close();
                }
            });
            ui.menu_button(t("도움말"), |ui| {
                ui.label(t("LDF Studio — LIN Description File 뷰어·편집기"));
                ui.weak(t("M1 뷰어 + M2 Checker"));
                if let Some(p) = &self.project_path {
                    ui.separator();
                    ui.weak(tf!("프로젝트: {}", p.display()));
                }
            });
        });
        if lang_changed {
            // 창 제목은 언어가 바뀌어도 그대로면 다시 보내지 않으므로 비워 새로 만들게 한다
            self.title.clear();
            if let Some(snap) = self.snapshot.clone() {
                self.set_title(&ctx, &snap);
            }
        }
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            if ui.button(t("LDF 열기")).clicked() {
                self.pick(&ctx, false);
            }
            if ui.button("Checker").clicked() {
                self.st.checker.open = true;
            }
            ui.separator();
            let (can_undo, can_redo, dirty) = self
                .snapshot
                .as_ref()
                .map_or((false, false, false), |s| (s.can_undo, s.can_redo, s.dirty));
            if ui
                .add_enabled(can_undo, egui::Button::new("Undo"))
                .on_hover_text(t("실행 취소 (Ctrl+Z)"))
                .clicked()
            {
                self.undo();
            }
            if ui
                .add_enabled(can_redo, egui::Button::new("Redo"))
                .on_hover_text(t("다시 실행 (Ctrl+Y)"))
                .clicked()
            {
                self.redo();
            }
            if ui
                .add_enabled(dirty, egui::Button::new(t("저장")))
                .on_hover_text(t("편집한 내용을 새 리비전으로 저장 (Ctrl+S)"))
                .clicked()
            {
                self.save_edits();
            }
            ui.separator();
            ui.toggle_value(&mut self.settings.hex, "hex")
                .on_hover_text(t("숫자를 16진수로 표시"));
            ui.separator();
            self.revision_combo(ui);
        });
    }

    fn revision_combo(&mut self, ui: &mut Ui) {
        if self.revisions.is_empty() {
            return;
        }
        let current = self.snapshot.as_ref().map(|s| s.revision.id);
        let label = |r: &RevisionInfo| format!("#{} {} ({})", r.id, tr(&r.label), r.created_at);
        let selected = self
            .revisions
            .iter()
            .find(|r| Some(r.id) == current)
            .map_or_else(|| t("리비전").to_owned(), label);
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
            self.request(Pending::OpenRevision(id));
        }
    }

    fn notice_bar(&mut self, ui: &mut Ui) {
        let Some(msg) = self.notice.clone() else {
            return;
        };
        egui::Panel::top("notice").show(ui, |ui| {
            ui.horizontal(|ui| {
                // 알림은 한국어로 쌓아 둔 채 언어를 바꿀 수 있으므로 그리는 자리에서 번역한다
                ui.colored_label(ui.visuals().warn_fg_color, tr(&msg));
                if ui.small_button(t("닫기")).clicked() {
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
                ui.label(t("LDF 파일을 이 창에 끌어다 놓거나 아래 버튼으로 여세요."));
                ui.add_space(12.0);
                if ui.button(t("LDF 열기…")).clicked() {
                    self.pick(&ctx, false);
                }
                if !self.revisions.is_empty() {
                    ui.add_space(16.0);
                    ui.weak(t("이 프로젝트에 저장된 리비전:"));
                    let revs: Vec<RevisionInfo> = self.revisions.iter().rev().cloned().collect();
                    for r in revs {
                        if ui
                            .link(format!("#{} {}  ({})", r.id, tr(&r.label), r.created_at))
                            .clicked()
                        {
                            self.request(Pending::OpenRevision(r.id));
                        }
                    }
                }
            });
        });
    }

    fn checker_window(&mut self, ctx: &egui::Context, snap: &Arc<ModelSnapshot>) {
        match crate::ui::checker::show(ctx, snap, &self.rules, &mut self.st.checker) {
            Some(CheckerAction::RunChecks(text)) => {
                if text == snap.source {
                    self.notice = Some(t("변경한 내용이 없다").to_owned());
                    return;
                }
                let name = snap
                    .revision
                    .source_name
                    .clone()
                    .unwrap_or_else(|| snap.revision.label.clone());
                self.busy = true;
                self.send(Command::ApplyText {
                    name,
                    text,
                    encoding: snap.revision.encoding.clone(),
                    parent: Some(snap.revision.id),
                });
            }
            Some(CheckerAction::SaveCsv(bytes)) => self.save_csv(ctx, bytes),
            Some(CheckerAction::SetRule { code, setting }) => {
                self.send(Command::SetRule {
                    code,
                    enabled: setting.enabled,
                    severity: setting.severity,
                });
            }
            None => {}
        }
    }

    /// 저장 안 한 변경이 있을 때 저장/버리기/취소를 묻는다(FR-EDT-09).
    fn unsaved_window(&mut self, ctx: &egui::Context) {
        let Some(act) = self.unsaved.clone() else {
            return;
        };
        let what = match &act {
            Pending::Quit => t("종료하기 전에"),
            Pending::Import(_) | Pending::OpenProject(_) => t("다른 파일을 열기 전에"),
            Pending::OpenRevision(_) => t("다른 리비전을 열기 전에"),
        };
        egui::Window::new(t("저장하지 않은 변경"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(tf!("{what} 편집한 내용을 저장할까?"));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(t("저장")).clicked() {
                        self.unsaved = None;
                        self.after_save = Some(act.clone());
                        self.save_edits();
                    }
                    if ui.button(t("버리기")).clicked() {
                        self.unsaved = None;
                        self.perform(act.clone());
                    }
                    if ui.button(t("취소")).clicked() {
                        self.unsaved = None;
                    }
                });
            });
    }

    fn revisions_window(&mut self, ctx: &egui::Context) {
        let current = self.snapshot.as_ref().map(|s| s.revision.id);
        let revisions = self.revisions.clone();
        match crate::ui::revisions::show(ctx, &revisions, current, &mut self.st.revisions) {
            Some(RevAction::Open(id)) => self.request(Pending::OpenRevision(id)),
            Some(RevAction::Delete(id)) => {
                self.busy = true;
                self.send(Command::DeleteRevision(id));
            }
            Some(RevAction::Diff { before, after }) => {
                self.busy = true;
                self.send(Command::DiffRevisions { before, after });
            }
            Some(RevAction::SaveDiff { csv }) => {
                if let Some(d) = &self.st.revisions.diff {
                    let (a, b) = (format!("#{}", d.before), format!("#{}", d.after));
                    if csv {
                        let bytes = ldf_engine::diff_report::to_csv(&d.diff);
                        self.save_bytes(
                            ctx,
                            bytes,
                            ("CSV", "csv"),
                            "ldf-diff.csv",
                            t("비교 결과를 CSV로 저장"),
                        );
                    } else {
                        let html = ldf_engine::diff_report::to_html(&d.diff, &a, &b);
                        self.save_bytes(
                            ctx,
                            html.into_bytes(),
                            ("HTML", "html"),
                            "ldf-diff.html",
                            t("비교 결과를 HTML로 저장"),
                        );
                    }
                }
            }
            None => {}
        }
    }

    fn export_window(&mut self, ctx: &egui::Context, snap: &Arc<ModelSnapshot>) {
        match crate::ui::export::show(ctx, snap, &mut self.st.export) {
            Some(ExportAction::Choose(options)) => {
                self.export_pending = Some(options);
                self.pick_with(ctx, Mode::Export);
            }
            Some(ExportAction::OpenChecker) => self.st.checker.open = true,
            Some(ExportAction::Retry(b)) => {
                self.busy = true;
                let allow = ExportOptions {
                    allow_errors: true,
                    ..b.options
                };
                self.send(Command::Export {
                    revision: b.revision,
                    path: b.path,
                    options: allow,
                });
            }
            None => {}
        }
    }

    /// 보고서를 CSV로 저장한다. 파일 대화상자는 별도 스레드에서 띄운다.
    fn save_csv(&mut self, ctx: &egui::Context, bytes: Vec<u8>) {
        self.save_bytes(
            ctx,
            bytes,
            ("CSV", "csv"),
            "check-report.csv",
            t("검사 보고서 저장"),
        );
    }

    /// 바이트를 사용자가 고른 파일에 쓴다(대화상자는 별도 스레드). 결과는 알림 줄로 보인다.
    fn save_bytes(
        &mut self,
        ctx: &egui::Context,
        bytes: Vec<u8>,
        filter: (&'static str, &'static str),
        default_name: &'static str,
        title: &'static str,
    ) {
        let tx = self.msg_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .add_filter(filter.0, &[filter.1])
                .set_file_name(default_name)
                .set_title(title)
                .save_file();
            if let Some(path) = picked {
                let msg = match std::fs::write(&path, bytes) {
                    Ok(()) => tf!("저장했다: {}", path.display()),
                    Err(e) => tf!("저장하지 못했다: {e}"),
                };
                let _ = tx.send(msg);
                ctx.request_repaint();
            }
        });
    }

    fn fatal_screen(&mut self, ui: &mut Ui, msg: &str) {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.3);
                ui.heading(t("엔진이 중단되었다"));
                ui.colored_label(ui.visuals().error_fg_color, tr(msg));
                ui.add_space(8.0);
                if ui.button(t("엔진 다시 시작")).clicked() {
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
}

/// 왼쪽 표(스케줄·컨텍스트)가 머리글("Max. Frame Time [ms]" 등)과 "종류" 칸을 줄임표 없이 보여 주는 최소 너비(점).
const LEFT_MIN_WIDTH: f32 = 540.0;

/// 메인 화면(내비게이션, 스케줄 뷰, 컨텍스트 뷰, 속성 창 2개)을 그린다. 속성 창에서 값을 고쳤으면 보낼 연산과 편집 뒤 선택할 객체를 돌려준다.
/// 앱 본체와 떨어진 함수라서 화면 시험이 상태(`UiState`)만 들고 그릴 수 있다.
pub(crate) fn main_view(
    ui: &mut Ui,
    snap: &Arc<ModelSnapshot>,
    st: &mut UiState,
    hex: bool,
) -> Option<(EditOp, Option<SelKey>)> {
    // 좁은 창에서는 내비게이션을 처음부터 좁게 연다(창 너비의 23%, 200~290점)
    let nav_width = (ui.available_width() * 0.23).clamp(200.0, 290.0);
    egui::Panel::left("nav")
        .default_size(nav_width)
        .size_range(200.0..=560.0)
        .show(ui, |ui| {
            nav::show(ui, snap, st, hex);
        });

    if let Selection::Schedule(s) | Selection::Slot(s, _) = &st.selection {
        st.last_schedule = Some(*s);
    }

    egui::CentralPanel::default()
        .show(ui, |ui| {
            // 속성 창 내용은 패널을 그리기 전에 계산해 두어, 표가 선택 상태를 바꿀 수 있게 한다
            let sched_props = {
                let sel = match (&st.selection, st.last_schedule) {
                    (s @ (Selection::Schedule(_) | Selection::Slot(..)), _) => s.clone(),
                    (_, Some(last)) => Selection::Schedule(last),
                    _ => Selection::None,
                };
                view::props::schedule_props(&snap.model, &sel)
            };
            let height = ui.available_height();
            // 속성 창의 입력 상태는 본 뷰와 동시에 빌릴 수 없어 잠시 꺼내 쓴다
            let mut inline = std::mem::take(&mut st.inline);
            let selection = st.selection.clone();
            let sched_selection = match (&selection, st.last_schedule) {
                (s @ (Selection::Schedule(_) | Selection::Slot(..)), _) => s.clone(),
                (_, Some(last)) => Selection::Schedule(last),
                _ => Selection::None,
            };
            let mut inline_op: Option<EditOp> = None;

            egui::Panel::top("schedule-region")
                .resizable(true)
                .default_size(height * 0.42)
                .size_range(110.0..=height.max(200.0))
                .show(ui, |ui| {
                    split(
                        ui,
                        |ui| schedule::show(ui, snap, st),
                        |ui| match &sched_props {
                            Some(sec) => {
                                let cx = props::EditCtx {
                                    model: &snap.model,
                                    selection: &sched_selection,
                                    scope: InlineScope::Schedule,
                                    inline: &mut inline,
                                };
                                if let Some(op) =
                                    props::section_editable(ui, "sched-props", sec, cx)
                                {
                                    inline_op = Some(op);
                                }
                            }
                            None => props::placeholder(
                                ui,
                                "Schedule Properties",
                                t("스케줄 테이블이나 슬롯을 선택하세요"),
                            ),
                        },
                    );
                });

            let obj_props = view::props::object_props(&snap.model, &st.selection, hex);
            let frame_for_bits: Option<FrameId> = match &st.selection {
                Selection::Frame(f) => Some(*f),
                Selection::Slot(s, i) => view::props::slot_frame(&snap.model, *s, *i),
                _ => None,
            };
            split(
                ui,
                |ui| context::show(ui, snap, st, hex),
                |ui| {
                    match &obj_props {
                        Some(sec) => {
                            let cx = props::EditCtx {
                                model: &snap.model,
                                selection: &selection,
                                scope: InlineScope::Object,
                                inline: &mut inline,
                            };
                            if let Some(op) = props::section_editable(ui, "obj-props", sec, cx) {
                                inline_op = Some(op);
                            }
                        }
                        None => props::placeholder(ui, "Properties", t("항목을 선택하세요")),
                    }
                    if let Some(f) = frame_for_bits {
                        bits::show(ui, &snap.model, f);
                    }
                },
            );
            st.inline = inline;
            // 이름을 바꾸면 선택이 그 이름을 따라가게 한다
            inline_op.map(|op| {
                let select = renamed_selection(&snap.model, &st.selection, &op);
                (op, select)
            })
        })
        .inner
}

/// 속성 창에서 이름을 바꾸는 연산이면, 편집 뒤에도 선택이 바뀐 이름의 객체를 가리키게 하는 키.
fn renamed_selection(m: &ldf_model::LdfModel, sel: &Selection, op: &EditOp) -> Option<SelKey> {
    let EditOp::Batch(ops) = op else {
        return match op {
            EditOp::RenameSchedule { to, .. } => Some(SelKey::Schedule(to.clone())),
            _ => None,
        };
    };
    ops.iter()
        .find_map(|o| match (o, sel) {
            (EditOp::EditFrame { draft, .. }, Selection::Frame(_)) => {
                Some(SelKey::Frame(draft.name.clone()))
            }
            (EditOp::EditSignal { draft, .. }, Selection::Signal(_)) => {
                Some(SelKey::Signal(draft.name.clone()))
            }
            (EditOp::RenameNode { to, .. }, Selection::Node(_)) => Some(SelKey::Node(to.clone())),
            _ => None,
        })
        .or_else(|| Some(sel.key(m)))
}

/// 왼쪽에 본 뷰, 오른쪽에 속성 창을 두는 가로 분할.
///
/// 왼쪽 표가 [`LEFT_MIN_WIDTH`]보다 좁아지면 머리글을 줄임표로 줄이지 않고 왼쪽 칸 안에서 가로로 스크롤한다.
/// 속성 패널(오른쪽)은 늘 310점으로 창 안에 남는다. 스크롤 영역을 가운데 영역 전체에 두면 패널이 자기 영역을 직접 자르기 때문에
/// 내비게이션 위로 번지므로, 스크롤은 칸 안에 가둔다.
fn split(ui: &mut Ui, left: impl FnOnce(&mut Ui), right: impl FnOnce(&mut Ui)) {
    StripBuilder::new(ui)
        .size(Size::remainder().at_least(220.0))
        .size(Size::exact(310.0))
        .horizontal(|mut strip| {
            strip.cell(|ui| {
                // 가로 막대만 떠 있지 않고 늘 보이게 해 표가 잘려 있다는 것을 알린다(안쪽 영역의 막대는 그대로)
                ui.spacing_mut().scroll.floating = false;
                egui::ScrollArea::horizontal()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().scroll.floating = true;
                        ui.set_min_width(LEFT_MIN_WIDTH);
                        left(ui);
                    });
            });
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
        while let Ok(m) = self.msg_rx.try_recv() {
            self.notice = Some(m);
        }
        self.handle_input(ui);
        // 창을 닫으려 할 때 저장 안 한 변경이 있으면 막고 확인을 받는다(FR-EDT-09)
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.unsaved = Some(Pending::Quit);
        }

        egui::Panel::top("menu").show(ui, |ui| {
            self.menu_bar(ui);
            self.toolbar(ui);
        });
        self.notice_bar(ui);

        let snap = self.snapshot.clone();
        let mut open_checker = false;
        egui::Panel::bottom("status").show(ui, |ui| {
            open_checker = status::show(ui, snap.as_deref(), self.busy, self.perf);
        });
        if open_checker {
            self.st.checker.open = true;
        }

        if let Some(msg) = self.fatal.clone() {
            self.fatal_screen(ui, &msg);
        } else if let Some(snap) = snap {
            if let Some((op, select)) = main_view(ui, &snap, &mut self.st, self.settings.hex) {
                self.select_after_edit = select;
                self.edit(op);
            }
            if let Some(op) = self.st.pending_op.take() {
                self.edit(op);
            }
            self.checker_window(&ctx, &snap);
            self.export_window(&ctx, &snap);
            self.edit_window(&ctx, &snap);
            self.unsaved_window(&ctx);
            self.save_window(&ctx);
        } else {
            self.welcome(ui);
        }
        self.revisions_window(&ctx);
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
