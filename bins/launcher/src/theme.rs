// SPDX-License-Identifier: GPL-3.0-or-later
//! The launcher's look: a neutral night theme with no blue tint (the default), a warm day theme,
//! flat fills, 1px lines, 2px corners on controls, and installed system fonts only.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke,
    TextStyle, Visuals,
};
use serde::{Deserialize, Serialize};

/// The theme the player picked; night is the default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    /// Neutral near-black.
    #[default]
    Night,
    /// Warm paper.
    Day,
}

impl ThemeChoice {
    /// Both choices, in the order the settings show them.
    pub const ALL: [Self; 2] = [Self::Night, Self::Day];

    /// The name shown on the theme button.
    pub fn label(self) -> &'static str {
        match self {
            Self::Night => "Night",
            Self::Day => "Day",
        }
    }

    fn egui_theme(self) -> egui::Theme {
        match self {
            Self::Night => egui::Theme::Dark,
            Self::Day => egui::Theme::Light,
        }
    }
}

/// The colours of one theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// The window and the base of every control.
    pub ground: Color32,
    /// Quiet surfaces.
    pub surface: Color32,
    /// Raised fills.
    pub raised: Color32,
    /// Text and the 2px rules.
    pub ink: Color32,
    /// Labels, detail lines and hints.
    pub mute: Color32,
    /// 1px dividers and field borders.
    pub line: Color32,
    /// Control colour: button text and borders, the selected fill.
    pub accent: Color32,
    /// Ready state.
    pub ready: Color32,
    /// Busy or strained state text.
    pub warn: Color32,
    /// Error state.
    pub alert: Color32,
}

/// Signal yellow: the focus outline and the text selection, the same in both themes.
pub const SIGNAL_YELLOW: Color32 = Color32::from_rgb(0xf2, 0xb7, 0x05);

/// The night theme.
pub const NIGHT: Palette = Palette {
    ground: Color32::from_rgb(0x12, 0x12, 0x12),
    surface: Color32::from_rgb(0x1a, 0x1a, 0x1a),
    raised: Color32::from_rgb(0x26, 0x26, 0x26),
    ink: Color32::from_rgb(0xe8, 0xe6, 0xe1),
    mute: Color32::from_rgb(0xa3, 0xa0, 0x9a),
    line: Color32::from_rgb(0x30, 0x30, 0x30),
    accent: Color32::from_rgb(0xd9, 0xd6, 0xcf),
    ready: Color32::from_rgb(0x4c, 0xaf, 0x6e),
    warn: SIGNAL_YELLOW,
    alert: Color32::from_rgb(0xf0, 0x47, 0x5c),
};

/// The day theme.
pub const DAY: Palette = Palette {
    ground: Color32::from_rgb(0xf4, 0xf1, 0xea),
    surface: Color32::from_rgb(0xeb, 0xe6, 0xdb),
    raised: Color32::from_rgb(0xe2, 0xdc, 0xcd),
    ink: Color32::from_rgb(0x1f, 0x1f, 0x1f),
    mute: Color32::from_rgb(0x63, 0x5e, 0x54),
    line: Color32::from_rgb(0xd8, 0xd1, 0xc2),
    accent: Color32::from_rgb(0x13, 0x29, 0x4b),
    // `day-state-ready`: signal green (#2e7d4f) gives 4.47:1 on the day ground, under AA.
    ready: Color32::from_rgb(0x2b, 0x76, 0x4a),
    warn: Color32::from_rgb(0x7a, 0x56, 0x00),
    alert: Color32::from_rgb(0xc8, 0x10, 0x2e),
};

impl Palette {
    /// The palette of the theme egui shows now.
    pub fn current(visuals: &Visuals) -> &'static Self {
        if visuals.dark_mode { &NIGHT } else { &DAY }
    }
}

/// Corner of buttons and fields.
pub const CONTROL_CORNER: u8 = 2;
/// Width of a button border.
const BUTTON_BORDER: f32 = 1.5;
/// Width of a field border and a divider.
pub const LINE_WIDTH: f32 = 1.0;
/// Width of the focus outline.
const FOCUS_WIDTH: f32 = 2.0;
/// Share of the accent mixed into the ground on hover, and when pressed.
pub const HOVER_MIX: f32 = 0.16;
const PRESS_MIX: f32 = 0.28;
/// Opacity of the text selection fill.
const SELECTION_ALPHA: u8 = 0x59;
/// Opacity factor of a disabled control.
const DISABLED_ALPHA: f32 = 0.45;
/// Space between items, and inside buttons.
const ITEM_SPACING: [f32; 2] = [8.0, 8.0];
const BUTTON_PADDING: [f32; 2] = [12.0, 5.0];
/// Height of a control.
const CONTROL_HEIGHT: f32 = 28.0;

