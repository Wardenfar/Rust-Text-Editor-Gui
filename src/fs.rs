use std::fs::{File as StdFile, File};
use std::path::PathBuf;

use crate::tree::{ItemStyle, Tree};
use crate::{AppState, THEME};
use druid::Data;
use lsp_types::Url;

#[derive(Default, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct LocalFs {}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct LocalPath {
    inner: PathBuf,
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
            inner: PathBuf::from(path.into()),
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

    fn name(&self) -> String;
    fn path(&self) -> String;
    fn uri(&self) -> Url;
    fn reader(&self) -> Self::Reader;
}

impl Tree for LocalFs {
    type Key = LocalPath;

    fn root(&self, data: &AppState) -> Self::Key {
        data.root_path.clone()
    }

    fn children(&self, _data: &AppState, parent: &Self::Key) -> Vec<Self::Key> {
        self.list(parent.clone())
    }

    fn refresh(&self, _data: &AppState, _parent: &Self::Key) {}

    fn item(&self, data: &AppState, key: &Self::Key) -> ItemStyle {
        let level = key.inner.components().count() - data.root_path.inner.components().count();
        let style = if key.inner.is_dir() {
            THEME.scope("tree.dir")
        } else {
            THEME.scope("tree.file")
        };
        ItemStyle {
            text: key.inner.file_name().unwrap().to_str().unwrap().into(),
            style,
            level,
        }
    }
}
