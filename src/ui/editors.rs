// Copyright (C) 2026 Juan Pablo Devoto
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[cfg(test)]
#[path = "editor_tests.rs"]
mod tests;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum AppearanceTab {
    #[default]
    Appearance,
    Highlighting,
}

struct RuleState {
    id: u64,
    checked: Rule,
    compiled: Result<regex::Regex, String>,
}
impl RuleState {
    fn new(id: u64, rule: &Rule) -> Self {
        Self {
            id,
            checked: rule.clone(),
            compiled: compile(&rule.pattern, rule.regex, rule.case_sensitive)
                .map_err(|e| e.to_string()),
        }
    }
}

pub(super) struct AppearanceDraft {
    pub background: [u8; 3],
    pub text: [u8; 3],
    pub accent: [u8; 3],
    pub font: f32,
    pub initial_lines: usize,
    pub rules: Vec<Rule>,
    pub error: String,
    id: String,
    rows: Vec<RuleState>,
    next_id: u64,
    selected: Option<u64>,
    reveal: Option<u64>,
    focus_name: Option<u64>,
    pub test_line: String,
    checked_line: String,
    preview_dirty: bool,
    pub preview_winner: Option<u64>,
    pub preview_valid: bool,
    #[cfg(test)]
    compilations: usize,
    #[cfg(test)]
    preview_evaluations: usize,
}

impl AppearanceDraft {
    pub fn new(settings: &Settings) -> Self {
        let rows: Vec<_> = settings
            .rules
            .iter()
            .enumerate()
            .map(|(i, r)| RuleState::new(i as u64, r))
            .collect();
        let mut draft = Self {
            background: settings.background,
            text: settings.text,
            accent: settings.accent,
            font: settings.font,
            initial_lines: settings.initial_lines,
            rules: settings.rules.clone(),
            error: String::new(),
            id: new_id(),
            selected: rows.first().map(|r| r.id),
            next_id: rows.len() as u64,
            rows,
            reveal: None,
            focus_name: None,
            test_line: "ERROR connection failed for request 42".into(),
            checked_line: String::new(),
            preview_dirty: true,
            preview_winner: None,
            preview_valid: true,
            #[cfg(test)]
            compilations: settings.rules.len(),
            #[cfg(test)]
            preview_evaluations: 0,
        };
        draft.refresh();
        draft
    }

    // Cache only draft-sized data. No log history or worker view is touched here.
    pub fn refresh(&mut self) {
        for (rule, state) in self.rules.iter().zip(&mut self.rows) {
            let old = &state.checked;
            if old.pattern != rule.pattern
                || old.regex != rule.regex
                || old.case_sensitive != rule.case_sensitive
            {
                state.compiled = compile(&rule.pattern, rule.regex, rule.case_sensitive)
                    .map_err(|e| e.to_string());
                #[cfg(test)]
                {
                    self.compilations += 1;
                }
            }
            if old != rule {
                state.checked = rule.clone();
                self.preview_dirty = true;
            }
        }
        if self.preview_dirty || self.checked_line != self.test_line {
            self.preview_valid = self.first_invalid().is_none();
            self.preview_winner = self
                .preview_valid
                .then(|| {
                    self.rules
                        .iter()
                        .zip(&self.rows)
                        .find(|(rule, state)| {
                            rule.enabled
                                && state
                                    .compiled
                                    .as_ref()
                                    .is_ok_and(|re| re.is_match(&self.test_line))
                        })
                        .map(|(_, state)| state.id)
                })
                .flatten();
            self.checked_line.clone_from(&self.test_line);
            self.preview_dirty = false;
            #[cfg(test)]
            {
                self.preview_evaluations += 1;
            }
        }
    }

    pub fn first_invalid(&self) -> Option<usize> {
        self.rules
            .iter()
            .zip(&self.rows)
            .position(|(rule, state)| rule.enabled && state.compiled.is_err())
    }

    pub fn select_invalid(&mut self) {
        if let Some(i) = self.first_invalid() {
            self.selected = Some(self.rows[i].id);
            self.reveal = self.selected;
        }
    }

    fn selected_index(&self) -> Option<usize> {
        self.rows
            .iter()
            .position(|row| Some(row.id) == self.selected)
    }