/// `a` mixed with `share` of `b`, per sRGB channel (as CSS `color-mix(in srgb)`).
pub fn mix(a: Color32, b: Color32, share: f32) -> Color32 {
    let channel = |from: u8, to: u8| {
        let value = f32::from(from) + (f32::from(to) - f32::from(from)) * share;
        // The value is between two u8 values, so it fits.
        value.round() as u8
    };
    Color32::from_rgb(
        channel(a.r(), b.r()),
        channel(a.g(), b.g()),
        channel(a.b(), b.b()),
    )
}

/// The egui visuals of a palette.
pub fn visuals(palette: &Palette) -> Visuals {
    let dark = palette == &NIGHT;
    let mut visuals = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let corner = CornerRadius::same(CONTROL_CORNER);
    let control = |fill: Color32, border: Stroke| egui::style::WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: fill,
        bg_stroke: border,
        corner_radius: corner,
        fg_stroke: Stroke::new(LINE_WIDTH, palette.accent),
        expansion: 0.0,
    };
    let border = Stroke::new(BUTTON_BORDER, palette.accent);
    let widgets = &mut visuals.widgets;
    widgets.noninteractive = egui::style::WidgetVisuals {
        fg_stroke: Stroke::new(LINE_WIDTH, palette.ink),
        ..control(palette.ground, Stroke::new(LINE_WIDTH, palette.line))
    };
    widgets.inactive = control(palette.ground, border);
    widgets.hovered = control(mix(palette.ground, palette.accent, HOVER_MIX), border);
    widgets.active = control(
        mix(palette.ground, palette.accent, PRESS_MIX),
        Stroke::new(FOCUS_WIDTH, SIGNAL_YELLOW),
    );
    widgets.open = widgets.active;
    visuals.selection.bg_fill = SIGNAL_YELLOW.gamma_multiply_u8(SELECTION_ALPHA);
    visuals.selection.stroke = Stroke::new(FOCUS_WIDTH, SIGNAL_YELLOW);
    visuals.text_cursor.stroke = Stroke::new(FOCUS_WIDTH, palette.accent);
    visuals.hyperlink_color = palette.accent;
    visuals.faint_bg_color = palette.surface;
    visuals.extreme_bg_color = palette.ground;
    visuals.text_edit_bg_color = Some(palette.ground);
    visuals.code_bg_color = palette.raised;
    visuals.warn_fg_color = palette.warn;
    visuals.error_fg_color = palette.alert;
    visuals.window_fill = palette.surface;
    visuals.window_stroke = Stroke::new(LINE_WIDTH, palette.line);
    visuals.window_corner_radius = CornerRadius::ZERO;
    visuals.menu_corner_radius = corner;
    visuals.window_shadow = Shadow::NONE;
    visuals.popup_shadow = Shadow::NONE;
    visuals.panel_fill = palette.ground;
    visuals.striped = false;
    visuals.disabled_alpha = DISABLED_ALPHA;
    visuals
}

/// Family name of the condensed face used for labels, titles and buttons.
pub const CONDENSED: &str = "condensed";
/// Font name of the body face and of the data face.
const BODY_FACE: &str = "body";
const DATA_FACE: &str = "data";
/// Text style of a section head and a fact label (label type).
pub const LABEL_STYLE: &str = "label";
/// Text style of the server name and the state word (title type).
pub const TITLE_STYLE: &str = "title";

/// Font size of each role.
const BODY_SIZE: f32 = 14.0;
const SMALL_SIZE: f32 = 12.0;
const BUTTON_SIZE: f32 = 13.0;
const HEADING_SIZE: f32 = 22.0;
const TITLE_SIZE: f32 = 15.0;
const LABEL_SIZE: f32 = 11.0;
const DATA_SIZE: f32 = 12.0;

fn condensed() -> FontFamily {
    FontFamily::Name(CONDENSED.into())
}

