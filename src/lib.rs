use crate::fs::{FileSystem, LocalPath, Path};
use druid::{Data, FontDescriptor, Key};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

pub mod buffer;
pub mod config;
pub mod draw;
pub mod editor;
pub mod fs;
pub mod highlight;
pub mod lsp;
pub mod lsp_ext;
pub mod style_layer;
pub mod theme;
pub mod tree;

use crate::buffer::Buffer;
use crate::config::Config;
use crate::lsp::{lsp_send_with_lang, LspInput, LspLang};
use anyhow::Context;
use fs::LocalFs;
use lsp::LspSystem;
use lsp_types::Url;
use parking_lot::RwLock;
use theme::Theme;

pub const FONT: Key<FontDescriptor> = Key::new("ui.font");
pub const EDITOR_FONT: Key<FontDescriptor> = Key::new("editor.font");

lazy_static::lazy_static! {
    pub static ref THEME: Theme = toml::from_str(include_str!("../runtime/themes/gruvbox.toml")).unwrap();
    pub static ref FS: LocalFs = LocalFs::default();
    pub static ref LSP: RwLock<LspSystem> = RwLock::new(LspSystem::default());
    pub static ref BUFFERS: RwLock<Buffers> = RwLock::new(Buffers::default());
    pub static ref CONFIG: RwLock<Config> = RwLock::new(Config::default());
    pub static ref GLOBAL: RwLock<Global> = RwLock::new(Global {
        root_path: FS.path(".")
    });
}

#[macro_export]
macro_rules! lock {
    (buffers) => {{
        // println!("{} {}", file!(), line!());
        $crate::BUFFERS.read()
    }};
    (mut buffers) => {{
        // println!("{} {}", file!(), line!());
        $crate::BUFFERS.write()
    }};
    (lsp) => {{
        // println!("lsp {} {}", file!(), line!());
        $crate::LSP.read()
    }};
    (mut lsp) => {{
        // println!("lsp {} {}", file!(), line!());
        $crate::LSP.write()
    }};
    (conf) => {{
        // println!("config {} {}", file!(), line!());
        $crate::CONFIG.read()
    }};
    (mut conf) => {{
        // println!("config {} {}", file!(), line!());
        $crate::CONFIG.write()
    }};
    (global) => {{
        // println!("global {} {}", file!(), line!());
        $crate::GLOBAL.read()
    }};
    (mut global) => {{
        // println!("global {} {}", file!(), line!());
        $crate::GLOBAL.write()
    }};
}

#[macro_export]
macro_rules! curr_buf {
    (row) => {{
        let buffers = lock!(buffers);
        buffers.get_curr()?.buffer.row()
    }};
    (lang) => {{
        let buffers = lock!(buffers);
        buffers.get_curr()?.lsp_lang.clone()
    }};
    (col) => {{
        let buffers = lock!(buffers);
        buffers.get_curr()?.buffer.col()
    }};
    (id) => {{
        let buffers = lock!(buffers);
        buffers.curr()?
    }};
    (text) => {{
        let buffers = lock!(buffers);
        buffers.get_curr()?.buffer.text()
    }};
    (rope) => {{
        let buffers = lock!(buffers);
        buffers.get_curr()?.buffer.rope()
    }};
    (cursor) => {{
        let buffers = lock!(buffers);
        buffers.get_curr()?.buffer.cursor()
    }};
    (uri) => {{
        let buffers = lock!(buffers);
        let buf = buffers.get_curr()?;
        if let $crate::BufferSource::File { path } = &buf.source {
            use $crate::fs::Path;
            Some(path.uri())
        } else {
            None
        }
    }};
}

#[derive(Clone, Data)]
pub struct AppState;

pub struct Global {
    pub root_path: LocalPath,
}

pub struct Buffers {
    counter: AtomicU32,
    pub current: Option<u32>,
    pub buffers: HashMap<u32, BufferData>,
}

impl Default for Buffers {
    fn default() -> Self {
        Self {
            counter: AtomicU32::new(1),
            current: None,
            buffers: Default::default(),
        }
    }
}

impl Buffers {
    pub fn curr(&self) -> anyhow::Result<u32> {
        self.current.context("no current")
    }

    pub fn open_file(&mut self, path: LocalPath) -> anyhow::Result<u32> {
        for (id, b) in &self.buffers {
            if let BufferSource::File { path: p } = &b.source {
                if &path == p {
                    self.current = Some(*id);
                    return Ok(*id);
                }
            }
        }

        let id = self.new_id();

        let source = BufferSource::File { path: path.clone() };

        let data = BufferData {
            id,
            source,
            lsp_lang: path.lsp_lang(),
            read_only: false,
            modified: false,
            buffer: Buffer::from_reader(id, path.reader()),
        };

        let text = data.buffer.text();

        self.buffers.insert(id, data);

        self.current = Some(id);

        let failed = lsp_send_with_lang(
            path.lsp_lang(),
            LspInput::OpenFile {
                uri: path.uri(),
                content: text,
            },
        )
        .is_err();
        if failed {
            println!("lsp start failed")
        }

        Ok(id)
    }

    pub fn new_id(&self) -> u32 {
        self.counter.fetch_add(1, Ordering::SeqCst)
    }

    pub fn get(&self, id: u32) -> anyhow::Result<&BufferData> {
        self.buffers.get(&id).context("no buffer")
    }

    pub fn get_curr(&self) -> anyhow::Result<&BufferData> {
        let id = self.curr()?;
        self.buffers.get(&id).context("no buffer")
    }

    pub fn get_mut(&mut self, id: u32) -> anyhow::Result<&mut BufferData> {
        self.buffers.get_mut(&id).context("no buffer")
    }

    pub fn get_mut_curr(&mut self) -> anyhow::Result<&mut BufferData> {
        let id = self.curr()?;
        self.buffers.get_mut(&id).context("no buffer")
    }
    pub fn get_by_uri(&self, uri: Url) -> Option<&BufferData> {
        for (_, b) in &self.buffers {
            if let BufferSource::File { path } = &b.source {
                if &path.uri().as_str().to_lowercase() == &uri.as_str().to_lowercase() {
                    return Some(b);
                }
            }
        }
        None
    }

    pub fn get_by_uri_mut(&mut self, uri: Url) -> Option<&mut BufferData> {
        for (_, b) in &mut self.buffers {
            if let BufferSource::File { path } = &b.source {
                if path.uri().as_str().to_lowercase() == uri.as_str().to_lowercase() {
                    return Some(b);
                }
            }
        }
        None
    }
}

pub enum BufferSource {
    Text,
    File { path: LocalPath },
}

impl BufferSource {
    pub fn path(&self) -> Option<LocalPath> {
        match self {
            BufferSource::Text => None,
            BufferSource::File { path } => Some(path.clone()),
        }
    }
}

pub struct BufferData {
    pub id: u32,
    pub source: BufferSource,
    pub lsp_lang: LspLang,
    pub read_only: bool,
    pub modified: bool,
    pub buffer: Buffer,
}

pub trait Ignore {
    fn ignore(self);
}

impl<T> Ignore for anyhow::Result<T> {
    fn ignore(self) {}
}
