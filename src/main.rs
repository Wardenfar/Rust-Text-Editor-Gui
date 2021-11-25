extern crate druid;

use std::ops::Sub;

use druid::widget::{Flex, Label, Padding, Painter, Split};
use druid::*;
use ste_lib::{AppState, EDITOR_FONT, FONT, FS};

use ste_lib::editor::TextEditor;
use ste_lib::fs::FileSystem;
use ste_lib::tree::TreeViewer;

const WINDOW_TITLE: LocalizedString<AppState> = LocalizedString::new("Hello World!");

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // describe the main window
    let main_window = WindowDesc::new(build_root_widget)
        .title(WINDOW_TITLE)
        .window_size((600.0, 450.0));

    let root = FS.path("./data/example");

    // create the initial app state
    let mut initial_state = AppState {
        root_path: root,
        current: None,
        opened: vec![],
    };

    initial_state.open(FS.path("./data/example/src/main.rs"));

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
        .with_flex_child(editor, 1.0)
        .with_default_spacer()
        .with_child(button)
        .with_default_spacer();

    let tree = TreeViewer::new(FS.clone());

    let layout = Split::columns(tree, layout)
        .draggable(true)
        .split_point(0.3);

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
                data.open(FS.path(path));
            }
            Handled::Yes
        } else {
            Handled::No
        }
    }
}