/// Spacing and text sizes, the same in both themes.
pub fn apply_style(style: &mut egui::Style) {
    style.spacing.item_spacing = ITEM_SPACING.into();
    style.spacing.button_padding = BUTTON_PADDING.into();
    style.spacing.interact_size.y = CONTROL_HEIGHT;
    style.text_styles = [
        (TextStyle::Body, FontId::proportional(BODY_SIZE)),
        (TextStyle::Small, FontId::proportional(SMALL_SIZE)),
        (TextStyle::Button, FontId::new(BUTTON_SIZE, condensed())),
        (TextStyle::Heading, FontId::new(HEADING_SIZE, condensed())),
        (TextStyle::Monospace, FontId::monospace(DATA_SIZE)),
        (
            TextStyle::Name(TITLE_STYLE.into()),
            FontId::new(TITLE_SIZE, condensed()),
        ),
        (
            TextStyle::Name(LABEL_STYLE.into()),
            FontId::new(LABEL_SIZE, condensed()),
        ),
    ]
    .into();
}

/// A font role and the installed files that can fill it, best first.
struct FontRole {
    name: &'static str,
    files: &'static [&'static str],
}

/// Installed faces per role on Linux (Fira Sans, then common faces).
#[cfg(unix)]
const FONT_ROLES: [FontRole; 3] = [
    FontRole {
        name: BODY_FACE,
        files: &[
            "/usr/share/fonts/opentype/fira/FiraSans-Regular.otf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ],
    },
    FontRole {
        name: CONDENSED,
        files: &[
            "/usr/share/fonts/opentype/fira/FiraSansCondensed-SemiBold.otf",
            "/usr/share/fonts/truetype/noto/NotoSans-CondensedSemiBold.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansCondensed-Bold.ttf",
        ],
    },
    FontRole {
        name: DATA_FACE,
        files: &["/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"],
    },
];

/// Installed faces per role on Windows, under the Windows font folder
/// (Segoe UI, Arial Narrow, Cascadia Mono, Consolas).
#[cfg(windows)]
const FONT_ROLES: [FontRole; 3] = [
    FontRole {
        name: BODY_FACE,
        files: &["segoeui.ttf"],
    },
    FontRole {
        name: CONDENSED,
        files: &["arialnb.ttf", "segoeuisb.ttf"],
    },
    FontRole {
        name: DATA_FACE,
        files: &["CascadiaMono.ttf", "consola.ttf"],
    },
];

/// Windows folder variable and the font folder under it.
#[cfg(windows)]
const WINDOWS_DIR_ENV: &str = "WINDIR";
#[cfg(windows)]
const WINDOWS_FONTS_DIR: &str = "Fonts";

/// The folder the font file names of [`FONT_ROLES`] are relative to.
fn font_dir() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var_os(WINDOWS_DIR_ENV)
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(WINDOWS_FONTS_DIR)
    }
    #[cfg(unix)]
    {
        PathBuf::new()
    }
}

/// The first readable file of `files` under `dir`.
fn first_font(dir: &Path, files: &[&str]) -> Option<Vec<u8>> {
    files
        .iter()
        .find_map(|file| std::fs::read(dir.join(file)).ok())
}

