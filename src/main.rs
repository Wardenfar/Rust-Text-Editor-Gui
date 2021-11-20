extern crate druid;

use std::ops::Sub;
use std::sync::Mutex;

use druid::widget::{Flex, Label, Padding, Painter};
use druid::*;

use crate::editor::TextEditor;
use crate::fs::{FileSystem, LocalFs, LocalPath};
use crate::lsp::LspSystem;
use crate::theme::Theme;

mod buffer;
mod editor;
mod fs;
mod highlight;
mod lsp;
mod theme;

const WINDOW_TITLE: LocalizedString<AppState> = LocalizedString::new("Hello World!");

pub const FONT: Key<FontDescriptor> = Key::new("ui.font");
pub const EDITOR_FONT: Key<FontDescriptor> = Key::new("editor.font");

#[derive(Clone, Data, Lens)]
struct AppState {
    root_path: LocalPath,
    file_path: Option<LocalPath>,
}

lazy_static::lazy_static! {
    pub static ref THEME: Theme = toml::from_str(include_str!("../runtime/themes/default.toml")).unwrap();
    pub static ref FS: LocalFs = LocalFs::default();
    pub static ref LSP: Mutex<LspSystem> = Mutex::new(LspSystem::default());
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dbg!(THEME.scope("keyword"));

    // describe the main window
    let main_window = WindowDesc::new(build_root_widget)
        .title(WINDOW_TITLE)
        .window_size((600.0, 450.0));

    let root = FS.path("./data/example");

    // create the initial app state
    let initial_state = AppState {
        root_path: root,
        file_path: Some(FS.path("./data/example/src/main.rs")),
    };

    // start the application
    AppLauncher::with_window(main_window)
        .delegate(Delegate)
        .launch(initial_state)
        .expect("Failed to launch application");

    Ok(())
}

fn build_root_widget() -> impl Widget<AppState> {
    let button = button("Create", |ctx, _, _| {
        ctx.submit_command(druid::commands::SHOW_OPEN_PANEL.with(FileDialogOptions::new()))
    });
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

fn button<F>(text: &str, on_click: F) -> impl Widget<AppState>
where
    F: Fn(&mut EventCtx, &mut AppState, &Env) + 'static,
{
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
        .on_click(on_click)
}

fn editor() -> impl Widget<AppState> {
    let editor = TextEditor::new();
    editor
}

struct Delegate;

impl AppDelegate<AppState> for Delegate {
    fn command(
        &mut self,
        _ctx: &mut DelegateCtx,
        _target: Target,
        cmd: &Command,
        data: &mut AppState,
        _env: &Env,
    ) -> Handled {
        if let Some(file_info) = cmd.get(commands::OPEN_FILE) {
            if let Some(path) = file_info.path().to_str() {
                data.file_path = Some(FS.path(path));
            }
            Handled::Yes
        } else {
            Handled::No
        }
    }
}