    fn add_rule(&mut self) {
        let rule = Rule {
            name: "Custom".into(),
            pattern: String::new(),
            regex: false,
            case_sensitive: false,
            enabled: true,
            color: self.text,
            background: self.background,
        };
        let id = self.next_id;
        self.next_id += 1;
        self.rows.push(RuleState::new(id, &rule));
        self.rules.push(rule);
        self.selected = Some(id);
        self.reveal = Some(id);
        self.focus_name = Some(id);
        self.preview_dirty = true;
        #[cfg(test)]
        {
            self.compilations += 1;
        }
    }

    fn remove_selected(&mut self) {
        if let Some(i) = self.selected_index() {
            self.rules.remove(i);
            self.rows.remove(i);
            self.selected = self
                .rows
                .get(i.min(self.rows.len().saturating_sub(1)))
                .map(|r| r.id);
            self.reveal = self.selected;
            self.focus_name = None;
            self.preview_dirty = true;
        }
    }

    fn move_selected(&mut self, delta: isize) {
        if let Some(i) = self.selected_index()
            && let Some(next) = i.checked_add_signed(delta)
            && next < self.rows.len()
        {
            self.rows.swap(i, next);
            self.rules.swap(i, next);
            self.reveal = self.selected;
            self.preview_dirty = true;
        }
    }

    pub fn candidate(&self, settings: &Settings) -> anyhow::Result<Settings> {
        // Revalidate on activation, including callers that edited the draft without rendering.
        for rule in self.rules.iter().filter(|r| r.enabled) {
            compile(&rule.pattern, rule.regex, rule.case_sensitive)
                .map_err(|e| anyhow::anyhow!("Fix or disable rule ‘{}’: {e}", rule.name))?;
        }
        let mut next = settings.clone();
        next.background = self.background;
        next.text = self.text;
        next.accent = self.accent;
        next.font = self.font;
        anyhow::ensure!(
            (1..=MAX_INITIAL_LINES).contains(&self.initial_lines),
            "Initial lines to load must be between 1 and 20,000"
        );
        next.initial_lines = self.initial_lines;
        next.rules = self.rules.clone();
        Ok(next)
    }