/// egui's built-in faces with each found system face first in its family. A role with no
/// installed face keeps the built-in face; the condensed family then uses the body faces.
fn font_definitions(found: &[(&'static str, Vec<u8>)]) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let proportional = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    fonts.families.insert(condensed(), proportional);
    for (role, data) in found {
        fonts.font_data.insert(
            (*role).to_owned(),
            Arc::new(FontData::from_owned(data.clone())),
        );
        let family = match *role {
            CONDENSED => condensed(),
            DATA_FACE => FontFamily::Monospace,
            _ => FontFamily::Proportional,
        };
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, (*role).to_owned());
    }
    fonts
}

/// Sets the fonts, the style and both themes, and shows `choice`.
pub fn install(ctx: &egui::Context, choice: ThemeChoice) {
    let dir = font_dir();
    let found: Vec<_> = FONT_ROLES
        .iter()
        .filter_map(|role| first_font(&dir, role.files).map(|data| (role.name, data)))
        .collect();
    ctx.set_fonts(font_definitions(&found));
    ctx.set_visuals_of(egui::Theme::Dark, visuals(&NIGHT));
    ctx.set_visuals_of(egui::Theme::Light, visuals(&DAY));
    ctx.all_styles_mut(apply_style);
    show(ctx, choice);
}

/// Shows the theme of `choice`.
pub fn show(ctx: &egui::Context, choice: ThemeChoice) {
    ctx.set_theme(choice.egui_theme());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2.1 AA: normal text, and large text or a control border.
    const TEXT_CONTRAST: f64 = 4.5;
    const LARGE_CONTRAST: f64 = 3.0;
    const SRGB_KNEE: f64 = 0.040_45;
    const SRGB_LINEAR_SLOPE: f64 = 12.92;
    const SRGB_OFFSET: f64 = 0.055;
    const SRGB_GAMMA: f64 = 2.4;
    const LUMINANCE_WEIGHTS: [f64; 3] = [0.2126, 0.7152, 0.0722];
    const CONTRAST_OFFSET: f64 = 0.05;
    const CHANNEL_MAX: f64 = 255.0;

    fn luminance(color: Color32) -> f64 {
        let linear = |channel: u8| {
            let value = f64::from(channel) / CHANNEL_MAX;
            if value <= SRGB_KNEE {
                value / SRGB_LINEAR_SLOPE
            } else {
                ((value + SRGB_OFFSET) / (1.0 + SRGB_OFFSET)).powf(SRGB_GAMMA)
            }
        };
        let [r, g, b] = LUMINANCE_WEIGHTS;
        r * linear(color.r()) + g * linear(color.g()) + b * linear(color.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (light, dark) = {
            let (x, y) = (luminance(a), luminance(b));
            if x > y { (x, y) } else { (y, x) }
        };
        (light + CONTRAST_OFFSET) / (dark + CONTRAST_OFFSET)
    }

    #[test]
    fn text_and_state_colours_pass_aa_on_both_grounds() {
        for palette in [&NIGHT, &DAY] {
            for (name, color) in [
                ("ink", palette.ink),
                ("mute", palette.mute),
                ("accent", palette.accent),
                ("ready", palette.ready),
                ("warn", palette.warn),
                ("alert", palette.alert),
            ] {
                assert!(
                    contrast(color, palette.ground) >= TEXT_CONTRAST,
                    "{name} on {:?}",
                    palette.ground
                );
            }
            let hovered = mix(palette.ground, palette.accent, HOVER_MIX);
            assert!(contrast(palette.accent, hovered) >= TEXT_CONTRAST);
            assert!(contrast(palette.ground, palette.accent) >= TEXT_CONTRAST);
            assert!(contrast(palette.accent, palette.ground) >= LARGE_CONTRAST);
        }
    }

    #[test]
    fn night_neutrals_have_no_blue_tint() {
        for color in [NIGHT.ground, NIGHT.surface, NIGHT.raised, NIGHT.line] {
            assert!(
                color.b() <= color.r() && color.b() <= color.g(),
                "{color:?}"
            );
        }
    }

    #[test]
    fn mix_moves_each_channel_by_the_share() {
        let black = Color32::from_rgb(0, 0, 0);
        let white = Color32::from_rgb(200, 100, 50);
        assert_eq!(mix(black, white, 0.0), black);
        assert_eq!(mix(black, white, 1.0), white);
        assert_eq!(mix(black, white, 0.5), Color32::from_rgb(100, 50, 25));
    }

    #[test]
    fn each_theme_has_its_own_visuals() {
        assert!(visuals(&NIGHT).dark_mode);
        assert!(!visuals(&DAY).dark_mode);
        assert_eq!(visuals(&NIGHT).panel_fill, NIGHT.ground);
        assert_eq!(visuals(&DAY).panel_fill, DAY.ground);
        assert_eq!(Palette::current(&visuals(&DAY)), &DAY);
        assert_eq!(Palette::current(&visuals(&NIGHT)), &NIGHT);
    }

    #[test]
    fn the_first_readable_font_file_is_taken() {
        let dir = tempfile::tempdir().expect("dir");
        std::fs::write(dir.path().join("second.ttf"), b"second").expect("font");
        std::fs::write(dir.path().join("third.ttf"), b"third").expect("font");
        assert_eq!(
            first_font(dir.path(), &["first.ttf", "second.ttf", "third.ttf"]),
            Some(b"second".to_vec())
        );
        assert_eq!(first_font(dir.path(), &["none.ttf"]), None);
    }

    #[test]
    fn found_faces_go_first_and_condensed_always_exists() {
        let none = font_definitions(&[]);
        assert_eq!(
            none.families.get(&condensed()),
            none.families.get(&FontFamily::Proportional)
        );
        let fonts = font_definitions(&[(CONDENSED, b"c".to_vec()), (DATA_FACE, b"d".to_vec())]);
        assert_eq!(fonts.families[&condensed()][0], CONDENSED);
        assert_eq!(fonts.families[&FontFamily::Monospace][0], DATA_FACE);
        assert_ne!(fonts.families[&FontFamily::Proportional][0], CONDENSED);
    }

    #[test]
    fn the_theme_choice_is_saved_in_lower_case() {
        assert_eq!(
            serde_json::to_string(&ThemeChoice::Day).expect("json"),
            "\"day\""
        );
        assert_eq!(ThemeChoice::default(), ThemeChoice::Night);
    }
}
