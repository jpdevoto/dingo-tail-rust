// Copyright (C) 2026 Juan Pablo Devoto
// SPDX-License-Identifier: GPL-3.0-only

pub const DEFAULT_INITIAL_LINES: usize = 200;
pub const MAX_INITIAL_LINES: usize = 20_000;
pub fn normalize_initial_lines(value: usize) -> usize {
    value.clamp(1, MAX_INITIAL_LINES)
}

pub const LEGACY_CREDENTIAL_SERVICE: &str = "SSHLogTail";
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, path::PathBuf};
pub fn new_id() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!(
        "{:x}-{:x}-{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Profile {
    #[serde(default = "new_id")]
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub key: String,
    pub remember: bool,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: "New server".into(),
            host: String::new(),
            port: 22,
            user: String::new(),
            key: String::new(),
            remember: false,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Source {
    #[serde(default = "new_id")]
    pub id: String,
    pub profile: String,
    pub path: String,
    pub kind: SourceKind,
    pub sudo: bool,
}
impl Default for Source {
    fn default() -> Self {
        Self {
            id: new_id(),
            profile: String::new(),
            path: "/var/log/syslog".into(),
            kind: SourceKind::SshFile,
            sudo: false,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum SourceKind {
    LocalFile,
    SshFile,
    Journal,
}
impl Source {
    pub fn is_journal(&self) -> bool {
        self.kind == SourceKind::Journal
    }
    pub fn group(&self) -> &str {
        if self.kind == SourceKind::LocalFile {
            "local"
        } else {
            &self.profile
        }
    }
    pub fn label(&self) -> String {
        if self.kind == SourceKind::Journal {
            return if self.path.is_empty() {
                "System journal".into()
            } else {
                self.path.clone()
            };
        }
        self.path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&self.path)
            .to_string()
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Workspace {
    pub layout: usize,
    pub panes: [Option<String>; 4],
    pub active_pane: usize,
    pub groups: std::collections::BTreeMap<String, bool>,
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            layout: 1,
            panes: Default::default(),
            active_pane: 0,
            groups: Default::default(),
        }
    }
}
impl Workspace {
    pub fn assign(&mut self, id: String) {
        if let Some(p) = self.panes[..self.layout]
            .iter()
            .position(|p| p.as_ref() == Some(&id))
        {
            self.active_pane = p;
            return;
        }
        for p in &mut self.panes {
            if p.as_ref() == Some(&id) {
                *p = None
            }
        }
        self.panes[self.active_pane.min(self.layout - 1)] = Some(id);
    }
    pub fn close(&mut self, id: &str) {
        for p in &mut self.panes {
            if p.as_deref() == Some(id) {
                *p = None
            }
        }
    }
    pub fn validate(&mut self, sources: &[Source]) {
        if ![1, 2, 4].contains(&self.layout) {
            self.layout = 1
        }
        self.active_pane = self.active_pane.min(self.layout - 1);
        let mut seen = std::collections::HashSet::new();
        for p in &mut self.panes {
            if let Some(id) = p.as_ref()
                && (!sources.iter().any(|s| &s.id == id) || !seen.insert(id.clone()))
            {
                *p = None
            }
        }
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    pub pattern: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub enabled: bool,
    pub color: [u8; 3],
    pub background: [u8; 3],
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub initial_lines: usize,
    pub renderer: crate::rendering::RendererMode,
    pub version: u32,
    pub workspace: Workspace,
    pub profiles: Vec<Profile>,
    pub sources: Vec<Source>,
    pub rules: Vec<Rule>,
    pub trusted: std::collections::BTreeMap<String, String>,
    pub background: [u8; 3],
    pub text: [u8; 3],
    pub accent: [u8; 3],
    pub font: f32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            initial_lines: DEFAULT_INITIAL_LINES,
            renderer: Default::default(),
            version: 2,
            workspace: Workspace::default(),
            profiles: vec![],
            sources: vec![],
            trusted: Default::default(),
            background: [18, 21, 27],
            text: [216, 223, 233],
            accent: [255, 164, 54],
            font: 14.,
            rules: [
                ("ERROR", [255, 130, 130]),
                ("WARN", [240, 196, 111]),
                ("INFO", [137, 192, 244]),
                ("DEBUG", [162, 173, 192]),
            ]
            .into_iter()
            .map(|(s, c)| Rule {
                name: s.into(),
                pattern: s.into(),
                regex: false,
                case_sensitive: false,
                enabled: true,
                color: c,
                background: [18, 21, 27],
            })
            .collect(),
        }
    }
}
pub fn settings_path() -> anyhow::Result<PathBuf> {
    Ok(crate::platform::config_dir()?.join("settings.json"))
}
impl Settings {
    pub fn load() -> anyhow::Result<Self> {
        let new = settings_path()?;
        if !cfg!(windows) || std::env::var_os("DINGOTAIL_CONFIG_DIR").is_some() {
            return Self::load_from(&new);
        }
        let legacy = new
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("SSHLogTail/settings.json");
        Self::load_with_legacy(&new, &legacy)
    }
    fn load_with_legacy(new: &std::path::Path, legacy: &std::path::Path) -> anyhow::Result<Self> {
        if new.exists() || !legacy.exists() {
            return Self::load_from(new);
        }
        let bytes = std::fs::read(legacy)?;
        let (mut settings, _) = Self::parse(&bytes)?;
        if settings.accent == [102, 165, 255] {
            settings.accent = [255, 164, 54];
        }
        std::fs::create_dir_all(new.parent().unwrap())?;
        let backup = new.with_extension("sshlogtail-import.bak");
        if !backup.exists() {
            std::fs::write(backup, &bytes)?;
        }
        settings.save_to(new)?;
        Ok(settings)
    }
    fn load_from(p: &std::path::Path) -> anyhow::Result<Self> {
        if !p.exists() {
            return Ok(Self::default());
        }
        let bytes = std::fs::read(p)?;
        let (settings, migrated) = Self::parse(&bytes)?;
        if migrated {
            let backup = p.with_extension("v1.bak");
            if !backup.exists() {
                std::fs::write(backup, &bytes)?;
            }
        }
        Ok(settings)
    }
    pub fn parse(bytes: &[u8]) -> anyhow::Result<(Self, bool)> {
        let mut v: serde_json::Value = serde_json::from_slice(bytes)?;
        let version = v["version"].as_u64().unwrap_or(1);
        anyhow::ensure!(version <= 2, "Settings were created by a newer application");
        if version == 1 {
            let mut ids = vec![];
            if let Some(ps) = v["profiles"].as_array_mut() {
                for p in ps {
                    let id = new_id();
                    p["id"] = id.clone().into();
                    ids.push(id)
                }
            }
            if let Some(ss) = v["sources"].as_array_mut() {
                for s in ss {
                    let index = s["profile"].as_u64().unwrap_or(u64::MAX) as usize;
                    s["profile"] = ids
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| format!("missing-legacy-{index}"))
                        .into();
                    s["id"] = new_id().into();
                    s["kind"] = if s["journal"].as_bool().unwrap_or(false) {
                        "Journal"
                    } else {
                        "SshFile"
                    }
                    .into();
                    if let Some(o) = s.as_object_mut() {
                        o.remove("journal");
                    }
                }
            }
            v["version"] = 2.into();
        }
        if let Some(value) = v.get("initial_lines").and_then(serde_json::Value::as_i64)
            && value < 1
        {
            v["initial_lines"] = 1.into();
        }
        let mut s: Self = serde_json::from_value(v)?;
        s.initial_lines = normalize_initial_lines(s.initial_lines);
        let mut ids = std::collections::HashSet::new();
        anyhow::ensure!(
            s.profiles
                .iter()
                .all(|p| !p.id.is_empty() && ids.insert(p.id.clone())),
            "Duplicate or empty server IDs in settings"
        );
        ids.clear();
        anyhow::ensure!(
            s.sources
                .iter()
                .all(|p| !p.id.is_empty() && ids.insert(p.id.clone())),
            "Duplicate or empty source IDs in settings"
        );
        s.workspace.validate(&s.sources);
        if version == 1 {
            s.workspace.panes[0] = s.sources.first().map(|s| s.id.clone());
        }
        Ok((s, version == 1))
    }
    pub fn save(&self) -> anyhow::Result<()> {
        self.save_to(&settings_path()?)
    }
    pub(crate) fn save_to(&self, p: &std::path::Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(p.parent().unwrap())?;
        use std::io::Write;
        let tmp = p.with_extension(format!("{}.tmp", new_id()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| -> anyhow::Result<()> {
            let mut file = options.open(&tmp)?;
            file.write_all(&serde_json::to_vec_pretty(self)?)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&tmp, p)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result?;
        Ok(())
    }
}
pub fn quote(s: &str) -> anyhow::Result<String> {
    anyhow::ensure!(!s.contains('\0'), "NUL is not allowed");
    Ok(format!("'{}'", s.replace('\'', "'\"'\"'")))
}
pub fn command(s: &Source, cursor: Option<&str>, initial_lines: usize) -> anyhow::Result<String> {
    let initial_lines = normalize_initial_lines(initial_lines);
    let prefix = if s.sudo { "sudo -n -- " } else { "" };
    anyhow::ensure!(
        s.kind != SourceKind::LocalFile,
        "Local sources do not execute SSH commands"
    );
    if s.is_journal() {
        Ok(format!(
            "{prefix}journalctl --no-pager -f -o json {} {}",
            if let Some(c) = cursor {
                format!("--after-cursor={}", quote(c)?)
            } else {
                format!("-n {initial_lines}")
            },
            if s.path.is_empty() {
                String::new()
            } else {
                format!("--unit={}", quote(&s.path)?)
            }
        ))
    } else {
        anyhow::ensure!(s.path.starts_with('/'), "Use an absolute remote file path");
        Ok(format!(
            "{prefix}tail -n {initial_lines} -F -- {}",
            quote(&s.path)?
        ))
    }
}
pub fn compile(pattern: &str, regex: bool, case: bool) -> Result<regex::Regex, regex::Error> {
    regex::RegexBuilder::new(&if regex {
        pattern.into()
    } else {
        regex::escape(pattern)
    })
    .case_insensitive(!case)
    .size_limit(1024 * 1024)
    .build()
}
#[derive(Clone)]
pub struct Line {
    pub id: u64,
    pub text: String,
    pub color: Option<[u8; 3]>,
    pub background: Option<[u8; 3]>,
}
impl Line {
    pub fn apply_rules(&mut self, rules: &[(Rule, regex::Regex)]) {
        let style = rules
            .iter()
            .find(|(_, re)| re.is_match(&self.text))
            .map(|(rule, _)| rule);
        self.color = style.map(|rule| rule.color);
        self.background = style.map(|rule| rule.background);
    }
}
#[derive(Default)]
pub struct Buffer {
    pub lines: VecDeque<Line>,
    pub bytes: usize,
    next: u64,
}
impl Buffer {
    pub fn push(&mut self, text: String, rules: &[(Rule, regex::Regex)]) {
        self.next += 1;
        self.bytes += text.len();
        let mut line = Line {
            id: self.next,
            text,
            color: None,
            background: None,
        };
        line.apply_rules(rules);
        self.lines.push_back(line);
        while self.lines.len() > 20_000 || self.bytes > 16 * 1024 * 1024 {
            if let Some(l) = self.lines.pop_front() {
                self.bytes -= l.text.len();
            } else {
                break;
            }
        }
    }
}
#[derive(Default)]
pub struct Decoder {
    bytes: Vec<u8>,
    truncated: bool,
}
impl Decoder {
    pub fn feed(&mut self, data: &[u8]) -> Vec<String> {
        let mut out = vec![];
        for segment in data.split_inclusive(|&b| b == b'\n') {
            let complete = segment.last() == Some(&b'\n');
            let text = if complete {
                &segment[..segment.len() - 1]
            } else {
                segment
            };
            let keep = text.len().min(65536 - self.bytes.len());
            self.bytes.extend_from_slice(&text[..keep]);
            self.truncated |= keep < text.len();
            if complete {
                out.push(self.take());
            }
        }
        out
    }
    pub fn take(&mut self) -> String {
        let mut s = String::from_utf8_lossy(&self.bytes)
            .trim_end_matches('\r')
            .to_string();
        self.bytes.clear();
        if self.truncated {
            s.push_str(" [line truncated]");
            self.truncated = false
        }
        s
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initial_lines_defaults_clamps_and_roundtrips() {
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value.as_object_mut().unwrap().remove("initial_lines");
        assert_eq!(
            Settings::parse(&serde_json::to_vec(&value).unwrap())
                .unwrap()
                .0
                .initial_lines,
            200
        );
        for (input, expected) in [
            (-1_i64, 1),
            (0, 1),
            (1, 1),
            (5000, 5000),
            (20_000, 20_000),
            (30_000, 20_000),
        ] {
            value["initial_lines"] = input.into();
            let settings = Settings::parse(&serde_json::to_vec(&value).unwrap())
                .unwrap()
                .0;
            assert_eq!(settings.initial_lines, expected);
            assert_eq!(
                Settings::parse(&serde_json::to_vec(&settings).unwrap())
                    .unwrap()
                    .0
                    .initial_lines,
                expected
            );
            assert_eq!(settings.version, 2);
        }
        let file = Source {
            path: "/var/log/test file".into(),
            ..Default::default()
        };
        assert_eq!(
            command(&file, None, 5000).unwrap(),
            "tail -n 5000 -F -- '/var/log/test file'"
        );
        assert!(
            command(&file, None, usize::MAX)
                .unwrap()
                .contains("-n 20000")
        );
        let journal = Source {
            kind: SourceKind::Journal,
            ..Default::default()
        };
        assert!(command(&journal, None, 5000).unwrap().contains("-n 5000"));
        let resumed = command(&journal, Some("cursor'value"), 5000).unwrap();
        assert!(resumed.contains("--after-cursor="));
        assert!(!resumed.contains("-n 5000"));
    }
    #[test]
    fn repeated_atomic_save_and_failed_replacement() {
        let dir = std::env::temp_dir().join(format!("dingotail-save-{}", new_id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("settings.json");
        let mut settings = Settings::default();
        for layout in [1, 2, 4, 1] {
            settings.workspace.layout = layout;
            settings.save_to(&path).unwrap();
            assert_eq!(Settings::load_from(&path).unwrap().workspace.layout, layout);
        }
        let blocked = dir.join("directory.json");
        std::fs::create_dir(&blocked).unwrap();
        assert!(settings.save_to(&blocked).is_err());
        assert!(blocked.is_dir());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(blocked).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn dingotail_import_preserves_workspace_and_original() {
        let root = std::env::temp_dir().join(format!("dingotail-import-{}", new_id()));
        let old = root.join("SSHLogTail/settings.json");
        let new = root.join("DingoTail/settings.json");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        let mut settings = Settings {
            accent: [102, 165, 255],
            ..Default::default()
        };
        settings.profiles.push(Profile {
            name: "Existing server".into(),
            remember: true,
            ..Default::default()
        });
        settings.sources.push(Source {
            profile: settings.profiles[0].id.clone(),
            ..Default::default()
        });
        settings.workspace.layout = 4;
        settings.workspace.assign(settings.sources[0].id.clone());
        settings
            .trusted
            .insert("host:22".into(), "fingerprint".into());
        let bytes = serde_json::to_vec(&settings).unwrap();
        std::fs::write(&old, &bytes).unwrap();
        let imported = Settings::load_with_legacy(&new, &old).unwrap();
        assert_eq!(std::fs::read(&old).unwrap(), bytes);
        assert_eq!(
            std::fs::read(new.with_extension("sshlogtail-import.bak")).unwrap(),
            bytes
        );
        assert_eq!(imported.accent, [255, 164, 54]);
        assert_eq!(imported.sources[0].profile, settings.profiles[0].id);
        assert_eq!(imported.workspace.panes, settings.workspace.panes);
        assert!(imported.profiles[0].remember);
        assert_eq!(imported.trusted, settings.trusted);
        assert_eq!(LEGACY_CREDENTIAL_SERVICE, "SSHLogTail");
        let mut custom = imported;
        custom.accent = [1, 2, 3];
        custom.save_to(&new).unwrap();
        assert_eq!(
            Settings::load_with_legacy(&new, &old).unwrap().accent,
            [1, 2, 3]
        );
        std::fs::write(&new, b"invalid").unwrap();
        assert!(Settings::load_with_legacy(&new, &old).is_err());
        std::fs::remove_file(&new).unwrap();
        std::fs::write(&old, b"invalid").unwrap();
        assert!(Settings::load_with_legacy(&new, &old).is_err());
        assert!(!new.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn migrate_and_reorder() {
        let old = serde_json::json!({"profiles":[{"name":"A","host":"a","port":22,"user":"u","key":"","remember":true},{"name":"B","host":"b","port":22,"user":"u","key":"","remember":false}],"sources":[{"profile":1,"path":"/var/log/app.log","journal":false,"sudo":true},{"profile":99,"path":"/broken","journal":true,"sudo":false}]});
        let (mut s, migrated) = Settings::parse(&serde_json::to_vec(&old).unwrap()).unwrap();
        assert!(migrated);
        let id = s.sources[0].profile.clone();
        s.profiles.reverse();
        s.profiles[0].name = "Renamed".into();
        assert_eq!(s.profiles.iter().find(|p| p.id == id).unwrap().host, "b");
        assert_eq!(s.sources[1].profile, "missing-legacy-99");
        assert!(s.profiles.iter().all(|p| p.id != s.sources[1].profile));
        let saved = serde_json::to_vec(&s).unwrap();
        let (restored, migrated) = Settings::parse(&saved).unwrap();
        assert!(!migrated);
        assert_eq!(restored.sources[0].id, s.sources[0].id);
        assert_eq!(restored.sources[0].profile, id);
    }
    #[test]
    fn migration_backup_and_workspace_restore() {
        let dir = std::env::temp_dir().join(format!("sshlogtail-settings-{}", new_id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("settings.json");
        let original = br#"{"profiles":[],"sources":[]}"#;
        std::fs::write(&path, original).unwrap();
        let mut settings = Settings::load_from(&path).unwrap();
        assert_eq!(
            std::fs::read(path.with_extension("v1.bak")).unwrap(),
            original
        );
        let source = Source {
            kind: SourceKind::LocalFile,
            path: "C:\\Logs\\app.log".into(),
            ..Default::default()
        };
        settings.workspace.layout = 4;
        settings.workspace.active_pane = 3;
        settings.workspace.assign(source.id.clone());
        settings.workspace.groups.insert("local".into(), false);
        settings.sources.push(source.clone());
        std::fs::write(&path, serde_json::to_vec(&settings).unwrap()).unwrap();
        let restored = Settings::load_from(&path).unwrap();
        assert_eq!(restored.workspace.layout, 4);
        assert_eq!(restored.workspace.panes[3].as_ref(), Some(&source.id));
        assert!(!restored.workspace.groups["local"]);
        assert_eq!(
            std::fs::read(path.with_extension("v1.bak")).unwrap(),
            original
        );
        settings.sources.push(source);
        assert!(Settings::parse(&serde_json::to_vec(&settings).unwrap()).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(path.with_extension("v1.bak")).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn pane_assignment() {
        let a = Source::default();
        let b = Source::default();
        let mut w = Workspace {
            layout: 4,
            ..Default::default()
        };
        w.assign(a.id.clone());
        w.active_pane = 1;
        w.assign(b.id.clone());
        w.active_pane = 2;
        w.assign(a.id.clone());
        assert_eq!(w.active_pane, 0);
        assert!(w.panes[2].is_none());
        w.layout = 1;
        w.active_pane = 0;
        w.assign(b.id.clone());
        assert!(w.panes[1].is_none());
        assert_eq!(w.panes[0].as_ref(), Some(&b.id));
        w.close(&b.id);
        assert!(w.panes.iter().all(Option::is_none));
    }
    #[test]
    fn byte_limit_and_precedence() {
        let mut rules = Settings::default().rules;
        rules[0].pattern = "INFO".into();
        let compiled: Vec<_> = rules
            .iter()
            .map(|r| {
                (
                    r.clone(),
                    compile(&r.pattern, r.regex, r.case_sensitive).unwrap(),
                )
            })
            .collect();
        let mut buffer = Buffer::default();
        buffer.push("INFO first matching rule".into(), &compiled);
        assert_eq!(buffer.lines[0].color, Some(rules[0].color));
        for _ in 0..300 {
            buffer.push("x".repeat(65536), &[]);
        }
        assert!(buffer.bytes <= 16 * 1024 * 1024);
    }
    #[test]
    fn command_arguments() {
        let mut source = Source {
            path: "/tmp/a'; touch injected; '".into(),
            ..Default::default()
        };
        assert_eq!(
            command(&source, None, DEFAULT_INITIAL_LINES).unwrap(),
            format!("tail -n 200 -F -- {}", quote(&source.path).unwrap())
        );
        source.kind = SourceKind::Journal;
        source.sudo = true;
        source.path = "app.service".into();
        let cmd = command(&source, Some("cursor'quote"), DEFAULT_INITIAL_LINES).unwrap();
        assert!(cmd.starts_with("sudo -n -- journalctl"));
        assert!(cmd.contains("--after-cursor='cursor'\"'\"'quote'"));
    }
    #[test]
    fn quoting() {
        assert_eq!(quote("a'b").unwrap(), "'a'\"'\"'b'");
        assert!(quote("\0").is_err());
    }
    #[test]
    fn decode() {
        let mut d = Decoder::default();
        assert!(d.feed(&[0xe2, 0x82]).is_empty());
        assert_eq!(d.feed(&[0xac, 10]), vec!["€"]);
        let v = vec![b'a'; 70000];
        d.feed(&v);
        assert!(d.feed(b"\n")[0].ends_with("[line truncated]"));
    }
    #[test]
    fn bounded() {
        let mut b = Buffer::default();
        for _ in 0..21000 {
            b.push("test".into(), &[])
        }
        assert_eq!(b.lines.len(), 20000);
        assert_eq!(b.bytes, 80000);
    }
    #[test]
    fn rules() {
        assert!(compile("[", true, false).is_err());
        assert!(compile("error", false, false).unwrap().is_match("ERROR"));
    }
    #[test]
    fn settings_roundtrip() {
        let s = Settings::default();
        let t: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(t.rules.len(), 4);
    }
}
