// Copyright (C) 2026 Juan Pablo Devoto
// SPDX-License-Identifier: GPL-3.0-only

use crate::model::*;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connecting,
    AwaitingTrust,
    Connected,
    Reconnecting,
    Failed(String),
}
impl ConnectionState {
    pub fn label(&self) -> &str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::Connecting => "Connecting",
            Self::AwaitingTrust => "Awaiting host-key verification",
            Self::Connected => "Connected",
            Self::Reconnecting => "Reconnecting",
            Self::Failed(_) => "Connection failed",
        }
    }
    pub fn pending(&self) -> bool {
        matches!(
            self,
            Self::Connecting | Self::AwaitingTrust | Self::Reconnecting
        )
    }
}
pub enum Event {
    Trust {
        host: String,
        fingerprint: String,
        reply: oneshot::Sender<bool>,
    },
    Files {
        request: u64,
        path: String,
        result: Result<Vec<(String, bool)>, String>,
    },
    Error(String),
}
struct LogFilter {
    include: Option<regex::Regex>,
    exclude: Option<regex::Regex>,
    ids: std::collections::VecDeque<u64>,
}
impl LogFilter {
    fn matches(&self, text: &str) -> bool {
        self.include.as_ref().is_none_or(|re| re.is_match(text))
            && self.exclude.as_ref().is_none_or(|re| !re.is_match(text))
    }
}

pub struct View {
    pub repaint: eframe::egui::Context,
    pub status: String,
    pub diagnostic: String,
    buffer: Buffer,
    pub query: String,
    pub exclude: String,
    pub regex: bool,
    pub error: String,
    pub include_error: Option<String>,
    pub exclude_error: Option<String>,
    checked_filter: (String, String, bool),
    applied_filter: (String, String, bool),
    pub filter_summary: String,
    filter: Option<LogFilter>,
    rules: Vec<(Rule, regex::Regex)>,
    pub rule_revision: u64,
}
impl View {
    pub fn new(repaint: eframe::egui::Context, rules: &[Rule]) -> Self {
        Self {
            repaint,
            status: "Stopped".into(),
            diagnostic: String::new(),
            buffer: Buffer::default(),
            query: String::new(),
            exclude: String::new(),
            regex: false,
            error: String::new(),
            include_error: None,
            exclude_error: None,
            checked_filter: Default::default(),
            applied_filter: Default::default(),
            filter_summary: String::new(),
            filter: None,
            rules: Self::compile_rules(rules),
            rule_revision: 0,
        }
    }

    pub fn retire(&mut self, clear: bool) -> Self {
        let mut next = Self {
            repaint: self.repaint.clone(),
            status: "Stopped".into(),
            diagnostic: String::new(),
            buffer: std::mem::take(&mut self.buffer),
            query: self.query.clone(),
            exclude: self.exclude.clone(),
            regex: self.regex,
            error: self.error.clone(),
            include_error: self.include_error.clone(),
            exclude_error: self.exclude_error.clone(),
            checked_filter: std::mem::take(&mut self.checked_filter),
            applied_filter: std::mem::take(&mut self.applied_filter),
            filter_summary: std::mem::take(&mut self.filter_summary),
            filter: self.filter.take(),
            rules: self.rules.clone(),
            rule_revision: self.rule_revision,
        };
        if clear {
            next.clear_history();
        }
        next.refresh_filter();
        next
    }

    pub fn set_status(&mut self, status: &str) {
        if self.status != status {
            self.status = status.into();
            self.repaint.request_repaint();
        }
    }

    pub fn set_diagnostic(&mut self, diagnostic: String) {
        if self.diagnostic != diagnostic {
            self.diagnostic = diagnostic;
            self.repaint.request_repaint();
        }
    }

