// Copyright (C) 2026 Juan Pablo Devoto
// SPDX-License-Identifier: GPL-3.0-only

use crate::{
    model::*,
    network::*,
    theme::{self, Icon, icon_button, icon_menu},
};
use eframe::egui::{self, Color32, RichText};
use std::sync::{Arc, Mutex, Weak};
mod editors;
use editors::{AppearanceDraft, AppearanceTab};
struct Tab {
    source: Source,
    view: Arc<Mutex<View>>,
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
    follow: bool,
    scroll: egui::Vec2,
    selected: Option<u64>,
    retained_origin: Option<String>,
    clear_on_start: bool,
    filters_open: bool,
}
impl Tab {
    fn copy_selection(&mut self) -> Option<String> {
        let view = self.view.lock().unwrap();
        let text = self
            .selected
            .and_then(|id| view.selected_line(id))
            .map(|l| l.text.clone());
        if text.is_none() {
            self.selected = None;
        }
        text
    }
    // Retiring workers retain only the previous view, so late output/status cannot
    // overwrite a restarted source or relabel bytes after editing its endpoint.
    fn retire_reader(&mut self, clear: bool) {
        self.cancel.take();
        let next = self.view.lock().unwrap().retire(clear);
        self.view = Arc::new(Mutex::new(next));
        if clear {
            self.selected = None;
            self.scroll = egui::Vec2::ZERO;
        }
    }
}
#[derive(Clone)]
enum PendingAction {
    Connect,
    Reconnect,
    Start(String),
    Open(Source),
    Browse(String),
}
struct PendingOperation {
    profile: String,
    generation: Option<String>,
    action: PendingAction,
    owns_connection: bool,
}
struct ConnectionDraft {
    profile: Profile,
    secret: String,
    is_new: bool,
    error: String,
}
struct TrustRequest {
    profile: String,
    generation: String,
    host: String,
    key: String,
    reply: tokio::sync::oneshot::Sender<bool>,
}
#[derive(Clone, Copy)]
enum DiscardAction {
    Clear,
    Close,
}
struct DiscardRequest {
    source: String,
    view: Weak<Mutex<View>>,
    action: DiscardAction,
}
struct ReconnectRequest {
    profile: String,
    generation: String,
}
pub struct App {
    appearance: Option<AppearanceDraft>,
    appearance_tab: AppearanceTab,
    manage_trust: bool,
    rule_revision: u64,
    discard: Option<DiscardRequest>,
    reconnect: Option<ReconnectRequest>,
    reconnected: std::collections::HashMap<String, String>,
    copy_feedback: Option<(String, std::time::Instant)>,
    export: Option<tokio::sync::oneshot::Receiver<std::io::Result<()>>>,
    frame_stats: crate::rendering::FrameStats,
    #[cfg(feature = "qa")]
    qa_frames: usize,
    #[cfg(feature = "qa")]
    qa_last_frame: std::time::Instant,
    #[cfg(feature = "qa")]
    qa_max_gap_ms: u128,
    wordmark: egui::TextureHandle,
    mascot: egui::TextureHandle,
    ctx: egui::Context,
    settings: Settings,
    rt: tokio::runtime::Runtime,
    sessions: Vec<Option<Session>>,
    secrets: Vec<String>,
    tabs: Vec<Tab>,
    active: usize,
    profile: usize,
    source: Source,
    status: String,
    credential_warning: Option<String>,
    preferences: bool,
    about: bool,
    edit_connection: bool,
    draft: Option<ConnectionDraft>,
    open_remote: bool,
    pending: Option<PendingOperation>,
    credential_profile: Option<String>,
    credential_secret: String,
    credential_remember: bool,
    states: std::collections::HashMap<String, ConnectionState>,
    operation_error: String,
    browser_sudo: bool,
    browser_request: u64,
    browser_error: String,
    browser_path: String,
    delete_server: Option<String>,
    local_path: String,
    can_save: bool,
    sidebar: bool,
    browse: bool,
    browse_profile: String,
    directory: String,
    files: Vec<(String, bool)>,
    trust: std::collections::VecDeque<TrustRequest>,
}
impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        let loaded = Settings::load();
        let status = loaded
            .as_ref()
            .err()
            .map(|e| format!("Could not load settings: {e}"))
            .unwrap_or_else(|| "Ready — open local files or connect a server".into());
        let can_save = loaded.is_ok();
        let settings = loaded.unwrap_or_default();
        #[allow(unused_mut)]
        let mut app = Self::from_settings(&cc.egui_ctx, settings, can_save, status);
        #[cfg(feature = "qa")]
        if let Ok(scale) = std::env::var("SSHLOGTAIL_QA_SCALE") {
            cc.egui_ctx
                .set_pixels_per_point(scale.parse().unwrap_or(1.));
            app.settings = Settings::default();
            app.settings.profiles.push(Profile {
                id: new_id(),
                name: "QA fixture (offline)".into(),
                ..Default::default()
            });
            app.sessions = vec![None];
            app.secrets = vec![String::new()];
            let tab = Self::tab(&app.ctx, Source::default(), &[]);
            {
                let mut v = tab.view.lock().unwrap();
                for i in 0..1000 {
                    v.push(format!(
                        "2026-11-03 14:00:{:02} INFO request={i} synthetic rendering fixture",
                        i % 60
                    ));
                }
            }
            app.tabs = vec![tab];
            app.status = "QA synthetic rendering data — no network connection".into();
            app.preferences = std::env::var_os("SSHLOGTAIL_QA_SETTINGS").is_some();
            if let Ok(dir) = std::env::var("SSHLOGTAIL_QA_LIVE_DIR") {
                app.settings.profiles[0] = Profile {
                    id: new_id(),
                    name: "QA live SSH".into(),
                    host: std::env::var("SSHLOGTAIL_QA_HOST").unwrap(),
                    port: 22,
                    user: "root".into(),
                    key: std::env::var("SSHLOGTAIL_TEST_KEY").unwrap(),
                    remember: false,
                };
                app.settings.trusted.insert(
                    format!("{}:22", app.settings.profiles[0].host),
                    std::env::var("SSHLOGTAIL_QA_FINGERPRINT").unwrap(),
                );
                app.tabs = (0..4)
                    .map(|n| {
                        Self::tab(
                            &app.ctx,
                            Source {
                                id: new_id(),
                                profile: app.settings.profiles[0].id.clone(),
                                path: format!("{dir}/{n}.log"),
                                kind: SourceKind::SshFile,
                                sudo: false,
                            },
                            &app.settings.rules,
                        )
                    })
                    .collect();
                if let Ok(dir) = std::env::var("SSHLOGTAIL_QA_MIXED_LOCAL_DIR") {
                    for n in 2..4 {
                        app.tabs[n].source.kind = SourceKind::LocalFile;
                        app.tabs[n].source.profile.clear();
                        app.tabs[n].source.path = std::path::Path::new(&dir)
                            .join(format!("{n}.log"))
                            .display()
                            .to_string();
                    }
                }
                app.connect();
                for n in 0..4 {
                    app.start_reader(n)
                }
            }
        }
        #[cfg(feature = "qa")]
        if std::env::var_os("SSHLOGTAIL_QA_SCALE").is_some()
            && std::env::var_os("SSHLOGTAIL_QA_LIVE_DIR").is_none()
        {
            app.tabs.clear();
            app.settings.profiles = (0..4)
                .map(|n| Profile {
                    name: format!("Server {}", n + 1),
                    host: format!("192.0.2.{}", n + 1),
                    ..Default::default()
                })
                .collect();
            app.sessions = (0..4).map(|_| None).collect();
            app.secrets = vec![String::new(); 4];
            for n in 0..5 {
                let source = Source {
                    id: new_id(),
                    profile: if n == 0 {
                        String::new()
                    } else {
                        app.settings.profiles[n - 1].id.clone()
                    },
                    path: if n == 0 {
                        r"C:\Logs\app.log".into()
                    } else {
                        "/var/log/app.log".into()
                    },
                    kind: if n == 0 {
                        SourceKind::LocalFile
                    } else {
                        SourceKind::SshFile
                    },
                    sudo: false,
                };
                let t = Self::tab(&app.ctx, source, &[]);
                {
                    let mut v = t.view.lock().unwrap();
                    v.status = "QA sample (stopped)".into();
                    for line in 0..200 {
                        v.push(format!("INFO origin={n} sequence={line} sample log entry"));
                    }
                }
                app.tabs.push(t);
            }
        }
        #[cfg(feature = "qa")]
        if std::env::var_os("SSHLOGTAIL_QA_SCALE").is_some() {
            for t in &mut app.tabs {
                if std::env::var_os("SSHLOGTAIL_QA_LIVE_DIR").is_some() {
                    t.source.profile = app.settings.profiles[0].id.clone();
                }
            }
            app.settings.workspace.layout = std::env::var("SSHLOGTAIL_QA_LAYOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .filter(|n| [1, 2, 4].contains(n))
                .unwrap_or(1);
            for (i, t) in app.tabs.iter().take(4).enumerate() {
                app.settings.workspace.panes[i] = Some(t.source.id.clone());
            }
        }
        #[cfg(feature = "qa")]
        if let Ok(scene) = std::env::var("DINGOTAIL_QA_SCENE") {
            if scene == "filters" {
                for (i, tab) in app.tabs.iter_mut().enumerate() {
                    tab.filters_open = true;
                    let mut v = tab.view.lock().unwrap();
                    v.query = if i == 0 {
                        "absent".into()
                    } else {
                        "sequence=19".into()
                    };
                    v.refresh_filter();
                    if i == 1 {
                        v.query = "[".into();
                        v.regex = true;
                        v.refresh_filter();
                    }
                    if i == 2 {
                        v.clear_history();
                        v.set_status("Waiting for source");
                    }
                    tab.follow = i != 3;
                }
            }
            if scene == "discard" && !app.tabs.is_empty() {
                app.request_discard(0, DiscardAction::Clear);
            }
            if scene == "highlighting" || scene == "settings-error" {
                app.set_preferences(true);
                app.appearance_tab = AppearanceTab::Highlighting;
            }
            if scene == "settings-error" {
                let draft = app.appearance.as_mut().unwrap();
                draft.rules[0].regex = true;
                draft.rules[0].pattern = "[".into();
                draft.error = "Could not apply settings: example write failure. Check the settings folder permissions and retry.".into();
            }
            if scene == "empty" {
                app.tabs.clear();
                app.settings.workspace = Workspace::default();
            }
            if scene == "long" {
                for p in &mut app.settings.profiles {
                    p.name = "Production application cluster west".into();
                }
                for t in &mut app.tabs {
                    t.source.path = if t.source.kind == SourceKind::LocalFile {
                        r"C:\ProgramData\DingoTail\logs\application-cluster\2026-11-03\app.log"
                            .into()
                    } else {
                        "/var/log/production/application-cluster/service/2026-11-03/app.log".into()
                    };
                    let mut v = t.view.lock().unwrap();
                    v.status = "Waiting for source".into();
                    v.diagnostic =
                        "QA permission failure: access denied. Check permissions and start again."
                            .into();
                    v.query = "INFO".into();
                    v.refresh_filter();
                }
            }
        }
        #[cfg(feature = "qa")]
        if let Ok(scene) = std::env::var("DINGOTAIL_QA_DIALOG") {
            if scene == "connection" {
                app.begin_edit(Some(0));
            }
            if scene == "source" {
                app.begin_open(0);
            }
            if scene == "about" {
                app.about = true;
            }
            if scene == "settings" {
                app.preferences = true;
            }
            if scene == "browser" {
                app.browse = true;
                app.browse_profile = app.settings.profiles[0].id.clone();
                app.files = vec![("a-very-long-application-log-filename.log".into(), false)];
            }
            app.states.insert(
                app.settings.profiles[0].id.clone(),
                ConnectionState::Connected,
            );
        }
        // Offline load fixture: exercises real local readers without an SSH server.
        #[cfg(feature = "qa")]
        if let Ok(dir) = std::env::var("DINGOTAIL_QA_LOCAL_DIR") {
            app.tabs.clear();
            app.settings.profiles.clear();
            app.sessions.clear();
            app.secrets.clear();
            for n in 0..4 {
                app.tabs.push(Self::tab(
                    &app.ctx,
                    Source {
                        path: std::path::Path::new(&dir)
                            .join(format!("{n}.log"))
                            .display()
                            .to_string(),
                        kind: SourceKind::LocalFile,
                        ..Default::default()
                    },
                    &app.settings.rules,
                ));
                app.settings.workspace.panes[n] = Some(app.tabs[n].source.id.clone());
                app.start(n);
            }
            app.settings.workspace.layout = 4;
        }
        app
    }
    fn tab(ctx: &egui::Context, source: Source, rules: &[Rule]) -> Tab {
        Tab {
            source,
            view: Arc::new(Mutex::new(View::new(ctx.clone(), rules))),
            cancel: None,
            follow: true,
            scroll: egui::Vec2::ZERO,
            selected: None,
            retained_origin: None,
            clear_on_start: false,
            filters_open: false,
        }
    }
    fn from_settings(
        ctx: &egui::Context,
        settings: Settings,
        can_save: bool,
        status: String,
    ) -> Self {
        let n = settings.profiles.len();
        let tabs = settings
            .sources
            .iter()
            .cloned()
            .map(|s| Self::tab(ctx, s, &settings.rules))
            .collect();
        Self {
            appearance: None,
            appearance_tab: AppearanceTab::Appearance,
            manage_trust: false,
            rule_revision: 0,
            discard: None,
            reconnect: None,
            reconnected: Default::default(),
            copy_feedback: None,
            frame_stats: Default::default(),
            export: None,
            #[cfg(feature = "qa")]
            qa_frames: 0,
            #[cfg(feature = "qa")]
            qa_last_frame: std::time::Instant::now(),
            #[cfg(feature = "qa")]
            qa_max_gap_ms: 0,
            wordmark: theme::texture(
                ctx,
                "wordmark",
                include_bytes!("../assets/brand/wordmark.png"),
            ),
            mascot: theme::texture(ctx, "mascot", include_bytes!("../assets/brand/mascot.png")),
            ctx: ctx.clone(),
            settings,
            rt: tokio::runtime::Runtime::new().unwrap(),
            sessions: (0..n).map(|_| None).collect(),
            secrets: vec![String::new(); n],
            tabs,
            active: 0,
            profile: 0,
            source: Source::default(),
            status,
            credential_warning: None,
            preferences: false,
            about: false,
            edit_connection: false,
            draft: None,
            open_remote: false,
            pending: None,
            credential_profile: None,
            credential_secret: String::new(),
            credential_remember: false,
            states: Default::default(),
            operation_error: String::new(),
            browser_sudo: false,
            browser_request: 0,
            browser_error: String::new(),
            browser_path: String::new(),
            delete_server: None,
            local_path: String::new(),
            can_save,
            sidebar: true,
            browse: false,
            browse_profile: String::new(),
            directory: "/var/log".into(),
            files: vec![],
            trust: Default::default(),
        }
    }
    fn save(&mut self) {
        if !self.can_save {
            return;
        }
        #[cfg(feature = "qa")]
        if std::env::var_os("SSHLOGTAIL_QA_SCALE").is_some() {
            return;
        }
        self.settings.sources = self.tabs.iter().map(|t| t.source.clone()).collect();
        if let Err(e) = self.settings.save() {
            self.status = format!("Settings save failed: {e}")
        }
    }
    fn disconnect_profile(&mut self, index: usize, clear: bool) {
        let id = self.settings.profiles[index].id.clone();
        self.trust.retain(|t| t.profile != id);
        self.states.insert(id, ConnectionState::Disconnected);
        if let Some(session) = self.sessions[index].take() {
            let _ = session.tx.try_send(Request::Stop);
        }
        let id = &self.settings.profiles[index].id;
        for t in &mut self.tabs {
            if t.source.kind != SourceKind::LocalFile && &t.source.profile == id {
                t.retire_reader(clear);
                t.view
                    .lock()
                    .unwrap()
                    .apply_rules_at(&self.settings.rules, self.rule_revision);
                t.view.lock().unwrap().status = "Stopped · connect and start to resume".into();
            }
        }
    }
    fn delete_server(&mut self, id: &str) {
        let Some(index) = self.settings.profiles.iter().position(|p| p.id == id) else {
            return;
        };
        let name = self.settings.profiles[index].name.clone();
        self.disconnect_profile(index, false);
        for tab in &self.tabs {
            if tab.source.group() == id {
                self.settings.workspace.close(&tab.source.id);
            }
        }
        self.tabs.retain(|tab| tab.source.group() != id);
        self.settings.profiles.remove(index);
        self.sessions.remove(index);
        self.secrets.remove(index);
        self.settings.workspace.groups.remove(id);
        self.states.remove(id);
        if self.pending.as_ref().is_some_and(|p| p.profile == id) {
            self.cancel_pending();
        }
        if self.source.profile == id {
            self.open_remote = false;
        }
        if self.draft.as_ref().is_some_and(|d| d.profile.id == id) {
            self.draft = None;
            self.edit_connection = false;
        }
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        if self.profile == index {
            self.edit_connection = false;
            self.source = Source::default();
        }
        if self.profile > index {
            self.profile -= 1;
        }
        self.profile = self
            .profile
            .min(self.settings.profiles.len().saturating_sub(1));
        if self.browse_profile == id {
            self.browse = false;
            self.browse_profile.clear();
            self.files.clear();
        }
        self.status = format!("Deleted server {name} from workspace");
        self.save();
    }
    #[cfg(feature = "qa")]
    fn connect(&mut self) {
        self.connect_index(self.profile);
    }
    fn connect_index(&mut self, index: usize) {
        if index >= self.settings.profiles.len() {
            return;
        }
        let state = self.state(&self.settings.profiles[index].id);
        if state == ConnectionState::Connected || state.pending() {
            return;
        }
        self.disconnect_profile(index, false);
        let p = self.settings.profiles[index].clone();
        let host = format!("{}:{}", p.host, p.port);
        let mut secret = self.secrets[index].clone();
        if p.remember {
            let result = if secret.is_empty() {
                crate::credentials::read(&p).map(|saved| {
                    secret = saved.unwrap_or_default();
                })
            } else {
                crate::credentials::write(&p, &secret)
            };
            if let Err(error) = result {
                self.credential_warning = Some(format!(
                    "Secure credential store unavailable: {error}. Continuing for this session without saving the secret."
                ));
            }
        }
        self.states
            .insert(p.id.clone(), ConnectionState::Connecting);
        self.sessions[index] = Some(crate::network::connect(
            &self.rt,
            p,
            secret,
            self.settings.trusted.get(&host).cloned(),
            self.ctx.clone(),
        ));
        self.save()
    }
    fn start(&mut self, index: usize) {
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        if tab.source.kind != SourceKind::LocalFile {
            let id = tab.source.profile.clone();
            if !self.settings.profiles.iter().any(|p| p.id == id) {
                tab.view.lock().unwrap().set_status("Missing server — restore its saved profile or open the log from an existing server");
                return;
            }
            if self.state(&id) != ConnectionState::Connected {
                self.request_operation_for(id, PendingAction::Start(tab.source.id.clone()));
                return;
            }
        }
        self.start_reader(index);
    }
    fn start_reader(&mut self, index: usize) {
        // Reserve queue space before retiring the old view or its attribution.
        let permit = if self.tabs[index].source.kind != SourceKind::LocalFile {
            let session = self
                .settings
                .profiles
                .iter()
                .position(|p| p.id == self.tabs[index].source.profile)
                .and_then(|i| self.sessions[i].as_ref());
            let Some(permit) = session.and_then(|s| s.tx.clone().try_reserve_owned().ok()) else {
                self.tabs[index]
                    .view
                    .lock()
                    .unwrap()
                    .set_status("Connection unavailable — reconnect the server and start again");
                return;
            };
            Some(permit)
        } else {
            None
        };
        let t = &mut self.tabs[index];
        t.retire_reader(t.clear_on_start);
        t.view
            .lock()
            .unwrap()
            .apply_rules_at(&self.settings.rules, self.rule_revision);
        t.clear_on_start = false;
        t.retained_origin = None;
        self.reconnected.remove(&t.source.profile);
        let (tx, rx) = tokio::sync::oneshot::channel();
        if t.source.kind == SourceKind::LocalFile {
            crate::local::start(
                &self.rt,
                t.source.clone(),
                t.view.clone(),
                rx,
                self.settings.initial_lines,
            );
        } else if let Some(permit) = permit {
            permit.send(Request::Start {
                source: t.source.clone(),
                view: t.view.clone(),
                cancel: rx,
                initial_lines: self.settings.initial_lines,
            });
        }
        t.cancel = Some(tx);
    }
    fn close_browser(&mut self) {
        self.browse = false;
        self.browser_sudo = false;
        self.browser_request += 1;
        self.files.clear();
        if let Some(i) = self
            .settings
            .profiles
            .iter()
            .position(|p| p.id == self.browse_profile)
            && let Some(session) = &self.sessions[i]
        {
            let _ = session.tx.try_send(Request::CancelBrowse);
        }
    }
    fn select_browser_file(&mut self, name: &str) {
        if self.source.profile == self.browse_profile {
            self.source.path = format!("{}/{}", self.browser_path.trim_end_matches('/'), name);
        }
        self.close_browser();
    }
    fn browse_request(&mut self) {
        self.browser_request += 1;
        self.files.clear();
        self.browser_error.clear();
        self.browser_path = self.directory.clone();
        if let Some(index) = self
            .settings
            .profiles
            .iter()
            .position(|p| p.id == self.browse_profile)
            && let Some(session) = &self.sessions[index]
            && session
                .tx
                .try_send(Request::Browse {
                    request: self.browser_request,
                    path: self.directory.clone(),
                    sudo: self.browser_sudo,
                })
                .is_err()
        {
            self.browser_error = "Connection unavailable; reconnect and retry".into();
        }
    }
    fn state(&self, id: &str) -> ConnectionState {
        self.settings
            .profiles
            .iter()
            .position(|p| p.id == id)
            .and_then(|i| self.sessions[i].as_ref())
            .map(|s| s.state.borrow().clone())
            .or_else(|| self.states.get(id).cloned())
            .unwrap_or_default()
    }
    fn begin_edit(&mut self, index: Option<usize>) {
        self.draft = Some(match index {
            Some(i) => ConnectionDraft {
                profile: self.settings.profiles[i].clone(),
                secret: self.secrets[i].clone(),
                is_new: false,
                error: String::new(),
            },
            None => ConnectionDraft {
                profile: Profile::default(),
                secret: String::new(),
                is_new: true,
                error: String::new(),
            },
        });
        self.edit_connection = true;
    }
    fn begin_open(&mut self, index: usize) {
        self.close_browser();
        self.cancel_pending();
        self.profile = index;
        self.source.profile = self.settings.profiles[index].id.clone();
        self.open_remote = true;
        self.operation_error.clear();
    }
    fn apply_draft(&mut self) {
        let Some(mut draft) = self.draft.take() else {
            return;
        };
        if draft.profile.name.trim().is_empty()
            || draft.profile.host.trim().is_empty()
            || draft.profile.user.trim().is_empty()
        {
            draft.error = "Enter a name, host and username.".into();
            self.draft = Some(draft);
            return;
        }
        if let Some(i) = self
            .settings
            .profiles
            .iter()
            .position(|p| p.id == draft.profile.id)
        {
            self.cancel_pending_for(&draft.profile.id);
            let previous = &self.settings.profiles[i];
            let changed =
                endpoint_changed(previous, &draft.profile) || self.secrets[i] != draft.secret;
            if changed {
                let old_origin = format!(
                    "{} ({}:{}) · retained",
                    previous.name, previous.host, previous.port
                );
                self.disconnect_profile(i, false);
                for tab in &mut self.tabs {
                    if tab.source.group() == draft.profile.id {
                        if tab.retained_origin.is_none() {
                            tab.retained_origin = Some(old_origin.clone());
                        }
                        tab.clear_on_start = true;
                    }
                }
                if self.browse_profile == draft.profile.id {
                    self.browse = false;
                    self.files.clear();
                    self.browser_request += 1;
                }
            }
            self.settings.profiles[i] = draft.profile;
            self.secrets[i] = draft.secret;
        } else if draft.is_new {
            self.settings.profiles.push(draft.profile);
            self.secrets.push(draft.secret);
            self.sessions.push(None);
        }
        self.edit_connection = false;
        self.save();
    }
    fn cancel_pending_for(&mut self, id: &str) {
        if self.pending.as_ref().is_some_and(|p| p.profile == id) {
            self.cancel_pending();
        }
    }
    fn cancel_pending(&mut self) {
        if let Some(pending) = self.pending.take()
            && pending.owns_connection
            && let Some(i) = self
                .settings
                .profiles
                .iter()
                .position(|p| p.id == pending.profile)
            && self.sessions[i]
                .as_ref()
                .is_some_and(|s| Some(&s.generation) == pending.generation.as_ref())
        {
            self.disconnect_profile(i, false);
        }
        self.credential_profile = None;
        self.credential_secret.clear();
    }
    fn request_operation(&mut self, action: PendingAction) {
        self.request_operation_for(self.source.profile.clone(), action);
    }
    fn request_operation_for(&mut self, id: String, action: PendingAction) {
        if self.pending.as_ref().is_some_and(|p| p.profile == id &&
            matches!((&p.action, &action), (PendingAction::Start(a), PendingAction::Start(b)) if a == b)) {
            return;
        }
        self.cancel_pending();
        self.operation_error.clear();
        let Some(i) = self.settings.profiles.iter().position(|p| p.id == id) else {
            return;
        };
        let state = self.state(&id);
        let connected_or_pending = state == ConnectionState::Connected || state.pending();
        let generation = if connected_or_pending {
            self.sessions[i].as_ref().map(|s| s.generation.clone())
        } else {
            None
        };
        self.pending = Some(PendingOperation {
            profile: id.clone(),
            generation,
            action,
            owns_connection: !connected_or_pending,
        });
        if connected_or_pending {
            return;
        }
        let p = &self.settings.profiles[i];
        if self.secrets[i].is_empty() && p.remember {
            match crate::credentials::read(p) {
                Ok(secret) => self.secrets[i] = secret.unwrap_or_default(),
                Err(error) => {
                    self.credential_warning = Some(format!(
                        "Secure credential store unavailable: {error}. Enter a secret for this session."
                    ))
                }
            }
        }
        let needs_secret = self.secrets[i].is_empty()
            && (p.key.is_empty() || russh::keys::load_secret_key(&p.key, None).is_err());
        if needs_secret {
            self.credential_profile = Some(id);
            self.credential_remember = p.remember;
            return;
        }
        self.connect_pending(i);
    }
    fn connect_pending(&mut self, i: usize) {
        self.connect_index(i);
        if let Some(p) = &mut self.pending {
            p.generation = self.sessions[i].as_ref().map(|s| s.generation.clone());
            if p.generation.is_none() {
                self.operation_error = self.status.clone();
                self.pending = None;
            }
        }
    }
    fn advance_pending(&mut self) {
        let Some(p) = &self.pending else {
            return;
        };
        if let PendingAction::Start(id) = &p.action
            && !self
                .tabs
                .iter()
                .any(|t| t.source.id == *id && t.source.profile == p.profile)
        {
            self.cancel_pending();
            return;
        }
        if self.credential_profile.is_some() || p.generation.is_none() {
            return;
        }
        let valid = self
            .settings
            .profiles
            .iter()
            .position(|v| v.id == p.profile)
            .and_then(|i| self.sessions[i].as_ref())
            .is_some_and(|s| Some(&s.generation) == p.generation.as_ref());
        if !valid {
            self.operation_error = "Connection was replaced. Retry this operation.".into();
            self.pending = None;
            return;
        }
        match self.state(&p.profile) {
            ConnectionState::Connected => {
                let pending = self.pending.take().unwrap();
                match pending.action {
                    PendingAction::Connect => {}
                    PendingAction::Reconnect => {
                        if let Some(generation) = pending.generation {
                            self.reconnected.insert(pending.profile, generation);
                        }
                        self.status = "Connected — sources remain stopped".into();
                    }
                    PendingAction::Start(id) => {
                        if let Some(index) = self.tabs.iter().position(|t| t.source.id == id) {
                            self.start_reader(index);
                        }
                    }
                    PendingAction::Open(source) => {
                        self.open_source(source);
                        self.open_remote = false;
                    }
                    PendingAction::Browse(path) => {
                        self.directory = path;
                        self.browse_profile = pending.profile;
                        self.browse = true;
                        self.browse_request();
                    }
                }
            }
            ConnectionState::Failed(error) => {
                if let PendingAction::Start(id) = &p.action
                    && let Some(tab) = self.tabs.iter().find(|t| &t.source.id == id)
                {
                    tab.view
                        .lock()
                        .unwrap()
                        .set_status(&format!("Connection failed — {error}. Start to retry"));
                }
                self.status = format!("Connection failed — {error}");
                self.operation_error = error;
                self.pending = None;
            }
            _ => {}
        }
    }
    fn origin(&self, source: &Source) -> String {
        if source.kind == SourceKind::LocalFile {
            return "Local computer".into();
        }
        if let Some(origin) = self
            .tabs
            .iter()
            .find(|t| t.source.id == source.id)
            .and_then(|t| t.retained_origin.as_ref())
        {
            return origin.clone();
        }
        self.settings
            .profiles
            .iter()
            .find(|p| p.id == source.profile)
            .map(|p| format!("{} ({}:{})", p.name, p.host, p.port))
            .unwrap_or_else(|| format!("Missing server [{}]", source.profile))
    }
    fn groups(&self) -> Vec<(String, String)> {
        let mut groups = vec![("local".into(), "Local computer".into())];
        groups.extend(
            self.settings
                .profiles
                .iter()
                .map(|p| (p.id.clone(), format!("{} ({}:{})", p.name, p.host, p.port))),
        );
        for t in &self.tabs {
            if !groups.iter().any(|(id, _)| id == t.source.group()) {
                groups.push((t.source.profile.clone(), self.origin(&t.source)));
            }
        }
        groups
    }
    fn focus_source(&mut self, index: usize) {
        self.active = index;
        self.settings
            .workspace
            .assign(self.tabs[index].source.id.clone());
        self.save();
    }
    fn close_source(&mut self, index: usize) {
        // Remove first: cancellation may retire all remaining views on this server.
        // The empty-history shortcut can then hold this removed view's lock safely.
        let mut tab = self.tabs.remove(index);
        tab.cancel.take();
        if self
            .pending
            .as_ref()
            .is_some_and(|p| matches!(&p.action, PendingAction::Start(id) if id == &tab.source.id))
        {
            self.cancel_pending();
        }
        self.settings.workspace.close(&tab.source.id);
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        self.save();
    }
    fn request_discard(&mut self, index: usize, action: DiscardAction) {
        let view = self.tabs[index].view.clone();
        let history = view.lock().unwrap();
        if history.buffer().lines.is_empty() {
            // Ingestion cannot insert evidence between the empty check and action.
            self.discard = None;
            match action {
                DiscardAction::Clear => self.tabs[index].selected = None,
                DiscardAction::Close => self.close_source(index),
            }
            return;
        }
        self.discard = Some(DiscardRequest {
            source: self.tabs[index].source.id.clone(),
            view: Arc::downgrade(&view),
            action,
        });
    }
    fn discard_index(&self) -> Option<usize> {
        let request = self.discard.as_ref()?;
        self.tabs.iter().position(|t| {
            t.source.id == request.source && request.view.ptr_eq(&Arc::downgrade(&t.view))
        })
    }
    fn confirm_discard(&mut self) {
        let index = self.discard_index();
        let request = self.discard.take();
        if let (Some(index), Some(request)) = (index, request) {
            match request.action {
                DiscardAction::Clear => {
                    let tab = &mut self.tabs[index];
                    tab.view.lock().unwrap().clear_history();
                    tab.selected = None;
                }
                DiscardAction::Close => self.close_source(index),
            }
        }
    }
    fn active_readers(&self, id: &str) -> usize {
        self.tabs
            .iter()
            .filter(|t| t.source.group() == id && t.cancel.as_ref().is_some_and(|c| !c.is_closed()))
            .count()
    }
    fn request_reconnect(&mut self, index: usize) {
        let Some(session) = &self.sessions[index] else {
            return;
        };
        let id = self.settings.profiles[index].id.clone();
        self.reconnect = Some(ReconnectRequest {
            profile: id.clone(),
            generation: session.generation.clone(),
        });
        if self.active_readers(&id) == 0 {
            self.confirm_reconnect();
        }
    }
    fn confirm_reconnect(&mut self) {
        let Some(request) = self.reconnect.take() else {
            return;
        };
        let Some(i) = self
            .settings
            .profiles
            .iter()
            .position(|p| p.id == request.profile)
        else {
            return;
        };
        if !self.sessions[i]
            .as_ref()
            .is_some_and(|s| s.generation == request.generation)
        {
            return;
        }
        self.cancel_pending_for(&request.profile);
        self.disconnect_profile(i, false);
        self.request_operation_for(request.profile, PendingAction::Reconnect);
    }
    fn set_preferences(&mut self, open: bool) {
        self.preferences = open;
        self.appearance = open.then(|| AppearanceDraft::new(&self.settings));
        self.manage_trust = false;
    }
    fn apply_appearance(&mut self) {
        let can_save = self.can_save;
        self.apply_appearance_with(|candidate| {
            anyhow::ensure!(
                can_save,
                "Settings could not be loaded. Resolve the settings-file error before saving."
            );
            #[cfg(feature = "qa")]
            if std::env::var_os("SSHLOGTAIL_QA_SCALE").is_some() {
                return Ok(());
            }
            candidate.save()
        });
    }
    fn apply_appearance_with(&mut self, save: impl FnOnce(&Settings) -> anyhow::Result<()>) {
        let Some(draft) = &self.appearance else {
            return;
        };
        let result = draft.candidate(&self.settings).and_then(|mut candidate| {
            candidate.sources = self.tabs.iter().map(|t| t.source.clone()).collect();
            save(&candidate)?;
            Ok(candidate)
        });
        match result {
            Ok(candidate) => {
                self.settings = candidate;
                self.rule_revision += 1;
                for tab in &self.tabs {
                    let view = tab.view.clone();
                    let rules = self.settings.rules.clone();
                    let revision = self.rule_revision;
                    self.rt.spawn_blocking(move || {
                        view.lock().unwrap().apply_rules_at(&rules, revision)
                    });
                }
                self.status = "Appearance and highlighting saved".into();
                self.set_preferences(false);
            }
            Err(error) => {
                self.appearance.as_mut().unwrap().error =
                    format!("Could not apply settings: {error}")
            }
        }
    }
    fn open_source(&mut self, source: Source) {
        if let Some(index) = self.tabs.iter().position(|t| {
            t.source.profile == source.profile
                && t.source.kind == source.kind
                && t.source.path == source.path
                && t.source.sudo == source.sudo
        }) {
            self.focus_source(index);
            if self.tabs[index].cancel.is_none() {
                self.start(index);
            }
            return;
        }
        self.tabs
            .push(Self::tab(&self.ctx, source, &self.settings.rules));
        let index = self.tabs.len() - 1;
        self.focus_source(index);
        self.start(index);
    }
    fn pick_local_files(&mut self) {
        if let Some(paths) = rfd::FileDialog::new().pick_files() {
            for path in paths {
                let path = match crate::platform::unicode_path(&path) {
                    Ok(path) => path,
                    Err(error) => {
                        self.status = error.to_string();
                        continue;
                    }
                };
                self.open_source(Source {
                    id: new_id(),
                    profile: String::new(),
                    path,
                    kind: SourceKind::LocalFile,
                    sudo: false,
                });
            }
        }
    }
    fn appearance_window(&mut self, ctx: &egui::Context) {
        if !self.preferences {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            if egui::Popup::is_any_open(ctx) {
                egui::Popup::close_all(ctx);
            } else if self.manage_trust {
                self.manage_trust = false;
            } else {
                self.set_preferences(false);
                return;
            }
        }
        let draft = self
            .appearance
            .get_or_insert_with(|| AppearanceDraft::new(&self.settings));
        draft.refresh();
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        let mut manage = self.manage_trust;
        theme::dialog(
            ctx,
            "Appearance & highlighting",
            "settings-editors-v1",
            940.,
            640.,
        )
        .open(&mut open)
        .vscroll(false)
        .max_width(
            (ctx.content_rect().width()
                - 24.
                - egui::Frame::window(&ctx.style()).total_margin().sum().x)
                .max(280.),
        )
        .show(ctx, |ui| {
            ui.add_enabled_ui(!self.manage_trust, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(
                        &mut self.appearance_tab,
                        AppearanceTab::Appearance,
                        "Appearance",
                    );
                    ui.selectable_value(
                        &mut self.appearance_tab,
                        AppearanceTab::Highlighting,
                        "Highlighting",
                    );
                    if draft.first_invalid().is_some()
                        && ui
                            .button(RichText::new("Fix invalid rules").color(Color32::LIGHT_RED))
                            .clicked()
                    {
                        self.appearance_tab = AppearanceTab::Highlighting;
                        draft.select_invalid();
                    }
                    if ui.button("Manage trusted servers…").clicked() {
                        manage = true;
                    }
                });
                ui.separator();
                // Reserve the footer using its measured content height. A guessed
                // height feeds overflow into egui's remembered window size each
                // frame and eventually pushes the dialog against the viewport.
                egui::TopBottomPanel::bottom("appearance-footer")
                    .frame(egui::Frame::NONE)
                    .resizable(false)
                    .show_separator_line(false)
                    .show_inside(ui, |ui| {
                        ui.separator();
                        if !draft.error.is_empty() {
                            egui::ScrollArea::vertical()
                                .id_salt("appearance-save-error")
                                .max_height(44.)
                                .min_scrolled_height(0.)
                                .show(ui, |ui| {
                                    ui.colored_label(Color32::LIGHT_RED, &draft.error);
                                });
                        }
                        ui.horizontal(|ui| {
                            apply = ui
                                .add_enabled(
                                    draft.first_invalid().is_none(),
                                    egui::Button::new(
                                        RichText::new("Apply and save")
                                            .color(editors::contrast_text(self.settings.accent)),
                                    )
                                    .fill(rgb(self.settings.accent)),
                                )
                                .clicked();
                            cancel = ui.button("Cancel").clicked();
                        });
                    });
                match self.appearance_tab {
                    AppearanceTab::Appearance => draft.appearance_ui(ui),
                    AppearanceTab::Highlighting => draft.highlighting_ui(ui),
                }
            });
        });
        self.manage_trust = manage;
        if !open || cancel {
            self.set_preferences(false);
        } else if apply {
            self.apply_appearance();
        }
        if self.preferences && self.manage_trust {
            let mut remove = None;
            let mut close = false;
            let response = egui::Modal::new(egui::Id::new("manage-trusted-servers")).show(ctx, |ui| {
                ui.set_max_width((ctx.content_rect().width() - 64.).clamp(200., 560.));
                ui.heading("Trusted servers");
                ui.label("Removing trust takes effect immediately and is saved separately from appearance changes.");
                egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 200.).max(80.)).show(ui, |ui| {
                    if self.settings.trusted.is_empty() { ui.label("No trusted servers saved."); }
                    for (host, key) in &self.settings.trusted {
                        ui.separator();
                        ui.strong(host);
                        ui.label(RichText::new(key).monospace());
                        if ui.button(format!("Remove trust for {host}")).clicked() { remove = Some(host.clone()); }
                    }
                });
                close = ui.button("Close").clicked();
            });
            if let Some(host) = remove {
                self.settings.trusted.remove(&host);
                self.save();
            }
            if close || response.should_close() {
                self.manage_trust = false;
            }
        }
    }
    fn viewer(&mut self, ui: &mut egui::Ui, index: usize) {
        let ctx = ui.ctx().clone();
        let pending_start = self.pending.as_ref().is_some_and(
            |p| matches!(&p.action, PendingAction::Start(id) if id == &self.tabs[index].source.id),
        );
        egui::TopBottomPanel::bottom(egui::Id::new(("log-footer", &self.tabs[index].source.id)))
            .frame(egui::Frame::NONE).resizable(false).show_inside(ui, |ui| {
                let mut v = self.tabs[index].view.lock().unwrap();
                ui.separator();
                ui.horizontal(|ui| {
                    let active = v.filtering() || !v.query.is_empty() || !v.exclude.is_empty() || v.regex;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if active && !self.tabs[index].filters_open && ui.small_button("Clear filters").clicked() { v.clear_filters(); }
                        let counts = format!("{} visible / {} retained", v.visible_len(), v.buffer().lines.len());
                        ui.add(egui::Label::new(&counts).truncate()).on_hover_text(format!("{counts}\nRetained history: up to 20,000 lines or 16 MiB per source. Oldest lines are evicted first. Export includes visible retained lines."));
                    });
                });
                let summary = pane_filter_summary(&v);
                if !summary.is_empty() {
                    ui.add(egui::Label::new(&summary).truncate()).on_hover_text(&summary);
                }
            });
        let mut start = false;
        let mut stop = false;
        let mut clear = false;
        let mut copy = false;
        let t = &mut self.tabs[index];
        let (status_width, diagnostic_width) = {
            let v = t.view.lock().unwrap();
            let (label, _) = source_indicator(&v.status, pending_start);
            let width = ui
                .painter()
                .layout_no_wrap(
                    label.into(),
                    egui::TextStyle::Button.resolve(ui.style()),
                    Color32::WHITE,
                )
                .size()
                .x
                + 52.;
            (
                width,
                if v.diagnostic.is_empty() && v.error.is_empty() {
                    0.
                } else {
                    34.
                },
            )
        };
        let compact = ui.available_width() < 238. + status_width + diagnostic_width;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.;
            if icon_button(ui, Icon::Play, "Start source", false).clicked() {
                start = true;
            }
            if icon_button(ui, Icon::Stop, "Stop source", false).clicked() {
                stop = true;
            }
            if icon_button(
                ui,
                Icon::Follow,
                if t.follow {
                    "Auto-scroll enabled"
                } else {
                    "View paused — resume auto-scroll"
                },
                t.follow,
            )
            .clicked()
            {
                t.follow = true;
            }
            if !compact && icon_button(ui, Icon::Clear, "Clear buffered history", false).clicked() {
                clear = true;
            }
            let v = t.view.lock().unwrap();
            if t.selected.is_some_and(|id| v.selected_line(id).is_none()) {
                t.selected = None;
            }
            let filter_active =
                v.filtering() || !v.query.is_empty() || !v.exclude.is_empty() || v.regex;
            if icon_button(
                ui,
                Icon::Search,
                if filter_active {
                    "Search & filters · active"
                } else {
                    "Search & filters"
                },
                filter_active || t.filters_open,
            )
            .clicked()
            {
                t.filters_open = !t.filters_open;
            }
            let mut export = false;
            if compact {
                icon_menu(ui, Icon::More, "More log actions", false, |ui| {
                    copy = ui
                        .add_enabled(
                            t.selected.is_some(),
                            egui::Button::new("Copy selected line"),
                        )
                        .clicked();
                    export = ui
                        .add_enabled(
                            self.export.is_none(),
                            egui::Button::new("Export filtered logs…"),
                        )
                        .clicked();
                    clear = ui.button("Clear buffered history").clicked();
                    if copy || export || clear {
                        ui.close();
                    }
                });
            } else {
                ui.add_enabled_ui(t.selected.is_some(), |ui| {
                    copy = icon_button(ui, Icon::Copy, "Copy selected line", false).clicked();
                });
                ui.add_enabled_ui(self.export.is_none(), |ui| {
                    export = icon_button(ui, Icon::Export, "Export filtered logs", false).clicked();
                });
            }
            let snapshot = export.then(|| v.export_text());
            drop(v);
            // Native dialogs may stay open indefinitely. Never hold the reader lock here.
            if let Some(text) = snapshot
                && let Some(path) = rfd::FileDialog::new().set_file_name("logs.txt").save_file()
            {
                self.export = Some(write_export(&self.rt, ctx.clone(), path, text));
            }
            let v = t.view.lock().unwrap();
            if !v.diagnostic.is_empty() || !v.error.is_empty() {
                icon_menu(ui, Icon::Warning, "Source diagnostics", true, |ui| {
                    ui.label(&v.diagnostic);
                    ui.label(&v.error);
                });
            }
            let (label, color) = source_indicator(&v.status, pending_start);
            ui.allocate_ui_with_layout(egui::vec2(status_width, 32.), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(14., 20.), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.5, color);
                ui.menu_button(RichText::new(label).color(color).strong(), |ui| {
                    ui.label(&v.status);
                    if pending_start { ui.label("Connecting to start source — Stop cancels"); }
                    if !v.diagnostic.is_empty() { ui.label(&v.diagnostic); }
                    if !v.error.is_empty() { ui.label(&v.error); }
                    ui.label("Live means the reader is active, even when no new lines arrive. Auto-scroll is independent.");
                }).response.on_hover_text("Source status and diagnostics");
            });
        });
        let filter_rect = if t.filters_open {
            let response = ui.push_id(("inline-filters", &t.source.id), |ui| {
                let mut view = t.view.lock().unwrap();
                editors::filter_panel(ui, &mut view, &mut t.filters_open, self.settings.font)
            });
            if response.inner {
                let view = t.view.clone();
                self.rt
                    .spawn_blocking(move || view.lock().unwrap().refresh_filter());
            }
            Some(response.response.rect)
        } else {
            None
        };
        let (visible_len, empty) = {
            let v = t.view.lock().unwrap();
            (v.visible_len(), v.buffer().lines.is_empty())
        };
        if visible_len == 0 {
            let message = if empty {
                "No lines received yet"
            } else {
                "No matching lines — clear or change the filters"
            };
            ui.add(egui::Label::new(message).truncate())
                .on_hover_text(message);
        }
        if ui.rect_contains_pointer(ui.available_rect_before_wrap())
            && !filter_rect.is_some_and(|rect| ui.rect_contains_pointer(rect))
            && ui.input(|i| i.raw_scroll_delta.y > 0.)
        {
            t.follow = false
        }
        ui.spacing_mut().button_padding.y = 1.;
        ui.spacing_mut().item_spacing.y = 0.;
        let mut scroll_area = egui::ScrollArea::both()
            .min_scrolled_height(0.)
            .max_height(ui.available_height().max(0.))
            .stick_to_bottom(t.follow)
            .auto_shrink([false, false])
            .horizontal_scroll_offset(t.scroll.x);
        if !t.follow {
            scroll_area = scroll_area.vertical_scroll_offset(t.scroll.y);
        }
        let scroll_output =
            scroll_area.show_rows(ui, self.settings.font + 5., visible_len, |ui, range| {
                // Copy only the viewport, then release the ingestion lock before layout/paint.
                let lines: Vec<_> = {
                    let v = t.view.lock().unwrap();
                    (range.start..range.end.min(v.visible_len()))
                        .map(|i| v.visible_line(i).clone())
                        .collect()
                };
                for l in &lines {
                    let text = RichText::new(&l.text)
                        .monospace()
                        .size(self.settings.font)
                        .color(rgb(l.color.unwrap_or(self.settings.text)))
                        .background_color(rgb(l.background.unwrap_or(self.settings.background)));
                    if ui
                        .add(
                            egui::Button::selectable(t.selected == Some(l.id), text)
                                .wrap_mode(egui::TextWrapMode::Extend),
                        )
                        .clicked()
                    {
                        t.selected = Some(l.id)
                    }
                }
            });
        t.scroll = scroll_output.state.offset;
        if copy {
            if let Some(text) = t.copy_selection() {
                ctx.copy_text(text);
                self.copy_feedback = Some((
                    format!(
                        "Copied selected line · {} · {}",
                        self.origin(&self.tabs[index].source),
                        self.tabs[index].source.label()
                    ),
                    std::time::Instant::now(),
                ));
                ctx.request_repaint_after(std::time::Duration::from_secs(3));
            } else {
                self.status =
                    "Selected line is no longer visible — select a current line to copy".into();
            }
        }
        if clear {
            self.request_discard(index, DiscardAction::Clear);
        }
        if stop {
            if self.pending.as_ref().is_some_and(|p| matches!(&p.action, PendingAction::Start(id) if id == &self.tabs[index].source.id)) { self.cancel_pending(); }
            let t = &mut self.tabs[index];
            t.retire_reader(false);
            t.view
                .lock()
                .unwrap()
                .apply_rules_at(&self.settings.rules, self.rule_revision);
        }
        if start {
            self.start(index)
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if let Some(export) = &mut self.export {
            match export.try_recv() {
                Ok(result) => {
                    if let Err(error) = result {
                        self.status = error.to_string();
                    }
                    self.export = None;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    self.status = "Export worker stopped before reporting its result".into();
                    self.export = None;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
            }
        }
        self.frame_stats.record(frame.info().cpu_usage);
        crate::rendering::set_minimized(ctx.input(|i| i.viewport().minimized.unwrap_or(false)));
        #[cfg(feature = "qa")]
        if let Ok(path) = std::env::var("SSHLOGTAIL_QA_METRICS") {
            self.qa_frames += 1;
            let gap = self.qa_last_frame.elapsed().as_millis();
            self.qa_last_frame = std::time::Instant::now();
            if self.qa_frames > 30 {
                self.qa_max_gap_ms = self.qa_max_gap_ms.max(gap);
            }
            if self.qa_frames == 1 || self.qa_frames.is_multiple_of(100) {
                let tabs:Vec<_>=self.tabs.iter().map(|t|{let v=t.view.lock().unwrap();serde_json::json!({"buffered":v.buffer().lines.len(),"raw_bytes":v.buffer().bytes,"last_id":v.buffer().lines.back().map(|l|l.id)})}).collect();
                let _=std::fs::write(path,serde_json::to_vec_pretty(&serde_json::json!({"frames":self.qa_frames,"max_frame_gap_ms":self.qa_max_gap_ms,"tabs":tabs})).unwrap());
            }
        }
        #[cfg(feature = "qa")]
        if let Ok(path) = std::env::var("SSHLOGTAIL_QA_OUTPUT") {
            self.qa_frames += 1;
            // egui may discard a sizing pass (notably for modals). Request again
            // until a screenshot event arrives instead of losing a one-shot command.
            if self.qa_frames >= 15 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            for event in ctx.input(|i| i.events.clone()) {
                if let egui::Event::Screenshot { image, .. } = event {
                    let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                    image::save_buffer(
                        &path,
                        &bytes,
                        image.width() as u32,
                        image.height() as u32,
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        for (profile_index, session) in self
            .sessions
            .iter_mut()
            .enumerate()
            .filter_map(|(i, s)| s.as_mut().map(|s| (i, s)))
        {
            let profile = &self.settings.profiles[profile_index];
            self.states
                .insert(profile.id.clone(), session.state.borrow().clone());
            while let Ok(event) = session.events.try_recv() {
                match event {
                    Event::Error(message) => {
                        self.status = format!(
                            "{} ({}:{}): {}",
                            profile.name, profile.host, profile.port, message
                        );
                    }
                    Event::Files {
                        request,
                        path,
                        result,
                    } => {
                        if profile.id == self.browse_profile
                            && request == self.browser_request
                            && path == self.browser_path
                            && self.browse
                        {
                            match result {
                                Ok(files) => self.files = files,
                                Err(error) => self.browser_error = error,
                            }
                        }
                    }
                    Event::Trust {
                        host,
                        fingerprint,
                        reply,
                    } => {
                        self.trust.push_back(TrustRequest {
                            profile: profile.id.clone(),
                            generation: session.generation.clone(),
                            host,
                            key: fingerprint,
                            reply,
                        });
                    }
                }
            }
        }
        self.trust.retain(|t| {
            !t.reply.is_closed()
                && self
                    .settings
                    .profiles
                    .iter()
                    .position(|p| p.id == t.profile)
                    .and_then(|i| self.sessions[i].as_ref())
                    .is_some_and(|s| s.generation == t.generation)
        });
        self.advance_pending();
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(25, 29, 37);
        visuals.extreme_bg_color = rgb(self.settings.background);
        visuals.override_text_color = Some(rgb(self.settings.text));
        visuals.selection.bg_fill = rgb(self.settings.accent).gamma_multiply(0.20);
        visuals.selection.stroke = egui::Stroke::new(1_f32, rgb(self.settings.accent));
        visuals.widgets.inactive.bg_fill = rgb([30, 35, 44]);
        visuals.widgets.inactive.weak_bg_fill = rgb([30, 35, 44]);
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1_f32, rgb([47, 53, 64]));
        visuals.widgets.hovered.bg_fill = rgb([43, 49, 60]);
        visuals.widgets.hovered.weak_bg_fill = rgb([43, 49, 60]);
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1_f32, rgb(self.settings.accent));
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1_f32, rgb(self.settings.accent));
        ctx.set_visuals(visuals);
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(7.);
            ui.horizontal(|ui| {
                if icon_button(ui, Icon::Menu, "Toggle sidebar", false).clicked() {
                    self.sidebar = !self.sidebar
                }
                ui.add(egui::Image::new(&self.wordmark).fit_to_exact_size(
                    self.wordmark.size_vec2() * (28. / self.wordmark.size_vec2().y),
                ))
                .on_hover_text("DingoTail · Local & SSH logs");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::About, "About DingoTail", self.about).clicked() {
                        self.about = !self.about;
                    }
                    if icon_button(
                        ui,
                        Icon::Settings,
                        "Appearance & highlighting",
                        self.preferences,
                    )
                    .clicked()
                    {
                        self.set_preferences(!self.preferences);
                    }
                });
            });
            ui.add_space(7.);
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            if let Some(warning) = self.credential_warning.clone() {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(Color32::from_rgb(255, 190, 90), warning);
                    if ui.button("Dismiss").clicked() {
                        self.credential_warning = None;
                    }
                });
            }
            if self
                .copy_feedback
                .as_ref()
                .is_some_and(|(_, at)| at.elapsed().as_secs() >= 3)
            {
                self.copy_feedback = None;
            }
            ui.label(
                self.copy_feedback
                    .as_ref()
                    .map_or(self.status.as_str(), |(message, _)| message),
            );
        });
        if self.sidebar {
            #[cfg(feature = "qa")]
            let sidebar_width = std::env::var("DINGOTAIL_QA_SIDEBAR_WIDTH")
                .ok()
                .and_then(|s| s.parse::<f32>().ok())
                .unwrap_or(250.);
            #[cfg(not(feature = "qa"))]
            let sidebar_width = 250.;
            egui::SidePanel::left("servers")
                .default_width(sidebar_width)
                .width_range(230.0..=400.0)
                .show(ctx, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.set_min_height(32.);
                            ui.strong("Sources");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if icon_button(ui, Icon::Add, "Add server", false).clicked() {
                                    self.begin_edit(None);
                                }
                            });
                        });
                        let groups = self.groups();
                        for (group, title) in groups {
                            let expanded =
                                *self.settings.workspace.groups.get(&group).unwrap_or(&true);
                            let mut toggle = false;
                            let mut header =
                                egui::collapsing_header::CollapsingState::load_with_default_open(
                                    ui.ctx(),
                                    ui.make_persistent_id((&group, "sidebar")),
                                    expanded,
                                )
                                .show_header(ui, |ui| {
                                    if group != "local" {
                                        connection_indicator(ui, &self.state(&group), &title);
                                    }
                                    let r = ui
                                        .add_sized(
                                            egui::vec2(ui.available_width(), 24.),
                                            egui::Button::new(&title)
                                                .frame(true)
                                                .frame_when_inactive(false)
                                                .wrap_mode(egui::TextWrapMode::Truncate),
                                        )
                                        .on_hover_text(&title);
                                    toggle = r.clicked();
                                });
                            if toggle {
                                header.toggle();
                            }
                            let expanded_now = header.is_open();
                            let _heading = header.body(|ui| {
                                ui.horizontal(|ui| {
                                    if group == "local" {
                                        icon_menu(
                                            ui,
                                            Icon::Folder,
                                            "Open local files or path",
                                            false,
                                            |ui| {
                                                if ui.button("Choose local files…").clicked() {
                                                    self.pick_local_files();
                                                    ui.close();
                                                }
                                                ui.separator();
                                                ui.label("Absolute local path");
                                                if theme::input_actions(
                                                    ui,
                                                    &mut self.local_path,
                                                    &[(Icon::Play, "Open local path")],
                                                ) == Some(0)
                                                {
                                                    let path = self.local_path.clone();
                                                    if std::path::Path::new(&path).is_absolute() {
                                                        self.open_source(Source {
                                                            id: new_id(),
                                                            profile: String::new(),
                                                            path,
                                                            kind: SourceKind::LocalFile,
                                                            sudo: false,
                                                        });
                                                        ui.close();
                                                    } else {
                                                        self.status =
                                                            "Use an absolute local file path"
                                                                .into();
                                                    }
                                                }
                                            },
                                        );
                                    } else if let Some(index) =
                                        self.settings.profiles.iter().position(|p| p.id == group)
                                    {
                                        if icon_button(ui, Icon::Folder, "Open remote log", false)
                                            .clicked()
                                        {
                                            self.begin_open(index);
                                        }
                                        let state = self.state(&group);
                                        if state == ConnectionState::Connected {
                                            icon_menu(ui, Icon::Connect, "Server connection actions", false, |ui| {
                                                let stopped = self.sessions[index].as_ref().is_some_and(|s| self.reconnected.get(&group) == Some(&s.generation));
                                                ui.label(if stopped { "Connected — sources remain stopped" } else { "Connected" });
                                                ui.label("Reconnect stops this server’s sources and retains buffered output.");
                                                if ui.button("Reconnect server…").clicked() {
                                                    self.request_reconnect(index);
                                                    ui.close();
                                                }
                                            });
                                        } else {
                                            ui.add_enabled_ui(!state.pending(), |ui| {
                                                if icon_button(ui, Icon::Connect, "Connect server", false).clicked() {
                                                    self.request_operation_for(group.clone(), PendingAction::Connect);
                                                }
                                            });
                                        }
                                        if icon_button(ui, Icon::Edit, "Edit connection", false)
                                            .clicked()
                                        {
                                            self.begin_edit(Some(index));
                                        }
                                        if icon_button(ui, Icon::Delete, "Delete server", false)
                                            .clicked()
                                        {
                                            self.delete_server = Some(group.clone());
                                        }
                                    }
                                });
                                let mut select = None;
                                let mut close = None;
                                for (index, tab) in self
                                    .tabs
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, t)| t.source.group() == group)
                                {
                                    ui.horizontal(|ui| {
                                        if ui
                                            .add_sized(
                                                egui::vec2(
                                                    (ui.available_width() - 38.).max(1.),
                                                    32.,
                                                ),
                                                egui::Button::selectable(
                                                    self.settings.workspace.panes
                                                        [self.settings.workspace.active_pane]
                                                        .as_ref()
                                                        == Some(&tab.source.id),
                                                    tab.source.label(),
                                                )
                                                .wrap_mode(egui::TextWrapMode::Truncate),
                                            )
                                            .on_hover_text(&tab.source.path)
                                            .clicked()
                                        {
                                            select = Some(index)
                                        }
                                        if icon_button(ui, Icon::Close, "Close source", false)
                                            .clicked()
                                        {
                                            close = Some(index)
                                        }
                                    });
                                }
                                if let Some(index) = select {
                                    self.focus_source(index)
                                }
                                if let Some(index) = close {
                                    self.request_discard(index, DiscardAction::Close)
                                }
                            });
                            self.settings.workspace.groups.insert(group, expanded_now);
                        }
                    });
                });
        }
        if self.discard.is_some() {
            if let Some(index) = self.discard_index() {
                let tab = &self.tabs[index];
                let count = tab.view.lock().unwrap().buffer().lines.len();
                let action = self.discard.as_ref().unwrap().action;
                let title = match action {
                    DiscardAction::Clear => "Clear buffered history",
                    DiscardAction::Close => "Close source",
                };
                let identity = format!("{} · {}", self.origin(&tab.source), tab.source.path);
                let mut confirm = false;
                let mut cancel = false;
                egui::Modal::new(egui::Id::new("discard_history")).show(ctx, |ui| {
                    ui.set_max_width(420.);
                    ui.heading(format!("{title}?"));
                    ui.label(identity);
                    let unit = if count == 1 { "line" } else { "lines" };
                    ui.label(format!("Discard {count} retained {unit}, including lines hidden by filters. Original log files are untouched."));
                    ui.label(match action {
                        DiscardAction::Clear => "Incoming logs will continue if the source is running.",
                        DiscardAction::Close => "The reader stops and this source is removed from the saved workspace.",
                    });
                    ui.horizontal(|ui| {
                        cancel = ui.button("Cancel").clicked();
                        confirm = ui.button(title).clicked();
                    });
                });
                if cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.discard = None;
                } else if confirm {
                    self.confirm_discard();
                }
            } else {
                self.discard = None;
            }
        }
        if let Some(request) = &self.reconnect {
            let valid = self
                .settings
                .profiles
                .iter()
                .position(|p| p.id == request.profile)
                .filter(|&i| {
                    self.sessions[i]
                        .as_ref()
                        .is_some_and(|s| s.generation == request.generation)
                });
            if let Some(index) = valid {
                let p = &self.settings.profiles[index];
                let count = self.active_readers(&p.id);
                let mut confirm = false;
                let mut cancel = false;
                egui::Modal::new(egui::Id::new("reconnect_server")).show(ctx, |ui| {
                    ui.set_max_width(420.);
                    ui.heading("Reconnect server?");
                    ui.label(format!("{} ({}:{})", p.name, p.host, p.port));
                    ui.label(format!("Stop {count} active reader(s) and reconnect. Buffered output is retained. Sources remain stopped until you start them individually."));
                    ui.horizontal(|ui| {
                        cancel = ui.button("Cancel").clicked();
                        confirm = ui.button("Reconnect server").clicked();
                    });
                });
                if cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.reconnect = None;
                } else if confirm {
                    self.confirm_reconnect();
                }
            } else {
                self.reconnect = None;
            }
        }
        if let Some(id) = self.delete_server.clone() {
            if let Some(profile) = self.settings.profiles.iter().find(|p| p.id == id) {
                let title = format!("{} ({}:{})", profile.name, profile.host, profile.port);
                let count = self.tabs.iter().filter(|t| t.source.group() == id).count();
                let mut confirm = false;
                let mut cancel = false;
                egui::Modal::new(egui::Id::new("delete_server_confirmation")).show(ctx, |ui| {
                    ui.set_max_width(420.);
                    ui.heading("Delete server?");
                    ui.label(title);
                    ui.label(format!("Disconnect this server and close its {count} open log(s). The server will be removed from your saved workspace."));
                    ui.label("Log files are untouched. Saved credentials and host-key trust are retained; manage them in Edit connection and Settings.");
                    ui.horizontal(|ui| {
                        cancel = ui.button("Cancel").clicked();
                        confirm = ui.button("Delete server").clicked();
                    });
                });
                if confirm {
                    self.delete_server(&id);
                }
                if confirm || cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.delete_server = None;
                }
            } else {
                self.delete_server = None;
            }
        }
        if self.edit_connection {
            let mut open = true;
            let mut save = false;
            let mut cancel = false;
            if let Some(draft) = &mut self.draft {
                let old = self
                    .settings
                    .profiles
                    .iter()
                    .position(|p| p.id == draft.profile.id);
                let changes_endpoint = old.is_some_and(|i| {
                    endpoint_changed(&self.settings.profiles[i], &draft.profile)
                        || self.secrets[i] != draft.secret
                });
                theme::dialog(ctx, "Edit connection", "connection", 480., 510.)
                    .open(&mut open).show(ctx, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    egui::ScrollArea::vertical().max_height((ui.available_height() - 50.).max(100.)).show(ui, |ui| {
                        ui.label("Name");
                        ui.add(egui::TextEdit::singleline(&mut draft.profile.name).desired_width(f32::INFINITY));
                        ui.horizontal(|ui| {
                            let available = ui.available_width();
                            ui.vertical(|ui| {
                                ui.set_width((available - 90.).max(120.));
                                ui.label("Host / IP");
                                ui.add(egui::TextEdit::singleline(&mut draft.profile.host).desired_width(f32::INFINITY));
                            });
                            ui.vertical(|ui| {
                                ui.label("Port");
                                ui.add(egui::DragValue::new(&mut draft.profile.port).range(1..=65535));
                            });
                        });
                        ui.label("Username");
                        ui.add(egui::TextEdit::singleline(&mut draft.profile.user).desired_width(f32::INFINITY));
                        ui.label("Private key (leave empty for password)");
                        if theme::input_actions(ui, &mut draft.profile.key, &[(Icon::Folder, "Choose private key")]) == Some(0)
                            && let Some(path) = rfd::FileDialog::new().pick_file() { match crate::platform::unicode_path(&path) {
                                Ok(path) => draft.profile.key = path,
                                Err(error) => draft.error = error.to_string(),
                            } }
                        ui.label(if draft.profile.key.is_empty() { "Password" } else { "Key passphrase" });
                        ui.add(egui::TextEdit::singleline(&mut draft.secret).password(true).desired_width(f32::INFINITY));
                        ui.checkbox(&mut draft.profile.remember, "Remember secret securely when connecting");
                        if let Some(i) = old
                            && ui.button("Forget saved secret").clicked() {
                                let p = &self.settings.profiles[i];
                                match crate::credentials::delete(p) {
                                    Ok(()) | Err(keyring::Error::NoEntry) => { self.secrets[i].clear(); draft.secret.clear(); draft.profile.remember = false; draft.error = "Saved secret removed".into(); }
                                    Err(error) => draft.error = error.to_string(),
                                }
                            }
                        if changes_endpoint {
                            ui.colored_label(Color32::from_rgb(255, 190, 90), "Saving changed connection details disconnects this server and stops its sources. Existing logs remain labeled with their original server. Restarting clears them; reconnect explicitly when ready.");
                        }
                        if !draft.error.is_empty() { ui.label(&draft.error); }
                    });
                    ui.separator();
                    ui.horizontal(|ui| { save = ui.button("Save changes").clicked(); cancel = ui.button("Cancel").clicked(); });
                });
            }
            if save {
                self.apply_draft();
            }
            if cancel || !open {
                self.draft = None;
                self.edit_connection = false;
            }
        }
        if self.open_remote {
            let mut open = true;
            let mut requested = None;
            let mut cancel = false;
            let id = self.source.profile.clone();
            let title = self
                .settings
                .profiles
                .iter()
                .find(|p| p.id == id)
                .map(|p| format!("Open remote log · {} ({}:{})", p.name, p.host, p.port))
                .unwrap_or_else(|| "Open remote log".into());
            theme::dialog(ctx, title, "source", 560., 245.).open(&mut open).show(ctx, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                ui.horizontal(|ui| { connection_indicator(ui, &self.state(&id), &id); ui.label(self.state(&id).label()); });
                egui::ScrollArea::vertical().max_height((ui.available_height() - 80.).max(85.)).show(ui, |ui| {
                    ui.add_enabled_ui(self.pending.is_none(), |ui| {
                        let previous_kind = self.source.kind;
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.source.kind, SourceKind::SshFile, "File");
                            ui.selectable_value(&mut self.source.kind, SourceKind::Journal, "System journal");
                        });
                        if previous_kind != self.source.kind { self.source.path = if self.source.is_journal() { String::new() } else { "/var/log/syslog".into() }; }
                        ui.label(if self.source.is_journal() { "Unit (leave empty for all units)" } else { "Absolute file path" });
                        let actions = if self.source.is_journal() { vec![(Icon::Play, "Open journal")] } else { vec![(Icon::Folder, "Browse remote files"), (Icon::Play, "Open log")] };
                        if let Some(action) = theme::input_actions(ui, &mut self.source.path, &actions) {
                            if !self.source.is_journal() && action == 0 { requested = Some(PendingAction::Browse(self.directory.clone())); }
                            else if !self.source.is_journal() && !self.source.path.starts_with('/') { self.operation_error = "Enter an absolute remote path starting with /.".into(); }
                            else { let mut source = self.source.clone(); source.id = new_id(); requested = Some(PendingAction::Open(source)); }
                        }
                        ui.checkbox(&mut self.source.sudo, "Passwordless sudo (log command only)");
                    });
                    if !self.operation_error.is_empty() { ui.colored_label(Color32::LIGHT_RED, &self.operation_error); }
                    if self.pending.is_some() { ui.label("Connecting… complete any credential or host-key prompt to continue."); }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    cancel = ui.button(if self.pending.is_some() { "Cancel operation" } else { "Close" }).clicked();
                    if self.pending.is_none() && ui.button("Edit connection").clicked()
                        && let Some(i) = self.settings.profiles.iter().position(|p| p.id == id) { self.begin_edit(Some(i)); }
                });
            });
            if let Some(action) = requested {
                self.request_operation(action);
            }
            if cancel || !open {
                self.cancel_pending();
                self.open_remote = false;
            }
        }
        if let Some(id) = self.credential_profile.clone() {
            let mut open = true;
            let mut connect = false;
            let mut cancel = false;
            if let Some(i) = self.settings.profiles.iter().position(|p| p.id == id) {
                let p = &self.settings.profiles[i];
                theme::dialog(
                    ctx,
                    format!("Credentials · {}", p.name),
                    "credentials",
                    430.,
                    200.,
                )
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(format!("{}@{}:{}", p.user, p.host, p.port));
                    ui.label(if p.key.is_empty() {
                        "Password"
                    } else {
                        "Private-key passphrase (check the key path if this fails)"
                    });
                    ui.add(
                        egui::TextEdit::singleline(&mut self.credential_secret)
                            .password(true)
                            .desired_width(f32::INFINITY),
                    );
                    ui.checkbox(&mut self.credential_remember, "Remember secret securely");
                    ui.horizontal(|ui| {
                        connect = ui.button("Connect and continue").clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                });
                if connect {
                    self.secrets[i] = std::mem::take(&mut self.credential_secret);
                    self.settings.profiles[i].remember = self.credential_remember;
                    self.credential_profile = None;
                    self.connect_pending(i);
                }
            }
            if cancel || !open {
                self.cancel_pending();
            }
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(rgb(self.settings.background))
                    .inner_margin(8.),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (count, icon, label) in [
                        (1, Icon::Single, "Single viewer"),
                        (2, Icon::Columns, "Two columns"),
                        (4, Icon::Grid, "Four panes"),
                    ] {
                        if icon_button(ui, icon, label, self.settings.workspace.layout == count)
                            .clicked()
                        {
                            self.settings.workspace.layout = count;
                            self.settings.workspace.active_pane =
                                self.settings.workspace.active_pane.min(count - 1);
                            self.save();
                        }
                    }
                    let (strip_rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 30.),
                        egui::Sense::hover(),
                    );
                    ui.scope_builder(egui::UiBuilder::new().max_rect(strip_rect), |ui| {
                        ui.set_clip_rect(strip_rect);
                        egui::ScrollArea::horizontal()
                            .id_salt("grouped-tabs")
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                                    let mut chosen = None;
                                    for (group, _) in self.groups() {
                                        if self.tabs.iter().any(|t| t.source.group() == group) {
                                            ui.separator();
                                            for (i, t) in self
                                                .tabs
                                                .iter()
                                                .enumerate()
                                                .filter(|(_, t)| t.source.group() == group)
                                            {
                                                let label = format!(
                                                    "{} · {}",
                                                    self.origin(&t.source),
                                                    t.source.label()
                                                );
                                                if ui
                                                    .selectable_label(
                                                        self.settings.workspace.panes
                                                            [self.settings.workspace.active_pane]
                                                            .as_ref()
                                                            == Some(&t.source.id),
                                                        label,
                                                    )
                                                    .on_hover_text(&t.source.path)
                                                    .clicked()
                                                {
                                                    chosen = Some(i);
                                                }
                                            }
                                        }
                                    }
                                    if let Some(i) = chosen {
                                        self.focus_source(i);
                                    }
                                });
                            });
                    });
                });
                ui.separator();
                if self.tabs.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(30.);
                        ui.add(
                            egui::Image::new(&self.mascot).fit_to_exact_size(egui::vec2(84., 84.)),
                        );
                        ui.heading("Follow your logs");
                        ui.label("Open local files or connect to a server.");
                        if ui.button("Open local files…").clicked() {
                            self.pick_local_files();
                        }
                        if ui.button("Add server").clicked() {
                            self.begin_edit(None);
                        }
                    });
                    return;
                }
                let area = ui.available_rect_before_wrap();
                let count = self.settings.workspace.layout;
                let columns = if count == 1 { 1 } else { 2 };
                let rows = if count == 4 { 2 } else { 1 };
                let gap = 8.;
                let width = (area.width() - gap * (columns - 1) as f32) / columns as f32;
                let height = (area.height() - gap * (rows - 1) as f32) / rows as f32;
                for pane in 0..count {
                    let min = area.min
                        + egui::vec2(
                            (pane % columns) as f32 * (width + gap),
                            (pane / columns) as f32 * (height + gap),
                        );
                    let rect =
                        egui::Rect::from_min_size(min, egui::vec2(width.max(1.), height.max(1.)));
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .id_salt(("pane", pane))
                            .max_rect(rect),
                        |ui| {
                            ui.set_clip_rect(rect);
                            ui.spacing_mut().item_spacing.y = 2.;
                            ui.spacing_mut().button_padding.y = 2.;
                            ui.painter().rect_stroke(
                                rect,
                                2.,
                                egui::Stroke::new(
                                    1_f32,
                                    if self.settings.workspace.active_pane == pane {
                                        rgb(self.settings.accent)
                                    } else {
                                        rgb([48, 55, 67])
                                    },
                                ),
                                egui::StrokeKind::Inside,
                            );
                            if ui.rect_contains_pointer(rect)
                                && ui.input(|i| i.pointer.any_pressed())
                            {
                                self.settings.workspace.active_pane = pane;
                            }
                            let source_details = self.settings.workspace.panes[pane]
                                .as_ref()
                                .and_then(|id| self.tabs.iter().find(|t| &t.source.id == id))
                                .map(|t| {
                                    format!(
                                        "{}\n{}",
                                        self.origin(&t.source),
                                        if t.source.path.is_empty() {
                                            "System journal (all units)"
                                        } else {
                                            &t.source.path
                                        }
                                    )
                                })
                                .unwrap_or_else(|| "Choose an open log".into());
                            egui::ComboBox::from_id_salt(("source-picker", pane))
                                .width(ui.available_width())
                                .truncate()
                                .selected_text(
                                    self.settings.workspace.panes[pane]
                                        .as_ref()
                                        .and_then(|id| {
                                            self.tabs.iter().find(|t| &t.source.id == id)
                                        })
                                        .map(|t| {
                                            format!(
                                                "{} · {}",
                                                self.origin(&t.source),
                                                t.source.label()
                                            )
                                        })
                                        .unwrap_or_else(|| "Choose an open log…".into()),
                                )
                                .show_ui(ui, |ui| {
                                    if let Some(tab) =
                                        self.settings.workspace.panes[pane].as_ref().and_then(
                                            |id| self.tabs.iter().find(|t| &t.source.id == id),
                                        )
                                    {
                                        ui.menu_button("Source details…", |ui| {
                                            ui.label(self.origin(&tab.source));
                                            ui.label(if tab.source.path.is_empty() {
                                                "System journal (all units)"
                                            } else {
                                                &tab.source.path
                                            });
                                        });
                                        ui.separator();
                                    }
                                    let mut selected = None;
                                    for (group, title) in self.groups() {
                                        ui.strong(title);
                                        for (i, t) in self
                                            .tabs
                                            .iter()
                                            .enumerate()
                                            .filter(|(_, t)| t.source.group() == group)
                                        {
                                            if ui
                                                .selectable_label(
                                                    false,
                                                    format!(
                                                        "{} · {}",
                                                        self.origin(&t.source),
                                                        t.source.label()
                                                    ),
                                                )
                                                .on_hover_text(&t.source.path)
                                                .clicked()
                                            {
                                                selected = Some(i)
                                            }
                                        }
                                    }
                                    if let Some(i) = selected {
                                        self.settings.workspace.active_pane = pane;
                                        self.focus_source(i)
                                    }
                                })
                                .response
                                .on_hover_text(source_details);
                            if let Some(index) = self.settings.workspace.panes[pane]
                                .as_ref()
                                .and_then(|id| self.tabs.iter().position(|t| &t.source.id == id))
                            {
                                self.viewer(ui, index)
                            } else {
                                ui.label("Choose a log from this menu or the sidebar.");
                            }
                        },
                    );
                }
            });
        if self.browse {
            let mut open = true;
            theme::dialog(
                ctx,
                format!(
                    "Remote files · {}",
                    self.settings
                        .profiles
                        .iter()
                        .find(|p| p.id == self.browse_profile)
                        .map(|p| format!("{} ({}:{})", p.name, p.host, p.port))
                        .unwrap_or_default()
                ),
                "browser",
                600.,
                420.,
            )
            .open(&mut open)
            .show(ctx, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                match theme::input_actions(
                    ui,
                    &mut self.directory,
                    &[
                        (Icon::Play, "Go to directory"),
                        (Icon::Up, "Parent directory"),
                    ],
                ) {
                    Some(0) => self.browse_request(),
                    Some(1) => {
                        self.directory = self
                            .directory
                            .trim_end_matches('/')
                            .rsplit_once('/')
                            .map(|(p, _)| if p.is_empty() { "/" } else { p })
                            .unwrap_or("/")
                            .into();
                        self.browse_request();
                    }
                    _ => {}
                }
                if ui.checkbox(&mut self.browser_sudo, "Browse with sudo (read-only)")
                    .on_hover_text("Uses sudo -n to launch the existing remote sftp-server in read-only mode. Requires existing passwordless sudo authorization; no server configuration changes.").changed() {
                    self.browse_request();
                }
                if self.browser_sudo { ui.label("Elevated browsing only. Enable Passwordless sudo separately to read the selected log."); }
                if !self.browser_error.is_empty() {
                    ui.colored_label(Color32::LIGHT_RED, &self.browser_error);
                }
                let mut navigate = None;
                let mut selected_file = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (name, dir) in &self.files {
                        if name == "." || name == ".." {
                            continue;
                        }
                        if ui
                            .selectable_label(
                                false,
                                format!("{}  {name}", if *dir { "Folder" } else { "File" }),
                            )
                            .clicked()
                        {
                            let path =
                                format!("{}/{}", self.browser_path.trim_end_matches('/'), name);
                            if *dir {
                                navigate = Some(path)
                            } else {
                                selected_file = Some(name.clone());
                            }
                        }
                    }
                });
                if let Some(name) = selected_file {
                    self.select_browser_file(&name);
                }
                if let Some(path) = navigate {
                    self.directory = path;
                    self.browse_request()
                }
            });
            if !open {
                self.close_browser();
            }
        }

        if self.about {
            let mut open = true;
            theme::dialog(ctx, "About DingoTail", "about", 380., 390.)
                .open(&mut open)
                .vscroll(true)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(12.);
                        ui.add(
                            egui::Image::new(&self.mascot)
                                .fit_to_exact_size(egui::vec2(144., 144.)),
                        )
                        .on_hover_text("Dingo mascot");
                        ui.add_space(8.);
                        ui.add(egui::Image::new(&self.wordmark).fit_to_exact_size(
                            self.wordmark.size_vec2() * (280. / self.wordmark.size_vec2().x),
                        ))
                        .on_hover_text("DingoTail");
                        ui.add_space(12.);
                        ui.label("Rust Edition");
                        ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                        ui.label("Copyright © 2026 Juan Pablo Devoto");
                        ui.label("Code: GNU GPL v3 · No warranty");
                        ui.label("DingoTail name and logos: rights reserved");
                        ui.collapsing("License & branding terms", |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(240.)
                                .show(ui, |ui| {
                                    ui.set_max_width(340.);
                                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                                    ui.label(include_str!("../COPYRIGHT"));
                                    ui.separator();
                                    ui.label(include_str!("../BRANDING.md"));
                                    ui.separator();
                                    ui.label(include_str!("../LICENSE"));
                                });
                        });
                        ui.add_space(6.);
                        ui.hyperlink_to("dingodicom.com", "https://dingodicom.com/")
                            .on_hover_text("Open https://dingodicom.com/ in your browser");
                        ui.hyperlink_to(
                            "GitHub · jpdevoto/dingo-tail",
                            "https://github.com/jpdevoto/dingo-tail-rust",
                        )
                        .on_hover_text("Open the DingoTail source repository on GitHub");
                        ui.add_space(12.);
                    });
                });
            self.about = open;
        }
        self.appearance_window(ctx);
        if let Some(trust) = self.trust.front() {
            let mut answer = None;
            theme::dialog(ctx, "Verify SSH server identity", "trust", 530., 225.).show(ctx, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                ui.label(&trust.host);
                ui.add(egui::Label::new(RichText::new(&trust.key).monospace()).wrap());
                ui.label("Verify this fingerprint with your server administrator before trusting.");
                ui.horizontal(|ui| {
                    if ui.button("Trust and connect").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Reject").clicked() {
                        answer = Some(false);
                    }
                });
            });
            if let Some(yes) = answer
                && let Some(t) = self.trust.pop_front()
            {
                let valid = !t.reply.is_closed()
                    && self
                        .settings
                        .profiles
                        .iter()
                        .position(|p| p.id == t.profile)
                        .and_then(|i| self.sessions[i].as_ref())
                        .is_some_and(|s| s.generation == t.generation);
                if valid {
                    if yes {
                        self.settings.trusted.insert(t.host, t.key);
                        self.save();
                    }
                    let _ = t.reply.send(yes);
                }
            }
        }
    }
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        self.save();
        for session in self.sessions.iter().flatten() {
            let _ = session.tx.try_send(Request::Stop);
        }
        for t in &mut self.tabs {
            t.cancel.take();
        }
    }
}

