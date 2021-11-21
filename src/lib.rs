use crate::fs::LocalPath;
use druid::{Data, FontDescriptor, Key, Lens};

pub mod buffer;
pub mod editor;
pub mod fs;
pub mod highlight;
pub mod lsp;
pub mod theme;

use fs::LocalFs;
use lsp::LspSystem;
use std::sync::Mutex;
use theme::Theme;

pub const FONT: Key<FontDescriptor> = Key::new("ui.font");
pub const EDITOR_FONT: Key<FontDescriptor> = Key::new("editor.font");

lazy_static::lazy_static! {
    pub static ref THEME: Theme = toml::from_str(include_str!("../runtime/themes/onedark.toml")).unwrap();
    pub static ref FS: LocalFs = LocalFs::default();
    pub static ref LSP: Mutex<LspSystem> = Mutex::new(LspSystem::default());
}

#[derive(Clone, Data, Lens)]
pub struct AppState {
    pub root_path: LocalPath,
    pub file_path: Option<LocalPath>,
}