    pub fn appearance_ui(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("appearance-body")
            .max_height(ui.available_height()).show(ui, |ui| {
            ui.heading("Log appearance");
            ui.add_space(8.);
            egui::Grid::new("appearance-colors").spacing([16., 12.]).show(ui, |ui| {
                for (name, value) in [
                    ("Background", &mut self.background),
                    ("Text", &mut self.text),
                    ("Accent", &mut self.accent),
                ] {
                    ui.label(name);
                    color_cell(ui, egui::Id::new((&self.id, name)), name, value);
                    ui.end_row();
                }
            });
            ui.add_space(12.);
            ui.add(egui::Slider::new(&mut self.font, 10.0..=24.0).text("Log font size"));
            ui.add_space(12.);
            ui.horizontal_wrapped(|ui| {
                ui.label("Initial lines to load");
                let response = ui.add(egui::DragValue::new(&mut self.initial_lines).range(1..=MAX_INITIAL_LINES).speed(100.));
                name_widget(&response, egui::WidgetType::DragValue, "Initial lines to load");
            });
            ui.label("Applies when starting or restarting local files, SSH files and journals. Running readers keep their current value. Byte limits may retain fewer lines.");
            ui.add_space(16.);
            ui.label("Appearance preview");
            egui::Frame::new().fill(rgb(self.background)).inner_margin(12.).show(ui, |ui| {
                egui::ScrollArea::horizontal().id_salt("appearance-preview").show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new("INFO Connection established\nDefault text without a highlighting rule")
                        .monospace().size(self.font).color(rgb(self.text)))
                        .wrap_mode(egui::TextWrapMode::Extend));
                });
            });
            ui.label(RichText::new("Accent preview").color(rgb(self.accent)));
            ui.add_space(8.);
            ui.label("Preview only. Appearance changes reach open logs when you apply and save.");
        });
    }

    pub fn highlighting_ui(&mut self, ui: &mut egui::Ui) {
        ui.label("First enabled match wins. Rules run from top to bottom.");
        let selected = self.selected_index();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Add rule").clicked() {
                self.add_rule();
            }
            if ui
                .add_enabled(selected.is_some(), egui::Button::new("Remove rule"))
                .clicked()
            {
                self.remove_selected();
            }
            if ui
                .add_enabled(
                    selected.is_some_and(|i| i > 0),
                    egui::Button::new("Move up"),
                )
                .clicked()
            {
                self.move_selected(-1);
            }
            if ui
                .add_enabled(
                    selected.is_some_and(|i| i + 1 < self.rows.len()),
                    egui::Button::new("Move down"),
                )
                .clicked()
            {
                self.move_selected(1);
            }
        });
        ui.add_space(6.);
        if ui.available_width() >= 820. {
            self.table_ui(ui);
            self.refresh();
            self.preview_ui(ui);
        } else {
            egui::ScrollArea::vertical()
                .id_salt("compact-rules-body")
                .max_height(ui.available_height())
                .show(ui, |ui| {
                    self.compact_ui(ui);
                    self.refresh();
                    self.preview_ui(ui);
                });
        }
    }

    fn table_ui(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width() - ui.spacing().scroll.allocated_width();
        let widths = [56., 110., (width - 542.).max(150.), 54., 78., 104., 104.];
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 6.;
            for (text, width) in [
                "Enabled",
                "Name",
                "Pattern",
                "Regex",
                "Match case",
                "Text color",
                "Background",
            ]
            .iter()
            .zip(widths)
            {
                cell(ui, width, |ui| {
                    ui.label(RichText::new(*text).strong());
                });
            }
        });
        ui.separator();
        let height = (ui.available_height() - 132.).max(48.);
        egui::ScrollArea::vertical()
            .id_salt("rule-table")
            .max_height(height)
            .min_scrolled_height(0.)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                if self.rules.is_empty() {
                    ui.label("No highlighting rules. Logs use the default text and background.");
                }
                for i in 0..self.rules.len() {
                    let id = self.rows[i].id;
                    let background = ui.painter().add(egui::Shape::Noop);
                    let mut selected = false;
                    let response = ui
                        .push_id((&self.id, id), |ui| {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.;
                                let rule = &mut self.rules[i];
                                let name = format!("Rule {} ({})", i + 1, rule.name);
                                cell(ui, widths[0], |ui| {
                                    let r = ui.add_sized(
                                        [widths[0], 26.],
                                        egui::Checkbox::new(&mut rule.enabled, ""),
                                    );
                                    r.widget_info(|| {
                                        egui::WidgetInfo::selected(
                                            egui::WidgetType::Checkbox,
                                            r.enabled(),
                                            rule.enabled,
                                            format!("{name}: Enabled"),
                                        )
                                    });
                                    selected |= r.clicked() || r.gained_focus();
                                });
                                cell(ui, widths[1], |ui| {
                                    let r =
                                        rule_text(ui, &self.id, id, "Name", &mut rule.name, &name);
                                    selected |= r.clicked() || r.has_focus();
                                    if self.focus_name == Some(id) {
                                        r.request_focus();
                                        self.focus_name = None;
                                    }
                                });
                                cell(ui, widths[2], |ui| {
                                    let r = rule_text(
                                        ui,
                                        &self.id,
                                        id,
                                        "Pattern",
                                        &mut rule.pattern,
                                        &name,
                                    );
                                    selected |= r.clicked() || r.has_focus();
                                    if let Err(error) = &self.rows[i].compiled {
                                        ui.colored_label(Color32::LIGHT_RED, "Invalid expression")
                                            .on_hover_text(error);
                                    }
                                });
                                for (column, label, value) in [
                                    (3, "Regex", &mut rule.regex),
                                    (4, "Match case", &mut rule.case_sensitive),
                                ] {
                                    cell(ui, widths[column], |ui| {
                                        let r = ui.add_sized(
                                            [widths[column], 26.],
                                            egui::Checkbox::new(value, ""),
                                        );
                                        r.widget_info(|| {
                                            egui::WidgetInfo::selected(
                                                egui::WidgetType::Checkbox,
                                                r.enabled(),
                                                *value,
                                                format!("{name}: {label}"),
                                            )
                                        });
                                        selected |= r.clicked() || r.gained_focus();
                                    });
                                }
                                for (column, label, value) in [
                                    (5, "Text color", &mut rule.color),
                                    (6, "Background", &mut rule.background),
                                ] {
                                    cell(ui, widths[column], |ui| {
                                        let r = color_cell(
                                            ui,
                                            egui::Id::new((&self.id, id, label)),
                                            &format!("{name}: {label}"),
                                            value,
                                        );
                                        selected |= r.clicked() || r.gained_focus();
                                    });
                                }
                            })
                        })
                        .inner;
                    if selected {
                        self.selected = Some(id);
                    }
                    if self.selected == Some(id) {
                        ui.painter().set(
                            background,
                            egui::Shape::rect_filled(
                                response.response.rect.expand(2.),
                                2.,
                                ui.visuals().selection.bg_fill,
                            ),
                        );
                    }
                    if self.reveal == Some(id) {
                        response.response.scroll_to_me(Some(egui::Align::Center));
                        self.reveal = None;
                    }
                    ui.add_space(4.);
                }
            });
    }

    fn compact_ui(&mut self, ui: &mut egui::Ui) {
        ui.label("Select a rule to edit");
        egui::ScrollArea::vertical()
            .id_salt("compact-rule-list")
            .max_height(120.)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (i, (rule, row)) in self.rules.iter().zip(&self.rows).enumerate() {
                    let label = format!(
                        "{}. {}{}{}",
                        i + 1,
                        rule.name,
                        if rule.enabled { "" } else { " · disabled" },
                        if row.compiled.is_err() {
                            " · invalid expression"
                        } else {
                            ""
                        }
                    );
                    let r = ui.selectable_label(self.selected == Some(row.id), label);
                    if r.clicked() {
                        self.selected = Some(row.id);
                    }
                    if self.reveal == Some(row.id) {
                        r.scroll_to_me(Some(egui::Align::Center));
                        self.reveal = None;
                    }
                }
            });
        let Some(i) = self.selected_index() else {
            ui.label("No highlighting rules. Add a rule to begin.");
            return;
        };
        ui.separator();
        let rule = &mut self.rules[i];
        let id = self.rows[i].id;
        let name = format!("Rule {} ({})", i + 1, rule.name);
        ui.checkbox(&mut rule.enabled, "Enabled");
        ui.label("Name");
        let response = rule_text(ui, &self.id, id, "Name", &mut rule.name, &name);
        if self.focus_name == Some(id) {
            response.request_focus();
            self.focus_name = None;
        }
        ui.label("Pattern");
        rule_text(ui, &self.id, id, "Pattern", &mut rule.pattern, &name);
        if let Err(error) = &self.rows[i].compiled {
            validation_error(ui, "Pattern", error);
        }
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut rule.regex, "Regex");
            ui.checkbox(&mut rule.case_sensitive, "Match case");
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Text color");
            color_cell(
                ui,
                egui::Id::new((&self.id, id, "Text color")),
                &format!("{name}: Text color"),
                &mut rule.color,
            );
            ui.label("Background");
            color_cell(
                ui,
                egui::Id::new((&self.id, id, "Background")),
                &format!("{name}: Background"),
                &mut rule.background,
            );
        });
    }

    fn preview_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.);
        ui.separator();
        let label = ui.label("Test line");
        let response = ui
            .add(
                egui::TextEdit::singleline(&mut self.test_line)
                    .id(egui::Id::new((&self.id, "test-line")))
                    .char_limit(4096)
                    .desired_width(f32::INFINITY),
            )
            .labelled_by(label.id);
        if response.changed() {
            self.refresh();
        }
        let winner = self
            .preview_winner
            .and_then(|id| self.rows.iter().position(|r| r.id == id));
        let (text, background) = if let Some(i) = winner {
            ui.label(format!("Matched by rule {}: {}", i + 1, self.rules[i].name));
            (self.rules[i].color, self.rules[i].background)
        } else {
            if self.preview_valid {
                ui.label("Default style — no enabled rule matched");
            } else {
                ui.colored_label(
                    Color32::LIGHT_RED,
                    "Highlighting preview unavailable — fix or disable invalid rules.",
                );
            }
            (self.text, self.background)
        };
        egui::Frame::new()
            .fill(rgb(background))
            .inner_margin(8.)
            .show(ui, |ui| {
                egui::ScrollArea::horizontal()
                    .id_salt("highlight-preview")
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&self.test_line)
                                    .monospace()
                                    .size(self.font)
                                    .color(rgb(text)),
                            )
                            .wrap_mode(egui::TextWrapMode::Extend),
                        );
                    });
            });
    }
}

