mod buffer;
mod editor;
mod highlight;
mod theme;

extern crate druid;

use crate::editor::TextEditor;
use crate::theme::Theme;
use druid::widget::{Flex, Label, Padding, Painter};
use druid::*;
use std::ops::Sub;

const WINDOW_TITLE: LocalizedString<AppState> = LocalizedString::new("Hello World!");

pub const FONT: Key<FontDescriptor> = Key::new("ui.font");
pub const EDITOR_FONT: Key<FontDescriptor> = Key::new("editor.font");

#[derive(Clone, Data, Lens)]
struct AppState {
    text: String,
}

lazy_static::lazy_static! {
    pub static ref THEME: Theme = toml::from_str(include_str!("../runtime/themes/onedark.toml")).unwrap();
}

fn main() {
    // describe the main window
    let main_window = WindowDesc::new(build_root_widget)
        .title(WINDOW_TITLE)
        .window_size((600.0, 450.0));

    // create the initial app state
    let initial_state = AppState {
        text: include_str!("../file.json").into(),
    };

    // start the application
    AppLauncher::with_window(main_window)
        .launch(initial_state)
        .expect("Failed to launch application");
}

fn build_root_widget() -> impl Widget<AppState> {
    let button = button("Create");
    let editor = editor();

    // arrange the two widgets vertically, with some padding
    let layout = Flex::column()
        .with_default_spacer()
        .with_flex_child(editor, 1.0)
        .with_default_spacer()
        .with_child(button)
        .with_default_spacer();

    let layout = Flex::row()
        .with_default_spacer()
        .with_flex_child(layout, 1.0)
        .with_default_spacer();

    // center the two widgets in the available space
    layout.env_scope(|env: &mut druid::Env, _data: &AppState| {
        env.set(
            FONT,
            FontDescriptor::new(FontFamily::new_unchecked("Segoe UI"))
                .with_weight(FontWeight::NORMAL)
                .with_size(14.0),
        );

        env.set(
            EDITOR_FONT,
            FontDescriptor::new(FontFamily::MONOSPACE)
                .with_weight(FontWeight::NORMAL)
                .with_size(18.0),
        );
    })
}

fn button(text: &str) -> impl Widget<AppState> {
    let my_painter = Painter::new(|ctx, _: &AppState, _| {
        let bounds = ctx.size().to_rect();
        let bounds = bounds.sub(Insets::uniform(2.0));

        ctx.fill(bounds.to_rounded_rect(2.5), &Color::rgb8(54, 88, 128));
        ctx.stroke(bounds.to_rounded_rect(2.5), &Color::rgb8(76, 112, 140), 1.0);

        if ctx.is_hot() {
            ctx.fill(bounds, &Color::rgba8(0, 0, 0, 128));
        }

        if ctx.is_active() {
            ctx.stroke(bounds, &Color::WHITE, 2.0);
        }
    });

    let button = Label::new(text)
        .with_text_color(Color::WHITE)
        .with_font(FONT);

    Padding::new(Insets::new(18.0, 5.0, 18.0, 5.0), button)
        .background(my_painter)
        .on_click(|_, _data: &mut AppState, _| {})
}

fn editor() -> impl Widget<AppState> {
    let editor = TextEditor::new();
    editor
}
