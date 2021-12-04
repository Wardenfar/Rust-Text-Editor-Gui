use crate::LspLang;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct Config {
    pub lsp: LspConfig,
    pub render: RenderConfig,
    pub extensions: Vec<Extension>,
}

#[derive(Deserialize, Serialize)]
pub struct Extension {
    pub file_extension: Vec<String>,
    pub file_names: Vec<String>,
    pub lang: LspLang,
}

impl Default for Config {
    fn default() -> Self {
        let mut extensions = Vec::new();
        extensions.push(Extension {
            file_extension: vec!["rs".to_string()],
            file_names: vec!["Cargo.toml".to_string()],
            lang: LspLang::Rust,
        });
        extensions.push(Extension {
            file_extension: vec!["py".to_string()],
            file_names: vec!["requirements.txt".to_string()],
            lang: LspLang::Python,
        });
        extensions.push(Extension {
            file_extension: vec!["json".to_string()],
            file_names: vec![],
            lang: LspLang::Json,
        });
        Self {
            lsp: LspConfig::default(),
            render: RenderConfig::default(),
            extensions,
        }
    }
}

#[derive(Deserialize, Serialize)]
pub struct RenderConfig {
    pub text_scale: f64,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self { text_scale: 1.0 }
    }
}

#[derive(Deserialize, Serialize)]
pub struct LspConfig {
    pub servers: Vec<LspServer>,
}

#[derive(Deserialize, Serialize)]
pub struct LspServer {
    pub lang: LspLang,
    pub command: Vec<String>,
}

impl Default for LspConfig {
    fn default() -> Self {
        let mut servers = Vec::new();
        servers.push(LspServer {
            lang: LspLang::Rust,
            command: vec![
                "rustup".into(),
                "run".into(),
                "nightly".into(),
                "rust-analyzer".into(),
            ],
        });
        servers.push(LspServer {
            lang: LspLang::Python,
            command: vec!["pylsp".into()],
        });
        Self { servers }
    }
}