fn write_export(
    rt: &tokio::runtime::Runtime,
    ctx: egui::Context,
    path: std::path::PathBuf,
    text: String,
) -> tokio::sync::oneshot::Receiver<std::io::Result<()>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    rt.spawn_blocking(move || {
        let _ = tx.send(std::fs::write(path, text));
        ctx.request_repaint();
    });
    rx
}

fn endpoint_changed(a: &Profile, b: &Profile) -> bool {
    (a.host.as_str(), a.port, a.user.as_str(), a.key.as_str())
        != (b.host.as_str(), b.port, b.user.as_str(), b.key.as_str())
}
fn pane_filter_summary(view: &View) -> String {
    let mut text = String::new();
    if view.filtering() {
        text.push_str(&view.filter_summary);
    }
    if view.filter_pending() {
        if !text.is_empty() {
            text.push_str(" · ");
        }
        text.push_str("Updating filter…");
    } else if !view.error.is_empty() {
        if !text.is_empty() {
            text.push_str(" · ");
        }
        text.push_str("Showing results from the previous valid filter");
    }
    text
}
#[cfg(test)]
#[test]
fn reader_indicators_distinguish_lifecycle_states() {
    let green = Color32::from_rgb(95, 214, 145);
    let amber = Color32::from_rgb(255, 190, 90);
    let gray = Color32::from_rgb(155, 163, 177);
    let red = Color32::from_rgb(255, 113, 123);
    for state in ["Live · local", "Live · SSH"] {
        assert_eq!(source_indicator(state, false), ("Live", green));
    }
    for state in ["Connecting stream", "Reconnecting", "Waiting for source"] {
        assert_eq!(source_indicator(state, false).1, amber);
    }
    for state in ["Stopped", "Exited (0)"] {
        assert_eq!(source_indicator(state, false).1, gray);
    }
    for state in [
        "Exited (1)",
        "Error",
        "Connection unavailable",
        "Missing server",
        "Connection failed",
    ] {
        assert_eq!(source_indicator(state, false).1, red);
    }
    assert_eq!(source_indicator("Stopped", true), ("Connecting", amber));
}
fn source_indicator(status: &str, pending: bool) -> (&str, Color32) {
    if pending {
        return ("Connecting", Color32::from_rgb(255, 190, 90));
    }
    if status.starts_with("Live") {
        ("Live", Color32::from_rgb(95, 214, 145))
    } else if status.starts_with("Connecting")
        || status.starts_with("Reconnecting")
        || status.starts_with("Waiting")
    {
        (
            if status.starts_with("Waiting") {
                "Waiting"
            } else if status.starts_with("Reconnecting") {
                "Reconnecting"
            } else {
                "Connecting"
            },
            Color32::from_rgb(255, 190, 90),
        )
    } else if status.starts_with("Stopped")
        || status == "QA sample (stopped)"
        || status == "Exited (0)"
    {
        (
            if status == "Exited (0)" {
                "Exited (0)"
            } else {
                "Stopped"
            },
            Color32::from_rgb(155, 163, 177),
        )
    } else {
        ("Error", Color32::from_rgb(255, 113, 123))
    }
}
fn connection_indicator(ui: &mut egui::Ui, state: &ConnectionState, endpoint: &str) {
    let color = match state {
        ConnectionState::Disconnected => Color32::from_rgb(155, 163, 177),
        ConnectionState::Connected => Color32::from_rgb(95, 214, 145),
        ConnectionState::Failed(_) => Color32::from_rgb(255, 113, 123),
        _ => Color32::from_rgb(255, 190, 90),
    };
    let detail = match state {
        ConnectionState::Failed(error) => format!("{}: {error}", state.label()),
        _ => state.label().into(),
    };
    let label = format!("{endpoint} — {detail}");
    let (rect, response) = ui.allocate_exact_size(egui::vec2(18., 24.), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.5, color);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
    response.on_hover_text(label);
}
fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

