use std::fs::{File as StdFile, File};
use std::path::PathBuf;

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
}

pub trait Path {
    type Reader;

    fn name(&self) -> String;
    fn path(&self) -> String;
    fn uri(&self) -> Url;
    fn reader(&self) -> Self::Reader;
}
