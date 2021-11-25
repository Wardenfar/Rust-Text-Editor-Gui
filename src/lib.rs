use crate::fs::{LocalPath, Path};
use druid::{Data, FontDescriptor, Key, Lens};

pub mod buffer;
pub mod editor;
pub mod fs;
pub mod highlight;
pub mod lsp;
pub mod theme;
pub mod tree;

use crate::lsp::{lsp_send, LspInput};
use fs::LocalFs;
use lsp::LspSystem;
use std::sync::Mutex;
use theme::Theme;

pub const FONT: Key<FontDescriptor> = Key::new("ui.font");
pub const EDITOR_FONT: Key<FontDescriptor> = Key::new("editor.font");

lazy_static::lazy_static! {
    pub static ref THEME: Theme = toml::from_str(include_str!("../runtime/themes/gruvbox.toml")).unwrap();
    pub static ref FS: LocalFs = LocalFs::default();
    pub static ref LSP: Mutex<LspSystem> = Mutex::new(LspSystem::default());
}

#[derive(Clone, Lens)]
pub struct AppState {
    pub root_path: LocalPath,
    pub current: Option<LocalPath>,
    pub opened: Vec<LocalPath>,
}

impl Data for AppState {
    fn same(&self, other: &Self) -> bool {
        self.root_path.same(&other.root_path) && self.current.same(&other.current)
    }
}

impl AppState {
    pub fn curr(&self) -> Option<LocalPath> {
        self.current.clone()
    }

    pub fn open(&mut self, path: LocalPath) {
        self.current = Some(path.clone());
        if !self.opened.contains(&path) {
            self.opened.push(path);
        }
    }

    pub fn close(&mut self, path: LocalPath) {
        let pos = self.opened.iter().position(|o| o == &path);
        if let Some(pos) = pos {
            self.opened.remove(pos);

            lsp_send(
                self.root_path.uri(),
                path.lsp_lang(),
                LspInput::CloseFile { uri: path.uri() },
            );

            if self.current.is_some() {
                if self.opened.is_empty() {
                    self.current = None;
                } else {
                    let curr = self.curr().unwrap();
                    if curr == path {
                        self.current = self.opened.get(0).cloned()
                    }
                }
            }
        }
    }
}