fn cell(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(width);
            add(ui);
        },
    );
}

fn name_widget(response: &egui::Response, kind: egui::WidgetType, name: &str) {
    response.widget_info(|| egui::WidgetInfo::labeled(kind, response.enabled(), name));
}

fn rule_text(
    ui: &mut egui::Ui,
    draft: &str,
    row: u64,
    field: &str,
    text: &mut String,
    name: &str,
) -> egui::Response {
    let response = ui.add_sized(
        [ui.available_width(), 26.],
        egui::TextEdit::singleline(text)
            .id(egui::Id::new((draft, row, field)))
            .desired_width(f32::INFINITY),
    );
    name_widget(
        &response,
        egui::WidgetType::TextEdit,
        &format!("{name}: {field}"),
    );
    response
}

pub(super) fn contrast_text(value: [u8; 3]) -> Color32 {
    let luminance = value
        .iter()
        .zip([0.2126, 0.7152, 0.0722])
        .map(|(v, w)| {
            let c = *v as f32 / 255.;
            w * if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        })
        .sum::<f32>();
    if luminance > 0.179 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

fn color_cell(ui: &mut egui::Ui, id: egui::Id, name: &str, value: &mut [u8; 3]) -> egui::Response {
    let hex = format!("#{:02X}{:02X}{:02X}", value[0], value[1], value[2]);
    let foreground = contrast_text(*value);
    let mut response = ui
        .push_id(id, |ui| {
            ui.add(
                egui::Button::new(RichText::new(hex).monospace().color(foreground))
                    .fill(rgb(*value))
                    .min_size(egui::vec2(96., 26.)),
            )
        })
        .inner;
    name_widget(&response, egui::WidgetType::Button, name);
    let mut color = rgb(*value);
    egui::Popup::menu(&response)
        .id(id.with("picker"))
        .show(|ui| {
            if egui::color_picker::color_picker_color32(
                ui,
                &mut color,
                egui::color_picker::Alpha::Opaque,
            ) {
                *value = [color.r(), color.g(), color.b()];
                response.mark_changed();
            }
        });
    response.on_hover_text(format!("{name} — select to edit color"))
}

fn validation_error(ui: &mut egui::Ui, field: &str, error: &str) {
    ui.colored_label(
        Color32::LIGHT_RED,
        format!("{field}: invalid regular expression"),
    );
    ui.push_id(field, |ui| {
        ui.collapsing("Error details", |ui| {
            ui.label(error);
        });
    });
}

pub(super) fn filter_panel(ui: &mut egui::Ui, view: &mut View, open: &mut bool, font: f32) -> bool {
    let mut changed = false;
    let mut clear = false;
    let width = (ui.available_width() - 8.).max(32.);
    let height = (ui.available_height()
        - (2. * (font + 5.)).min(ui.available_height() * 0.5)
        - 6. // Separator below the editor.
        - 2. * ui.spacing().item_spacing.y)
        .clamp(0., 220.);
    egui::ScrollArea::vertical().id_salt("filter-editor").max_height(height)
        .min_scrolled_height(0.).auto_shrink([false, true]).show(ui, |ui| {
        ui.set_width(width);
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        ui.horizontal(|ui| {
            ui.strong("Filters");
            if ui.small_button("Hide filters").clicked() { *open = false; }
        });
        for (id, label, value, error) in [
            ("include", "Include matching lines", &mut view.query, &view.include_error),
            ("exclude", "Exclude matching lines", &mut view.exclude, &view.exclude_error),
        ] {
            let edit = |ui: &mut egui::Ui, value: &mut String| {
                let response = ui.add_sized([ui.available_width(), 22.],
                    egui::TextEdit::singleline(value).id_salt(id));
                name_widget(&response, egui::WidgetType::TextEdit, label);
                response
            };
            let response = if width < 420. {
                ui.label(label);
                edit(ui, value)
            } else {
                ui.horizontal(|ui| {
                    ui.add_sized([152., 22.], egui::Label::new(label));
                    edit(ui, value)
                }).inner
            };
            changed |= response.changed();
            if let Some(error) = error {
                ui.painter().rect_stroke(response.rect, 2., egui::Stroke::new(1_f32, Color32::LIGHT_RED), egui::StrokeKind::Inside);
                validation_error(ui, label, error);
            }
        }
        ui.horizontal_wrapped(|ui| {
            changed |= ui.checkbox(&mut view.regex, "Regular expression").changed();
            clear = ui.button("Clear filters").clicked();
        });
        ui.label(RichText::new("Matches ignore case. Regex applies to both fields. Filters use retained lines.").small());
        if clear { view.clear_filters(); changed = false; }
        if view.filter_pending() { ui.label("Updating filter…"); }
        else if !view.error.is_empty() { ui.label("Showing results from the previous valid filter"); }
    });
    ui.separator();
    changed
}
