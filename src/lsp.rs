use std::borrow::Borrow;
use std::collections::HashMap;
use std::io::Write;
use std::process;
use std::process::Command;

use anyhow::Context;
use jsonrpc_core::Output;
use lsp_types::{
    CompletionClientCapabilities, CompletionItem, CompletionResponse, CompletionTextEdit,
    GeneralClientCapabilities, PublishDiagnosticsClientCapabilities, Range,
    TextDocumentClientCapabilities, TextDocumentContentChangeEvent, TraceOption, Url,
    VersionedTextDocumentIdentifier,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::FileSystem;

const ID_INIT: u64 = 0;
const ID_COMPLETION: u64 = 1;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum LspLang {
    Rust,
}

impl LspLang {
    pub fn from_ext<S: Into<String>>(ext: S) -> Option<LspLang> {
        match ext.into().as_str() {
            "rs" => Some(LspLang::Rust),
            _ => None,
        }
    }

    pub fn cmd(&self) -> Command {
        match self {
            LspLang::Rust => {
                let mut cmd = std::process::Command::new("rustup");
                cmd.args(["run", "nightly", "rust-analyzer", "--log-file", "logs.txt"]);
                cmd
            }
        }
    }
}

#[derive(Default)]
pub struct LspSystem {
    clients: HashMap<(Url, LspLang), LspClient>,
}

impl LspSystem {
    pub fn get(&mut self, root_path: Url, lang: LspLang) -> &mut LspClient {
        let key = (root_path.clone(), lang.clone());
        self.clients
            .entry(key)
            .or_insert_with(|| LspClient::new(root_path.clone(), lang.cmd()).unwrap())
    }
}

#[derive(Debug)]
pub struct LspClient {
    process: tokio::process::Child,
    pub input_channel: mpsc::UnboundedSender<LspInput>,
    pub output_channel: mpsc::UnboundedReceiver<LspOutput>,
}

#[derive(Debug)]
pub enum LspInput {
    Edit {
        uri: Url,
        version: i32,
        text: String,
    },
    RequestCompletion {
        uri: Url,
        row: u32,
        col: u32,
    },
    OpenFile {
        uri: Url,
        content: String,
    },
    CloseFile {
        url: Url,
    },
}

#[derive(Debug)]
pub enum LspOutput {
    Completion(Vec<LspCompletion>),
}

#[derive(Debug, Clone)]
pub struct LspCompletion {
    pub label: String,
    pub data: CompletionData,
}

#[derive(Debug, Clone)]
pub enum CompletionData {
    Simple(String),
    Edit { range: Range, new_text: String },
}

#[derive(Debug, Clone)]
pub struct TextEdit {}

impl LspClient {
    fn new(root_path: Url, cmd: Command) -> anyhow::Result<LspClient> {
        let mut lsp = tokio::process::Command::from(cmd)
            .stdin(process::Stdio::piped())
            .stdout(process::Stdio::piped())
            .stderr(process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        #[allow(deprecated)]
        let init = lsp_types::InitializeParams {
            process_id: Some(u32::from(process::id())),
            root_path: None,
            root_uri: Some(root_path),
            initialization_options: None,
            capabilities: lsp_types::ClientCapabilities {
                workspace: None,
                text_document: Some(TextDocumentClientCapabilities {
                    synchronization: None,
                    completion: Some(CompletionClientCapabilities {
                        dynamic_registration: Some(false),
                        completion_item: None,
                        completion_item_kind: None,
                        context_support: Some(true),
                    }),
                    hover: None,
                    signature_help: None,
                    references: None,
                    document_highlight: None,
                    document_symbol: None,
                    formatting: None,
                    range_formatting: None,
                    on_type_formatting: None,
                    declaration: None,
                    definition: None,
                    type_definition: None,
                    implementation: None,
                    code_action: None,
                    code_lens: None,
                    document_link: None,
                    color_provider: None,
                    rename: None,
                    publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
                        related_information: Some(true),
                        tag_support: None,
                        version_support: Some(true),
                        code_description_support: Some(true),
                        data_support: Some(true),
                    }),
                    folding_range: None,
                    selection_range: None,
                    linked_editing_range: None,
                    call_hierarchy: None,
                    semantic_tokens: None,
                    moniker: None,
                }),
                window: None,
                general: Some(GeneralClientCapabilities {
                    regular_expressions: None,
                    markdown: None,
                }),
                experimental: None,
            },
            trace: Some(TraceOption::Verbose),
            workspace_folders: None,
            client_info: None,
            locale: None,
        };

        let mut stdin = lsp.stdin.take().context("take stdin")?;
        let mut reader = tokio::io::BufReader::new(lsp.stdout.take().context("take stdout")?);

        let (init_tx, mut init_rx) = mpsc::unbounded_channel();
        let (tx, rx) = mpsc::unbounded_channel();

        let (c_tx, mut c_rx) = mpsc::unbounded_channel::<LspInput>();
        tokio::spawn(async move {
            send_request_async::<_, lsp_types::request::Initialize>(&mut stdin, ID_INIT, init)
                .await
                .unwrap();
            // Wait initialize
            init_rx.recv().await.unwrap();

            send_notify_async::<_, lsp_types::notification::Initialized>(
                &mut stdin,
                lsp_types::InitializedParams {},
            )
            .await
            .unwrap();

            let mut version = 0;
            let mut edited_text = String::new();

            while let Some(lsp_input) = c_rx.recv().await {
                match lsp_input {
                    LspInput::RequestCompletion { row, col, uri: url } => {
                        let edits = lsp_types::DidChangeTextDocumentParams {
                            text_document: VersionedTextDocumentIdentifier {
                                uri: url.clone(),
                                version,
                            },
                            content_changes: vec![TextDocumentContentChangeEvent {
                                range: None,
                                range_length: None,
                                text: edited_text.clone(),
                            }],
                        };
                        send_notify_async::<_, lsp_types::notification::DidChangeTextDocument>(
                            &mut stdin, edits,
                        )
                        .await
                        .unwrap();
                        let completion = lsp_types::CompletionParams {
                            text_document_position: lsp_types::TextDocumentPositionParams {
                                text_document: lsp_types::TextDocumentIdentifier { uri: url },
                                position: lsp_types::Position {
                                    line: row,
                                    character: col,
                                },
                            },
                            work_done_progress_params: Default::default(),
                            partial_result_params: Default::default(),
                            context: None,
                        };
                        send_request_async::<_, lsp_types::request::Completion>(
                            &mut stdin,
                            ID_COMPLETION,
                            completion,
                        )
                        .await
                        .unwrap();
                    }
                    LspInput::OpenFile { uri: url, content } => {
                        let open = lsp_types::DidOpenTextDocumentParams {
                            text_document: lsp_types::TextDocumentItem {
                                uri: url,
                                language_id: "py".into(),
                                version: 0,
                                text: content,
                            },
                        };
                        send_notify_async::<_, lsp_types::notification::DidOpenTextDocument>(
                            &mut stdin, open,
                        )
                        .await
                        .unwrap();
                    }
                    LspInput::CloseFile { .. } => {}
                    LspInput::Edit {
                        version: v, text, ..
                    } => {
                        version = v;
                        edited_text = text;
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        });

        tokio::spawn(async move {
            let mut headers = HashMap::new();
            loop {
                headers.clear();
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).await? == 0 {
                        return Ok::<(), anyhow::Error>(());
                    }
                    let header = header.trim();
                    if header.is_empty() {
                        break;
                    }
                    let parts: Vec<&str> = header.split(": ").collect();
                    assert_eq!(parts.len(), 2);
                    headers.insert(parts[0].to_string(), parts[1].to_string());
                }
                let content_len = headers["Content-Length"].parse()?;
                let mut content = vec![0; content_len];
                reader.read_exact(&mut content).await?;
                let msg = String::from_utf8(content)?;
                let output: serde_json::Result<Output> = serde_json::from_str(&msg);
                if let Ok(Output::Success(suc)) = output {
                    if suc.id == jsonrpc_core::id::Id::Num(ID_INIT) {
                        init_tx.send(())?;
                    } else if suc.id == jsonrpc_core::id::Id::Num(ID_COMPLETION) {
                        let completion =
                            serde_json::from_value::<lsp_types::CompletionResponse>(suc.result)?;
                        let completions = match completion {
                            CompletionResponse::Array(arr) => convert_completions(arr),
                            CompletionResponse::List(list) => convert_completions(list.items),
                        };
                        dbg!(&completions);
                        tx.send(LspOutput::Completion(completions))?;
                    }
                }
            }
        });

        Ok(Self {
            process: lsp,
            output_channel: rx,
            input_channel: c_tx,
        })
    }
}

fn convert_completions(mut input: Vec<CompletionItem>) -> Vec<LspCompletion> {
    input
        .drain(..)
        .filter_map(|c| {
            if let Some(insert_text) = c.insert_text {
                Some(LspCompletion {
                    label: c.label,
                    data: CompletionData::Simple(insert_text),
                })
            } else if let Some(text_edit) = c.text_edit {
                match text_edit {
                    CompletionTextEdit::Edit(e) => Some(LspCompletion {
                        label: c.label,
                        data: CompletionData::Edit {
                            range: e.range,
                            new_text: e.new_text,
                        },
                    }),
                    CompletionTextEdit::InsertAndReplace(_) => {
                        unimplemented!("insert and replace")
                    }
                }
            } else {
                None
            }
        })
        .collect()
}

async fn send_request_async<T: AsyncWrite + std::marker::Unpin, R: lsp_types::request::Request>(
    t: &mut T,
    id: u64,
    params: R::Params,
) -> anyhow::Result<()>
where
    R::Params: serde::Serialize,
{
    if let serde_json::value::Value::Object(params) = serde_json::to_value(params)? {
        let req = jsonrpc_core::Call::MethodCall(jsonrpc_core::MethodCall {
            jsonrpc: Some(jsonrpc_core::Version::V2),
            method: R::METHOD.to_string(),
            params: jsonrpc_core::Params::Map(params),
            id: jsonrpc_core::Id::Num(id),
        });
        let request = serde_json::to_string(&req)?;
        println!("REQUEST: {}", request);
        let mut buffer: Vec<u8> = Vec::new();
        write!(
            &mut buffer,
            "Content-Length: {}\r\n\r\n{}",
            request.len(),
            request
        )?;
        t.write_all(&buffer).await?;
        Ok(())
    } else {
        anyhow::bail!("Invalid params");
    }
}

async fn send_notify_async<
    T: AsyncWrite + std::marker::Unpin,
    R: lsp_types::notification::Notification,
>(
    t: &mut T,
    params: R::Params,
) -> anyhow::Result<()>
where
    R::Params: serde::Serialize,
{
    if let serde_json::value::Value::Object(params) = serde_json::to_value(params)? {
        let req = jsonrpc_core::Notification {
            jsonrpc: Some(jsonrpc_core::Version::V2),
            method: R::METHOD.to_string(),
            params: jsonrpc_core::Params::Map(params),
        };
        let request = serde_json::to_string(&req)?;
        println!("NOTIFY: {}", request);
        let mut buf: Vec<u8> = Vec::new();
        write!(
            &mut buf,
            "Content-Length: {}\r\n\r\n{}",
            request.len(),
            request
        )?;
        t.write_all(&buf).await?;
        Ok(())
    } else {
        anyhow::bail!("Invalid params")
    }
}