#[cfg(test)]
#[path = "workflow_tests.rs"]
mod workflow_tests;

#[cfg(test)]
mod workspace_tests {
    use super::*;

    #[test]
    fn export_worker_writes_snapshot_and_reports_io_errors() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = egui::Context::default();
        let dir = std::env::temp_dir().join(format!("dingotail-export-{}", new_id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("logs.txt");
        let text = "\nINFO é\nERROR 😀";
        rt.block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                write_export(&rt, ctx.clone(), path.clone(), text.into()),
            )
            .await
            .unwrap()
            .unwrap()
            .unwrap();
            let failed = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                write_export(&rt, ctx, dir.join("missing/logs.txt"), "data".into()),
            )
            .await
            .unwrap()
            .unwrap();
            assert!(failed.is_err());
        });
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    fn fixture() -> App {
        let profile = Profile {
            name: "Original".into(),
            host: "old.example".into(),
            user: "reader".into(),
            ..Default::default()
        };
        let mut settings = Settings::default();
        settings.sources.push(Source {
            profile: profile.id.clone(),
            path: "/var/log/app.log".into(),
            ..Default::default()
        });
        settings.profiles.push(profile);
        App::from_settings(&egui::Context::default(), settings, false, String::new())
    }
    #[test]
    fn browser_selection_uses_loaded_path_and_owning_server() {
        let mut app = fixture();
        let id = app.settings.profiles[0].id.clone();
        app.source.profile = id.clone();
        app.browse_profile = id;
        app.browser_path = "/var/log".into();
        app.directory = "/tmp/unsubmitted".into();
        app.select_browser_file("app.log");
        assert_eq!(app.source.path, "/var/log/app.log");
        app.source.profile = "other-server".into();
        app.select_browser_file("wrong.log");
        assert_eq!(app.source.path, "/var/log/app.log");
        app.browse = true;
        app.browser_sudo = true;
        app.files.push(("stale.log".into(), false));
        let request = app.browser_request;
        app.begin_open(0);
        assert!(!app.browse);
        assert!(!app.browser_sudo);
        assert!(app.files.is_empty());
        assert!(app.browser_request > request);
    }
    #[test]
    fn connection_drafts_preserve_live_state_and_retained_identity() {
        let mut app = fixture();
        app.sessions[0] = Some(simulated_session(&app.rt));
        let generation = app.sessions[0].as_ref().unwrap().generation.clone();
        app.tabs[0].view.lock().unwrap().push("original log".into());
        app.begin_edit(Some(0));
        app.draft.as_mut().unwrap().profile.host = "draft.example".into();
        assert_eq!(app.settings.profiles[0].host, "old.example");
        assert_eq!(app.sessions[0].as_ref().unwrap().generation, generation);
        app.draft = None; // Cancel is discard-only.
        app.begin_edit(Some(0));
        app.draft.as_mut().unwrap().profile.name = "Renamed".into();
        app.apply_draft();
        assert_eq!(app.sessions[0].as_ref().unwrap().generation, generation);
        assert_eq!(app.tabs[0].view.lock().unwrap().buffer().lines.len(), 1);
        app.begin_edit(Some(0));
        app.draft.as_mut().unwrap().profile.host = "new.example".into();
        app.apply_draft();
        assert!(app.sessions[0].is_none());
        assert!(app.origin(&app.tabs[0].source).contains("old.example"));
        assert_eq!(app.tabs[0].view.lock().unwrap().buffer().lines.len(), 1);
        app.sessions[0] = Some(simulated_session(&app.rt));
        app.start(0);
        assert_eq!(app.tabs[0].view.lock().unwrap().buffer().lines.len(), 0);
        assert!(app.origin(&app.tabs[0].source).contains("new.example"));
    }
    #[test]
    fn pending_open_runs_once_and_rejects_replaced_sessions() {
        let mut app = fixture();
        let id = app.settings.profiles[0].id.clone();
        app.source.profile = id.clone();
        app.sessions[0] = Some(simulated_session(&app.rt));
        app.states.insert(id.clone(), ConnectionState::Connected);
        let source = Source {
            profile: id.clone(),
            path: "/var/log/other.log".into(),
            ..Default::default()
        };
        app.request_operation(PendingAction::Open(source.clone()));
        app.advance_pending();
        app.advance_pending();
        assert_eq!(app.tabs.len(), 2);
        app.request_operation(PendingAction::Open(source.clone()));
        app.advance_pending();
        assert_eq!(app.tabs.len(), 2);
        app.request_operation(PendingAction::Open(Source {
            path: "/var/log/obsolete.log".into(),
            ..source
        }));
        app.sessions[0] = Some(simulated_session(&app.rt));
        app.advance_pending();
        assert!(app.pending.is_none());
        assert_eq!(app.tabs.len(), 2);
        assert!(app.operation_error.contains("replaced"));
    }
    #[test]
    fn cancelling_pending_open_cannot_create_source_or_leave_trust() {
        let mut app = fixture();
        let id = app.settings.profiles[0].id.clone();
        app.sessions[0] = Some(simulated_session(&app.rt));
        let generation = app.sessions[0].as_ref().unwrap().generation.clone();
        let (reply, receiver) = tokio::sync::oneshot::channel();
        app.trust.push_back(TrustRequest {
            profile: id.clone(),
            generation: generation.clone(),
            host: "old.example:22".into(),
            key: "fixture".into(),
            reply,
        });
        app.pending = Some(PendingOperation {
            profile: id,
            generation: Some(generation),
            action: PendingAction::Open(Source::default()),
            owns_connection: true,
        });
        app.cancel_pending();
        app.advance_pending();
        assert_eq!(app.tabs.len(), 1);
        assert!(app.sessions[0].is_none());
        assert!(app.trust.is_empty());
        assert!(receiver.blocking_recv().is_err());
    }
    #[test]
    fn background_log_updates_wake_an_idle_view() {
        let ctx = egui::Context::default();
        let tab = App::tab(&ctx, Source::default(), &[]);
        tab.view.lock().unwrap().set_status("Live · local");
        for _ in 0..3 {
            let _ = ctx.run(egui::RawInput::default(), |_| {});
        }
        assert!(!ctx.has_requested_repaint());

        tab.view.lock().unwrap().set_status("Live · local");
        consume(&tab.view, vec![], false, &mut None);
        assert!(!ctx.has_requested_repaint());

        consume(&tab.view, vec!["new log line".into()], false, &mut None);
        assert!(ctx.has_requested_repaint());
        assert_eq!(
            tab.view.lock().unwrap().visible_line(0).text,
            "new log line"
        );
    }

    #[test]
    fn source_state_survives_reassignment_and_starts_stopped() {
        let ctx = egui::Context::default();
        let settings = Settings::default();
        let mut local = App::tab(
            &ctx,
            Source {
                kind: SourceKind::LocalFile,
                path: "C:\\Logs\\app.log".into(),
                ..Default::default()
            },
            &settings.rules,
        );
        let remote = App::tab(
            &ctx,
            Source {
                path: "/var/log/app.log".into(),
                profile: "server-id".into(),
                ..Default::default()
            },
            &settings.rules,
        );
        assert!(local.cancel.is_none() && remote.cancel.is_none());
        assert_eq!(local.view.lock().unwrap().status, "Stopped");
        local.follow = false;
        local.scroll = egui::vec2(12., 50.);
        local.selected = Some(1);
        {
            let mut view = local.view.lock().unwrap();
            view.query = "ERROR".into();
            view.refresh_filter();
        }
        for tab in [&local, &remote] {
            crate::network::consume(
                &tab.view,
                vec!["INFO entry".into(), "ERROR entry".into()],
                false,
                &mut None,
            );
        }
        assert_eq!(local.view.lock().unwrap().visible_len(), 1);
        assert_eq!(remote.view.lock().unwrap().visible_len(), 2);
        let mut workspace = Workspace {
            layout: 4,
            ..Default::default()
        };
        workspace.assign(local.source.id.clone());
        workspace.active_pane = 2;
        workspace.assign(remote.source.id.clone());
        workspace.layout = 1;
        workspace.active_pane = 0;
        workspace.assign(remote.source.id.clone());
        workspace.layout = 4;
        workspace.active_pane = 3;
        workspace.assign(local.source.id.clone());
        assert!(!local.follow && remote.follow);
        assert_eq!(local.scroll.y, 50.);
        assert_eq!(local.selected, Some(1));
        workspace.close(&remote.source.id);
        assert!(
            workspace
                .panes
                .iter()
                .all(|p| p.as_ref() != Some(&remote.source.id))
        );
        assert_eq!(local.view.lock().unwrap().visible_len(), 1);
        let retired = local.view.clone();
        local.retire_reader(true);
        crate::network::consume(
            &retired,
            vec!["ERROR late old-server output".into()],
            false,
            &mut None,
        );
        assert!(local.view.lock().unwrap().visible_len() == 0);
        assert_eq!(local.view.lock().unwrap().query, "ERROR");
    }
}