    pub fn refresh_filter(&mut self) {
        let (query, exclude, regex) = &self.checked_filter;
        if *query == self.query && *exclude == self.exclude && *regex == self.regex {
            return;
        }
        self.checked_filter = (self.query.clone(), self.exclude.clone(), self.regex);
        let compile_filter = |pattern: &str| {
            (!pattern.is_empty())
                .then(|| compile(pattern, self.regex, false))
                .transpose()
        };
        let include = compile_filter(&self.query);
        let exclude = compile_filter(&self.exclude);
        self.include_error = include.as_ref().err().map(ToString::to_string);
        self.exclude_error = exclude.as_ref().err().map(ToString::to_string);
        match (include, exclude) {
            (Ok(None), Ok(None)) => {
                self.error.clear();
                self.filter = None;
            }
            (Ok(include), Ok(exclude)) => {
                self.error.clear();
                let mut filter = LogFilter {
                    include,
                    exclude,
                    ids: Default::default(),
                };
                filter.ids = self
                    .buffer
                    .lines
                    .iter()
                    .filter(|line| filter.matches(&line.text))
                    .map(|line| line.id)
                    .collect();
                self.filter = Some(filter);
            }
            // Keep applying the last valid filter while the user fixes the pattern.
            _ => {
                self.error = [
                    self.include_error.as_ref().map(|e| format!("Include: {e}")),
                    self.exclude_error.as_ref().map(|e| format!("Exclude: {e}")),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("\n");
            }
        }
        if self.error.is_empty() {
            self.applied_filter = self.checked_filter.clone();
            self.filter_summary = filter_summary(&self.applied_filter);
        }
        self.repaint.request_repaint();
    }

    pub fn filter_pending(&self) -> bool {
        self.checked_filter.0 != self.query
            || self.checked_filter.1 != self.exclude
            || self.checked_filter.2 != self.regex
    }

    pub fn filtering(&self) -> bool {
        self.filter.is_some()
    }

    pub fn clear_filters(&mut self) {
        self.query.clear();
        self.exclude.clear();
        self.regex = false;
        self.refresh_filter();
    }

    /// Retained IDs are contiguous; checking a selection never scans history.
    pub fn selected_line(&self, id: u64) -> Option<&Line> {
        let first = self.buffer.lines.front()?.id;
        let offset = usize::try_from(id.checked_sub(first)?).ok()?;
        let line = self.buffer.lines.get(offset)?;
        self.filter
            .as_ref()
            .is_none_or(|f| f.ids.binary_search(&id).is_ok())
            .then_some(line)
    }

    pub fn push(&mut self, text: String) {
        self.buffer.push(text, &self.rules);
        if let Some(filter) = &mut self.filter {
            let Some(first) = self.buffer.lines.front() else {
                filter.ids.clear();
                return;
            };
            while filter.ids.front().is_some_and(|&id| id < first.id) {
                filter.ids.pop_front();
            }
            let line = self.buffer.lines.back().unwrap();
            if filter.matches(&line.text) {
                filter.ids.push_back(line.id);
            }
        }
    }

    pub fn clear_history(&mut self) {
        self.buffer = Buffer::default();
        if let Some(filter) = &mut self.filter {
            filter.ids.clear();
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn visible_len(&self) -> usize {
        self.filter
            .as_ref()
            .map_or(self.buffer.lines.len(), |filter| filter.ids.len())
    }

    pub fn visible_line(&self, index: usize) -> &Line {
        // Retained IDs are contiguous because the buffer only evicts from the front.
        let offset = match &self.filter {
            Some(filter) => (filter.ids[index] - self.buffer.lines[0].id) as usize,
            None => index,
        };
        &self.buffer.lines[offset]
    }

    pub fn visible_lines(&self) -> impl Iterator<Item = &Line> {
        (0..self.visible_len()).map(|index| self.visible_line(index))
    }

    /// An owned, point-in-time snapshot; no trailing separator, including empty rows.
    pub fn export_text(&self) -> String {
        let bytes: usize = self.visible_lines().map(|line| line.text.len()).sum();
        let mut text = String::with_capacity(bytes + self.visible_len().saturating_sub(1));
        for (index, line) in self.visible_lines().enumerate() {
            if index > 0 {
                text.push('\n');
            }
            text.push_str(&line.text);
        }
        text
    }

    fn compile_rules(rules: &[Rule]) -> Vec<(Rule, regex::Regex)> {
        rules
            .iter()
            .filter(|rule| rule.enabled)
            .filter_map(|rule| {
                compile(&rule.pattern, rule.regex, rule.case_sensitive)
                    .ok()
                    .map(|re| (rule.clone(), re))
            })
            .collect()
    }

    pub fn apply_rules(&mut self, rules: &[Rule]) {
        self.rules = Self::compile_rules(rules);
        for line in &mut self.buffer.lines {
            line.apply_rules(&self.rules);
        }
        self.repaint.request_repaint();
    }

    pub fn apply_rules_at(&mut self, rules: &[Rule], revision: u64) {
        if revision > self.rule_revision {
            self.apply_rules(rules);
            self.rule_revision = revision;
        }
    }
}

fn filter_summary((include, exclude, regex): &(String, String, bool)) -> String {
    let mut parts = Vec::with_capacity(2);
    if !include.is_empty() {
        parts.push(format!("Include: {include}"));
    }
    if !exclude.is_empty() {
        parts.push(format!("Exclude: {exclude}"));
    }
    let summary = parts.join(" · ");
    if *regex && !summary.is_empty() {
        format!("Regex · {summary}")
    } else {
        summary
    }
}
pub enum Request {
    Start {
        source: Source,
        view: Arc<Mutex<View>>,
        cancel: oneshot::Receiver<()>,
    },
    Browse {
        request: u64,
        path: String,
        sudo: bool,
    },
    CancelBrowse,
    Stop,
}
pub struct Session {
    pub generation: String,
    pub state: tokio::sync::watch::Receiver<ConnectionState>,
    abort: tokio::task::AbortHandle,
    _lifetime: tokio::sync::watch::Sender<()>,
    pub tx: mpsc::Sender<Request>,
    pub events: mpsc::Receiver<Event>,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.abort.abort();
    }
}
struct LoginAttempt {
    updates: SessionUpdates,
    completed: bool,
}
impl Drop for LoginAttempt {
    fn drop(&mut self) {
        if !self.completed {
            self.updates.set_state(ConnectionState::Failed(
                "Connection attempt interrupted; retry to reconnect".into(),
            ));
        }
    }
}
#[derive(Clone)]
struct SessionUpdates {
    state: Option<tokio::sync::watch::Sender<ConnectionState>>,
    events: mpsc::Sender<Event>,
    repaint: eframe::egui::Context,
}
impl SessionUpdates {
    fn set_state(&self, state: ConnectionState) {
        if let Some(tx) = &self.state {
            tx.send_replace(state);
            self.repaint.request_repaint();
        }
    }

    async fn send(&self, event: Event) {
        if self.events.send(event).await.is_ok() {
            self.repaint.request_repaint();
        }
    }
}

struct Handler {
    attempt: tokio::sync::watch::Receiver<()>,
    lifetime: Option<tokio::sync::watch::Receiver<()>>,
    updates: SessionUpdates,
    host: String,
    trusted: Arc<Mutex<Option<String>>>,
}
impl russh::client::Handler for Handler {
    type Error = russh::Error;
    async fn check_server_key(
        &mut self,
        key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let fingerprint = key
            .public_key()
            .fingerprint(russh::keys::ssh_key::HashAlg::Sha256)
            .to_string();
        if let Some(old) = &*self.trusted.lock().unwrap() {
            if old != &fingerprint {
                let event = Event::Error(format!(
                    "Host key changed for {}. Connection blocked. Verify the new fingerprint independently, then remove the old trust entry in settings.",
                    self.host
                ));
                if self.updates.events.try_send(event).is_ok() {
                    self.updates.repaint.request_repaint();
                }
            }
            return Ok(old == &fingerprint);
        }
        self.updates.set_state(ConnectionState::AwaitingTrust);
        let (tx, rx) = oneshot::channel();
        self.updates
            .send(Event::Trust {
                host: self.host.clone(),
                fingerprint: fingerprint.clone(),
                reply: tx,
            })
            .await;
        let accepted = tokio::select! {
            _ = self.attempt.changed() => false,
            accepted = rx => accepted.unwrap_or(false),
            _ = async {
                if let Some(lifetime) = &mut self.lifetime { let _ = lifetime.changed().await; }
                else { std::future::pending::<()>().await; }
            } => false,
        };
        if accepted {
            *self.trusted.lock().unwrap() = Some(fingerprint);
        }
        Ok(accepted)
    }
}

type Client = russh::client::Handle<Handler>;
#[cfg(test)]
#[path = "network_live_tests.rs"]
mod live_tests;
struct Shared {
    lifetime: Option<tokio::sync::watch::Receiver<()>>,
    client: tokio::sync::Mutex<Client>,
    profile: Profile,
    secret: String,
    trust: Arc<Mutex<Option<String>>>,
    updates: SessionUpdates,
}
#[cfg(test)]
async fn login(
    p: &Profile,
    secret: &str,
    trust: Arc<Mutex<Option<String>>>,
    events: mpsc::Sender<Event>,
) -> anyhow::Result<Client> {
    let updates = SessionUpdates {
        state: None,
        events,
        repaint: Default::default(),
    };
    login_tracked(p, secret, trust, updates, None).await
}
async fn login_tracked(
    p: &Profile,
    secret: &str,
    trust: Arc<Mutex<Option<String>>>,
    updates: SessionUpdates,
    lifetime: Option<tokio::sync::watch::Receiver<()>>,
) -> anyhow::Result<Client> {
    let mut guard = LoginAttempt {
        updates: updates.clone(),
        completed: false,
    };
    let (_attempt_tx, attempt) = tokio::sync::watch::channel(());
    let config = russh::client::Config {
        keepalive_interval: Some(std::time::Duration::from_secs(20)),
        keepalive_max: 3,
        ..Default::default()
    };
    let mut client = russh::client::connect(
        Arc::new(config),
        (p.host.as_str(), p.port),
        Handler {
            attempt,
            lifetime,
            updates,
            host: format!("{}:{}", p.host, p.port),
            trusted: trust,
        },
    )
    .await?;
    let auth = if p.key.is_empty() {
        client.authenticate_password(&p.user, secret).await?
    } else {
        let key = russh::keys::load_secret_key(
            &p.key,
            if secret.is_empty() {
                None
            } else {
                Some(secret)
            },
        )?;
        let hash = client.best_supported_rsa_hash().await?.flatten();
        client
            .authenticate_publickey(
                &p.user,
                russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), hash),
            )
            .await?
    };
    anyhow::ensure!(
        auth.success(),
        "Authentication failed; reconnect after correcting credentials"
    );
    guard.completed = true;
    Ok(client)
}
async fn channel(shared: &Shared) -> anyhow::Result<russh::Channel<russh::client::Msg>> {
    let mut client = shared.client.lock().await;
    if client.is_closed() {
        shared.updates.set_state(ConnectionState::Reconnecting);
        match login_tracked(
            &shared.profile,
            &shared.secret,
            shared.trust.clone(),
            shared.updates.clone(),
            shared.lifetime.clone(),
        )
        .await
        {
            Ok(next) => {
                *client = next;
                shared.updates.set_state(ConnectionState::Connected);
            }
            Err(error) => {
                shared
                    .updates
                    .set_state(ConnectionState::Failed(error.to_string()));
                return Err(error);
            }
        }
    }
    Ok(client.channel_open_session().await?)
}
pub(crate) fn consume(
    view: &Arc<Mutex<View>>,
    lines: Vec<String>,
    journal: bool,
    cursor: &mut Option<String>,
) {
    if lines.is_empty() {
        return;
    }
    // Parse outside the shared lock; bound each ingestion lock to 128 rows.
    let mut batch = Vec::with_capacity(128);
    for line in lines {
        let text = if journal {
            match serde_json::from_str::<serde_json::Value>(&line) {
                Ok(j) => {
                    *cursor = j["__CURSOR"].as_str().map(str::to_owned);
                    let message = j["MESSAGE"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| j["MESSAGE"].to_string());
                    format!(
                        "{} [{}] {}",
                        j["__REALTIME_TIMESTAMP"].as_str().unwrap_or(""),
                        j["PRIORITY"].as_str().unwrap_or("?"),
                        message.replace('\n', " ↵ ")
                    )
                }
                Err(_) => line,
            }
        } else {
            line
        };
        batch.push(text);
        if batch.len() == 128 {
            let mut v = view.lock().unwrap();
            for text in batch.drain(..) {
                v.push(text);
            }
        }
    }
    let mut v = view.lock().unwrap();
    for text in batch {
        v.push(text);
    }
    crate::rendering::request_log_repaint(&v.repaint);
}
async fn stream(
    shared: Arc<Shared>,
    source: Source,
    view: Arc<Mutex<View>>,
    mut cancel: oneshot::Receiver<()>,
) {
    view.lock().unwrap().set_status("Connecting stream");
    let mut cursor = None;
    let mut delay = 1;
    let mut reconnect = false;
    loop {
        let opened = tokio::select! {_=&mut cancel=>return,r=channel(&shared)=>r};
        let mut ch = match opened {
            Ok(ch) => ch,
            Err(e) => {
                let message = e.to_string();
                let permanent = message.to_lowercase().contains("key")
                    || message.contains("Authentication")
                    || message.contains("decrypt");
                {
                    let mut v = view.lock().unwrap();
                    v.set_diagnostic(message);
                    if permanent {
                        v.set_status("Error · reconnect stopped");
                        return;
                    }
                    v.set_status("Reconnecting");
                }
                tokio::select! {_=&mut cancel=>return,_=tokio::time::sleep(std::time::Duration::from_secs(delay))=>{}}
                delay = (delay * 2).min(30);
                continue;
            }
        };
        let cmd = match command(&source, cursor.as_deref()) {
            Ok(c) => c,
            Err(e) => {
                let mut v = view.lock().unwrap();
                v.set_diagnostic(e.to_string());
                v.set_status("Error");
                return;
            }
        };
        if let Err(e) = ch.exec(true, cmd).await {
            let mut v = view.lock().unwrap();
            v.set_diagnostic(e.to_string());
            v.set_status("Error");
            return;
        }
        view.lock().unwrap().set_status("Live · SSH");
        if reconnect {
            consume(
                &view,
                vec![if source.is_journal() {
                    "[Reconnected to journal]".into()
                } else {
                    "[Reconnected: recent 200 lines reloaded; gaps or duplicates may exist]".into()
                }],
                false,
                &mut None,
            );
        }
        let mut decoder = Decoder::default();
        let mut pending = std::collections::VecDeque::new();
        let mut bytes = 0usize;
        let mut dropped = 0usize;
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(100));
        let mut exited = false;
        loop {
            tokio::select! {
                _=&mut cancel=>{let _=ch.close().await;view.lock().unwrap().set_status("Stopped");return},
                _=tick.tick()=>{if !pending.is_empty(){let mut lines:Vec<String>=pending.drain(..).collect();bytes=0;if dropped>0{lines.insert(0,format!("[Dropped {dropped} queued lines under load]"));dropped=0;}consume(&view,lines,source.is_journal(),&mut cursor);}},
                msg=ch.wait()=>match msg {
                    Some(russh::ChannelMsg::Data{data})=>{delay=1;for line in decoder.feed(&data){bytes+=line.len();pending.push_back(line);while bytes>4*1024*1024||pending.len()>4000{if let Some(l)=pending.pop_front(){bytes-=l.len();dropped+=1;}}}},
                    Some(russh::ChannelMsg::ExtendedData{data,..})=>{view.lock().unwrap().set_diagnostic(String::from_utf8_lossy(&data).into());},
                    Some(russh::ChannelMsg::ExitStatus{exit_status})=>{view.lock().unwrap().set_status(&format!("Exited ({exit_status})"));exited=true;break},
                    None=>break,
                    _=>{}
                }
            }
        }
        let last = decoder.take();
        if !last.is_empty() {
            pending.push_back(last)
        }
        consume(
            &view,
            pending.into_iter().collect(),
            source.is_journal(),
            &mut cursor,
        );
        if exited {
            return;
        }
        consume(
            &view,
            vec!["[Connection interrupted; retrying]".into()],
            false,
            &mut None,
        );
        view.lock().unwrap().set_status("Reconnecting");
        reconnect = true;
        tokio::select! {_=&mut cancel=>return,_=tokio::time::sleep(std::time::Duration::from_secs(delay))=>{}}
        delay = (delay * 2).min(30);
    }
}
pub fn connect(
    rt: &tokio::runtime::Runtime,
    p: Profile,
    secret: String,
    trusted: Option<String>,
    repaint: eframe::egui::Context,
) -> Session {
    let (tx, mut rx) = mpsc::channel(32);
    let (etx, events) = mpsc::channel(128);
    let (state_tx, state) = tokio::sync::watch::channel(ConnectionState::Connecting);
    let (lifetime_tx, lifetime) = tokio::sync::watch::channel(());
    let updates = SessionUpdates {
        state: Some(state_tx.clone()),
        events: etx,
        repaint,
    };
    let task = rt.spawn(async move {
        let trust = Arc::new(Mutex::new(trusted));
        let client = match login_tracked(&p, &secret, trust.clone(), updates.clone(), Some(lifetime.clone())).await {
            Ok(client) => client,
            Err(error) => { updates.set_state(ConnectionState::Failed(error.to_string())); return; }
        };
        let shared = Arc::new(Shared {
            lifetime: Some(lifetime),
            client: tokio::sync::Mutex::new(client), profile: p, secret, trust,
            updates: updates.clone(),
        });
        updates.set_state(ConnectionState::Connected);
        let mut workers = tokio::task::JoinSet::new();
        let mut browser: Option<tokio::task::AbortHandle> = None;
        let mut monitor = tokio::time::interval(std::time::Duration::from_millis(250));
        loop {
            tokio::select! {
                _ = monitor.tick() => {
                    if let Ok(client) = shared.client.try_lock()
                        && client.is_closed() && *state_tx.borrow() == ConnectionState::Connected {
                            updates.set_state(ConnectionState::Failed("SSH transport closed; reconnect to resume".into()));
                        }
                }
                request = rx.recv() => match request {
                    None | Some(Request::Stop) => break,
                    Some(Request::Start { source, view, cancel }) => {
                        workers.spawn(stream(shared.clone(), source, view, cancel));
                    }
                    Some(Request::CancelBrowse) => { if let Some(task) = browser.take() { task.abort(); } }
                    Some(Request::Browse { request, path, sudo }) => {
                        if let Some(task) = browser.take() { task.abort(); }
                        let shared = shared.clone();
                        browser = Some(workers.spawn(async move {
                            let result = browse_directory(&shared, &path, sudo).await;
                            shared.updates.send(Event::Files { request, path, result: result.map_err(|e| e.to_string()) }).await;
                        }));
                    }
                }
            }
            while workers.try_join_next().is_some() {}
        }
        workers.abort_all();
        updates.set_state(ConnectionState::Disconnected);
        let _ = shared.client.lock().await.disconnect(russh::Disconnect::ByApplication, "Closing", "en").await;
    });
    Session {
        tx,
        events,
        state,
        generation: new_id(),
        abort: task.abort_handle(),
        _lifetime: lifetime_tx,
    }
}

