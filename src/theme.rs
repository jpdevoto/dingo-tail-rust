// Copyright (C) 2026 Juan Pablo Devoto
// SPDX-License-Identifier: GPL-3.0-only

use eframe::egui::{self, Color32, RichText};
#[derive(Clone, Copy)]
pub enum Icon {
    Menu,
    Settings,
    About,
    Add,
    Folder,
    Connect,
    Edit,
    Delete,
    Close,
    Play,
    Stop,
    Follow,
    Clear,
    Search,
    Copy,
    Export,
    Single,
    Columns,
    Grid,
    Warning,
    Up,
    More,
}
impl Icon {
    pub fn glyph(self) -> char {
        match self {
            Self::Menu => '\u{e5d2}',
            Self::Settings => '\u{e8b8}',
            Self::About => '\u{e88e}',
            Self::Add => '\u{e145}',
            Self::Folder => '\u{e2c8}',
            Self::Connect => '\u{e157}',
            Self::Edit => '\u{e3c9}',
            Self::Delete => '\u{e872}',
            Self::Close => '\u{e5cd}',
            Self::Play => '\u{e037}',
            Self::Stop => '\u{e047}',
            Self::Follow => '\u{e258}',
            Self::Clear => '\u{e16c}',
            Self::Search => '\u{e8b6}',
            Self::Copy => '\u{f08a}',
            Self::Export => '\u{e2c4}',
            Self::Single => '\u{e3c6}',
            Self::Columns => '\u{e8ec}',
            Self::Grid => '\u{e9b0}',
            Self::Warning => '\u{e002}',
            Self::Up => '\u{e316}',
            Self::More => '\u{eae1}',
        }
    }
    fn text(self) -> RichText {
        RichText::new(self.glyph().to_string()).font(egui::FontId::new(
            20.,
            egui::FontFamily::Name("material".into()),
        ))
    }
}
pub fn dialog(
    ctx: &egui::Context,
    title: impl Into<egui::WidgetText>,
    id: impl std::hash::Hash,
    width: f32,
    height: f32,
) -> egui::Window<'static> {
    let area = ctx.content_rect().shrink(12.);
    egui::Window::new(title)
        .id(egui::Id::new(("dialogs-v3", id)))
        .collapsible(false)
        .constrain_to(area)
        .default_size(egui::vec2(
            width.min(area.width()),
            height.min(area.height() - 36.),
        ))
        .min_width(280f32.min(area.width()))
        .max_width(area.width())
        .max_height((area.height() - 36.).max(120.))
}
/// Reserve fixed action targets before allocating the flexible text field.
pub fn input_actions(
    ui: &mut egui::Ui,
    value: &mut String,
    actions: &[(Icon, &str)],
) -> Option<usize> {
    let mut clicked = None;
    let width = ui.available_width();
    let reserved = actions.len() as f32 * 38.;
    let mut buttons = |ui: &mut egui::Ui| {
        for (i, (icon, label)) in actions.iter().enumerate() {
            if icon_button(ui, *icon, label, false).clicked() {
                clicked = Some(i);
            }
        }
    };
    if width - reserved >= 120. {
        ui.horizontal(|ui| {
            ui.add_sized([width - reserved, 32.], egui::TextEdit::singleline(value));
            buttons(ui);
        });
    } else {
        ui.add_sized([width, 32.], egui::TextEdit::singleline(value));
        ui.horizontal(buttons);
    }
    clicked
}
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "material".into(),
        egui::FontData::from_static(include_bytes!(
            "../assets/icons/MaterialIconsOutlined-Regular.otf"
        ))
        .into(),
    );
    fonts.families.insert(
        egui::FontFamily::Name("material".into()),
        vec!["material".into()],
    );
    ctx.set_fonts(fonts);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6., 6.);
        s.spacing.button_padding = egui::vec2(8., 5.);
        s.spacing.combo_width = 180.;
    });
}
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, label: &str, selected: bool) -> egui::Response {
    let text = icon.text();
    let button = if selected {
        egui::Button::new(text.color(Color32::from_rgb(18, 21, 27)))
            .min_size(egui::vec2(32., 32.))
            .fill(ui.visuals().selection.stroke.color)
    } else {
        egui::Button::new(text)
            .min_size(egui::vec2(32., 32.))
            .frame(true)
            .frame_when_inactive(false)
    };
    let r = ui
        .push_id(label, |ui| {
            ui.spacing_mut().button_padding = egui::vec2(6., 4.);
            ui.add(button)
        })
        .inner;
    r.widget_info(|| {
        if matches!(
            icon,
            Icon::Single | Icon::Columns | Icon::Grid | Icon::Follow
        ) {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                ui.is_enabled(),
                selected,
                label,
            )
        } else {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
        }
    });
    r.on_hover_text(label)
}
pub fn icon_menu(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    active: bool,
    contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let mut text = icon.text();
    if active {
        text = text.color(ui.visuals().selection.stroke.color);
    }
    let r = ui
        .push_id(icon.glyph(), |ui| {
            ui.spacing_mut().button_padding = egui::vec2(6., 4.);
            egui::containers::menu::MenuButton::from_button(
                egui::Button::new(text).min_size(egui::vec2(32., 32.)),
            )
            .config(
                egui::containers::menu::MenuConfig::new()
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
            )
            .ui(ui, contents)
        })
        .inner;
    r.0.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
    r.0.on_hover_text(label)
}
pub fn texture(ctx: &egui::Context, name: &str, bytes: &[u8]) -> egui::TextureHandle {
    let image = image::load_from_memory(bytes)
        .expect("embedded brand PNG")
        .into_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    ctx.load_texture(
        name,
        egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
        egui::TextureOptions::LINEAR,
    )
}
pub fn window_icon() -> egui::IconData {
    let image = image::load_from_memory(include_bytes!("../assets/brand/icon-256.png"))
        .expect("embedded icon")
        .into_rgba8();
    egui::IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filter_popup_stays_open_during_editing() {
        let ctx = egui::Context::default();
        install(&ctx);
        let mut text = String::new();
        let mut button = egui::Rect::NOTHING;
        let mut field = egui::Rect::NOTHING;
        let mut shown = false;
        let mut run = |events: Vec<egui::Event>| {
            shown = false;
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(500., 400.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        button = icon_menu(
                            ui,
                            Icon::Search,
                            if text.is_empty() {
                                "Filters"
                            } else {
                                "Filters active"
                            },
                            !text.is_empty(),
                            |ui| {
                                shown = true;
                                field = ui.text_edit_singleline(&mut text).rect;
                            },
                        )
                        .rect;
                    });
                },
            );
            (button, field, shown, text.clone())
        };
        let (button, _, _, _) = run(vec![]);
        let click = |pos| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ]
        };
        run(click(button.center()));
        let (_, field, shown, _) = run(vec![]);
        assert!(shown);
        run(click(field.center()));
        let (_, _, shown, text) = run(vec![egui::Event::Text("ERROR".into())]);
        assert!(shown);
        assert_eq!(text, "ERROR");
        assert!(
            run(vec![]).2,
            "Changing the tooltip must not change popup identity"
        );
    }
    #[test]
    fn embedded_resources_and_material_glyphs() {
        let icon = window_icon();
        assert_eq!((icon.width, icon.height), (256, 256));
        assert!(icon.rgba.as_chunks::<4>().0.iter().any(|p| p[3] == 0));
        let map = include_str!("../assets/icons/MaterialIconsOutlined-Regular.codepoints");
        for icon in [
            Icon::Menu,
            Icon::Settings,
            Icon::About,
            Icon::Add,
            Icon::Folder,
            Icon::Connect,
            Icon::Edit,
            Icon::Delete,
            Icon::Close,
            Icon::Play,
            Icon::Stop,
            Icon::Follow,
            Icon::Clear,
            Icon::Search,
            Icon::Copy,
            Icon::Export,
            Icon::Single,
            Icon::Columns,
            Icon::Grid,
            Icon::Warning,
            Icon::Up,
            Icon::More,
        ] {
            assert!(
                map.lines()
                    .any(|l| l.ends_with(&format!(" {:x}", icon.glyph() as u32)))
            );
        }
        for bytes in [
            include_bytes!("../assets/brand/wordmark.png").as_slice(),
            include_bytes!("../assets/brand/mascot.png").as_slice(),
        ] {
            assert!(image::load_from_memory(bytes).is_ok());
        }
    }
}
