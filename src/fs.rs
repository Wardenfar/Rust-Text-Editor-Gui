use std::fs::{File as StdFile, File};
use std::path::PathBuf;

use crate::lock;
use crate::lsp::LspLang;
use crate::tree::{ItemStyle, ShouldRepaint, Tree};
use druid::{Data, KbKey};
use lsp_types::Url;

#[derive(Default, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct LocalFs {}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct LocalPath {
    inner: PathBuf,
}

impl LocalPath {
    pub fn file_name(&self) -> String {
        self.inner.file_name().unwrap().to_str().unwrap().into()
    }
    pub fn extension(&self) -> Option<String> {
        self.inner.extension().map(|e| e.to_str().unwrap().into())
    }
}

impl Data for LocalPath {
    fn same(&self, other: &Self) -> bool {
        self == other
    }
}

impl FileSystem for LocalFs {
    type Path = LocalPath;

    fn path<S>(&self, path: S) -> LocalPath
    where
        S: Into<String>,
    {
        LocalPath {
            inner: PathBuf::from(path.into()).canonicalize().unwrap(),
        }
    }

    fn list(&self, path: Self::Path) -> Vec<Self::Path> {
        if path.inner.is_dir() {
            path.inner
                .read_dir()
                .unwrap()
                .map(|e| LocalPath {
                    inner: e.unwrap().path(),
                })
                .collect()
        } else {
            vec![]
        }
    }
}

impl Path for LocalPath {
    type Reader = File;
    type Writer = File;

    fn lsp_lang(&self) -> LspLang {
        let name = self.name();
        let config = lock!(conf);
        let lang = config
            .extensions
            .iter()
            .find(|e| e.file_names.contains(&name))
            .map(|e| e.lang.clone());

        if let Some(lang) = lang {
            return lang;
        }

        if let Some(ext) = self.extension() {
            config
                .extensions
                .iter()
                .find(|e| e.file_extension.contains(&ext))
                .map(|e| e.lang.clone())
                .unwrap_or(LspLang::PlainText)
        } else {
            LspLang::PlainText
        }
    }

    fn name(&self) -> String {
        self.inner.file_name().unwrap().to_str().unwrap().into()
    }

    fn path(&self) -> String {
        self.inner.canonicalize().unwrap().to_str().unwrap().into()
    }

    fn uri(&self) -> Url {
        Url::from_file_path(self.path()).unwrap()
    }

    fn reader(&self) -> Self::Reader {
        StdFile::open(&self.inner).unwrap()
    }
    fn writer(&self) -> Self::Writer {
        StdFile::create(&self.inner).unwrap()
    }
}

pub trait FileSystem {
    type Path;

    fn path<S>(&self, path: S) -> Self::Path
    where
        S: Into<String>;

    fn list(&self, path: Self::Path) -> Vec<Self::Path>;
}

pub trait Path {
    type Reader;
    type Writer;

    fn lsp_lang(&self) -> LspLang;
    fn name(&self) -> String;
    fn path(&self) -> String;
    fn uri(&self) -> Url;
    fn reader(&self) -> Self::Reader;
    fn writer(&self) -> Self::Writer;
}

impl Tree for LocalFs {
    type Key = LocalPath;

    fn root(&self) -> Self::Key {
        let global = lock!(global);
        global.root_path.clone()
    }

    fn children(&self, parent: &Self::Key) -> Vec<Self::Key> {
        let mut list = self.list(parent.clone());
        list.sort_by_key(|k| k.file_name());
        list.sort_by_key(|k| if k.inner.is_dir() { 1 } else { 2 });
        list
    }

    fn refresh(&self, _parent: &Self::Key) {}

    fn item(&self, key: &Self::Key) -> ItemStyle {
        let level = key.inner.components().count() - self.root().inner.components().count();
        let style_scope = if key.inner.is_dir() {
            "tree.dir"
        } else {
            "tree.file"
        };
        ItemStyle {
            text: key.file_name(),
            style_scope: style_scope.into(),
            level,
        }
    }

    fn key_down(&mut self, selected: &Self::Key, key: &KbKey) -> ShouldRepaint {
        if key == &KbKey::Enter && selected.inner.is_file() {
            let mut buffers = lock!(mut buffers);
            buffers.open_file(selected.clone()).unwrap();
            true
        } else {
            false
        }
    }
}