#[cfg(test)]
pub(crate) fn simulated_session(rt: &tokio::runtime::Runtime) -> Session {
    let (tx, mut rx) = mpsc::channel(32);
    let (etx, events) = mpsc::channel(128);
    let (state_tx, state) = tokio::sync::watch::channel(ConnectionState::Connected);
    let (lifetime_tx, _) = tokio::sync::watch::channel(());
    let task = rt.spawn(async move {
        let _keep = (etx, state_tx);
        while rx.recv().await.is_some() {}
    });
    Session {
        tx,
        events,
        state,
        generation: new_id(),
        abort: task.abort_handle(),
        _lifetime: lifetime_tx,
    }
}

#[cfg(test)]
pub(crate) fn controlled_session(
    rt: &tokio::runtime::Runtime,
    initial: ConnectionState,
) -> (
    Session,
    tokio::sync::watch::Sender<ConnectionState>,
    mpsc::Receiver<Request>,
) {
    let (tx, requests) = mpsc::channel(32);
    let (etx, events) = mpsc::channel(128);
    let (updates, state) = tokio::sync::watch::channel(initial);
    let (lifetime, _) = tokio::sync::watch::channel(());
    let task = rt.spawn(async move {
        let _keep = etx;
        std::future::pending::<()>().await;
    });
    (
        Session {
            tx,
            events,
            state,
            generation: new_id(),
            abort: task.abort_handle(),
            _lifetime: lifetime,
        },
        updates,
        requests,
    )
}

// Only fixed, known server locations are executed; directory names stay in SFTP packets.
const SUDO_SFTP: &str = "for p in /usr/lib/openssh/sftp-server /usr/libexec/openssh/sftp-server /usr/lib/ssh/sftp-server /usr/libexec/sftp-server; do if test -x \"$p\"; then exec sudo -n -- \"$p\" -R; fi; done; echo 'No supported external OpenSSH sftp-server found' >&2; exit 127";

struct BrowsePump(Option<oneshot::Sender<()>>);
impl Drop for BrowsePump {
    fn drop(&mut self) {
        if let Some(cancel) = self.0.take() {
            let _ = cancel.send(());
        }
    }
}
async fn browse_directory(
    shared: &Arc<Shared>,
    path: &str,
    sudo: bool,
) -> anyhow::Result<Vec<(String, bool)>> {
    let ch = channel(shared).await?;
    if sudo {
        ch.exec(true, SUDO_SFTP).await?;
    } else {
        ch.request_subsystem(true, "sftp").await?;
    }
    read_directory(ch, path, sudo).await
}
async fn read_directory(
    mut ch: russh::Channel<russh::client::Msg>,
    path: &str,
    sudo: bool,
) -> anyhow::Result<Vec<(String, bool)>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let stderr = Arc::new(Mutex::new(String::new()));
    let errors = stderr.clone();
    let (client, mut bridge) = tokio::io::duplex(65536);
    let (cancel, mut cancelled) = oneshot::channel();
    let _pump = BrowsePump(Some(cancel));
    tokio::spawn(async move {
        let mut buffer = [0u8; 16384];
        loop {
            tokio::select! {
                _ = &mut cancelled => break,
                read = bridge.read(&mut buffer) => match read {
                    Ok(n) if n > 0 => { if ch.data(&buffer[..n]).await.is_err() { break; } }
                    _ => break,
                },
                message = ch.wait() => match message {
                    Some(russh::ChannelMsg::Data { data }) => { if bridge.write_all(&data).await.is_err() { break; } }
                    Some(russh::ChannelMsg::ExtendedData { data, .. }) => {
                        let mut error = errors.lock().unwrap();
                        let remaining = 8192usize.saturating_sub(error.len());
                        error.push_str(&String::from_utf8_lossy(&data[..data.len().min(remaining)]));
                    }
                    Some(russh::ChannelMsg::Failure) => { *errors.lock().unwrap() = "SSH server rejected the browsing request".into(); break; }
                    None | Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) => break,
                    _ => {}
                }
            }
        }
        let _ = ch.close().await;
    });
    let result = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        let sftp = russh_sftp::client::SftpSession::new(client).await?;
        let entries = sftp
            .read_dir(path)
            .await?
            .map(|e| (e.file_name(), e.file_type().is_dir()))
            .collect();
        Ok::<_, anyhow::Error>(entries)
    })
    .await
    .unwrap_or_else(|_| Err(anyhow::anyhow!("Directory request timed out")));
    result.map_err(|error| {
        let detail = stderr.lock().unwrap().trim().to_owned();
        if sudo { anyhow::anyhow!("Sudo browsing failed: {}. Requires existing passwordless permission for the external sftp-server -R command.", if detail.is_empty() { error.to_string() } else { detail }) }
        else { anyhow::anyhow!("{}", if detail.is_empty() { error.to_string() } else { detail }) }
    })
}

#[cfg(test)]
mod view_tests {
    use super::*;

    #[test]
    fn export_snapshot_preserves_empty_rows_filters_and_rule_updates() {
        let mut view = View::new(Default::default(), &Settings::default().rules);
        assert_eq!(view.export_text(), "");
        for text in ["", "INFO é", "ERROR 😀", ""] {
            view.push(text.into());
        }
        assert_eq!(view.export_text(), "\nINFO é\nERROR 😀\n");
        view.query = "info|error".into();
        view.regex = true;
        view.refresh_filter();
        let snapshot = view.export_text();
        assert_eq!(snapshot, "INFO é\nERROR 😀");
        view.query = "[".into();
        view.refresh_filter();
        assert_eq!(view.export_text(), snapshot);
        let mut rules = Settings::default().rules;
        rules[0].pattern = "INFO".into();
        view.apply_rules(&rules);
        assert_eq!(view.visible_line(0).color, Some(rules[0].color));
        assert_eq!(view.export_text(), snapshot);
        view.clear_history();
        view.push("INFO later".into());
        assert_eq!(snapshot, "INFO é\nERROR 😀");
        assert_eq!(view.export_text(), "INFO later");
    }

    #[test]
    fn chunked_ingestion_preserves_order_cursor_and_filter() {
        let mut view = View::new(Default::default(), &Settings::default().rules);
        view.query = "ERROR".into();
        view.refresh_filter();
        let view = Arc::new(Mutex::new(view));
        let mut cursor = None;
        consume(
            &view,
            (0..300)
                .map(|i| format!(r#"{{"__CURSOR":"{i}","MESSAGE":"ERROR row {i}"}}"#))
                .collect(),
            true,
            &mut cursor,
        );
        let view = view.lock().unwrap();
        assert_eq!(cursor.as_deref(), Some("299"));
        assert_eq!(view.visible_len(), 300);
        for i in 0..300 {
            assert!(
                view.visible_line(i)
                    .text
                    .ends_with(&format!("ERROR row {i}"))
            );
            assert!(view.visible_line(i).color.is_some());
        }
    }

    #[test]
    fn filtered_view_tracks_appends_and_both_history_limits() {
        let mut view = View::new(Default::default(), &[]);
        view.push("ERROR old match".into());
        for _ in 0..19_998 {
            view.push("INFO background".into());
        }
        view.push("ERROR skip".into());
        view.query = "^error".into();
        view.exclude = "skip".into();
        view.regex = true;
        view.refresh_filter();
        assert_eq!(view.visible_len(), 1);
        assert_eq!(view.visible_line(0).id, 1);

        view.push("ERROR fresh match".into());
        view.push("INFO fresh nonmatch".into());
        assert_eq!(view.buffer().lines.len(), 20_000);
        assert_eq!(view.visible_len(), 1);
        assert_eq!(view.visible_line(0).id, 20_001);

        let chunk = "x".repeat(64 * 1024);
        for _ in 0..256 {
            view.push(chunk.clone());
        }
        assert_eq!(view.buffer().lines.len(), 256);
        assert_eq!(view.buffer().bytes, 16 * 1024 * 1024);
        assert_eq!(view.visible_len(), 0);
        view.push("ERROR after byte eviction".into());
        assert_eq!(view.visible_line(0).text, "ERROR after byte eviction");
    }

    #[test]
    fn filter_edits_clear_history_and_keep_one_copy_of_each_line() {
        let mut view = View::new(Default::default(), &[]);
        view.push("INFO first".into());
        view.push("ERROR second".into());
        assert!(std::ptr::eq(view.visible_line(0), &view.buffer().lines[0]));

        view.regex = true;
        view.query = "ERROR".into();
        view.refresh_filter();
        assert!(std::ptr::eq(view.visible_line(0), &view.buffer().lines[1]));

        view.query = "[".into();
        view.refresh_filter();
        assert!(!view.error.is_empty());
        view.push("ERROR third".into());
        assert_eq!(view.visible_len(), 2);

        // A source restart can happen before the queued filter job runs.
        view.query = "INFO".into();
        let mut view = view.retire(false);
        assert!(view.error.is_empty());
        assert_eq!(view.visible_len(), 1);
        assert_eq!(view.visible_line(0).text, "INFO first");
        view.clear_history();
        view.push("ERROR hidden".into());
        view.push("INFO after clear".into());
        assert_eq!(view.visible_len(), 1);
        assert_eq!(view.visible_line(0).text, "INFO after clear");

        view.query.clear();
        view.refresh_filter();
        assert_eq!(view.visible_len(), 2);
    }
}

#[cfg(test)]
mod sustained {
    use super::*;
    #[test]
    #[ignore = "ten minute processing stress test"]
    fn four_streams_ten_minutes() {
        let views: Vec<_> = (0..4)
            .map(|_| {
                let mut view = View::new(Default::default(), &Settings::default().rules);
                view.query = "INFO".into();
                view.refresh_filter();
                Arc::new(Mutex::new(view))
            })
            .collect();
        let start = std::time::Instant::now();
        let mut max_batch = std::time::Duration::ZERO;
        for tick in 0..6000 {
            let begin = std::time::Instant::now();
            for (stream, v) in views.iter().enumerate() {
                consume(
                    v,
                    (0..125)
                        .map(|n| {
                            format!(
                                "INFO stream={stream} sequence={} payload={} ",
                                tick * 125 + n,
                                "x".repeat(180)
                            )
                        })
                        .collect(),
                    false,
                    &mut None,
                );
            }
            max_batch = max_batch.max(begin.elapsed());
            let target = std::time::Duration::from_millis((tick + 1) * 100);
            if let Some(wait) = target.checked_sub(start.elapsed()) {
                std::thread::sleep(wait)
            }
        }
        for v in views {
            let v = v.lock().unwrap();
            assert_eq!(v.buffer.lines.len(), 20000);
            assert!(v.buffer.bytes <= 16 * 1024 * 1024);
            assert_eq!(v.visible_len(), 20000);
        }
        println!(
            "3,000,000 lines; elapsed={:?}; worst 500-line processing batch={max_batch:?}",
            start.elapsed()
        );
        assert!(start.elapsed().as_secs() < 660);
    }
}
