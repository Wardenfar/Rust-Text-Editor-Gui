use std::collections::HashMap;
use std::io::Write;
use std::process;
use std::process::Command;
use std::sync::atomic::Ordering;

use anyhow::Context;
use jsonrpc_core::id::Id;
use jsonrpc_core::Output;
use lsp_types::*;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::ChildStdin;
use tokio::sync::mpsc;

use crate::buffer::{Bounds, IntoWithBuffer};
use crate::{lock, BufferSource, Path, GLOBAL, LSP};

const ID_INIT: u64 = 0;
const ID_COMPLETION: u64 = 1;
const ID_COMPLETION_RESOLVE: u64 = 2;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum LspLang {
    Rust,
    Json,
    Python,
    PlainText,
}

impl LspLang {
    pub fn from_ext<S: Into<String>>(ext: S) -> Option<LspLang> {
        match ext.into().as_str() {
            "rs" => Some(LspLang::Rust),
            "py" => Some(LspLang::Python),
            "json" => Some(LspLang::Json),
            _ => None,
        }
    }

    pub fn cmd(&self) -> Option<Command> {
        match self {
            LspLang::Rust => {
                let mut cmd = std::process::Command::new("rustup");
                cmd.args(["run", "nightly", "rust-analyzer"]);
                Some(cmd)
            }
            _ => None,
        }
    }
}

pub fn lsp_send(buffer_id: u32, input: LspInput) -> anyhow::Result<()> {
    let global = GLOBAL.lock().unwrap();
    let root_path = &global.root_path;

    let buffers = lock!(buffers);
    let buffer = buffers.get(buffer_id)?;

    let mut lsp = LSP.lock().unwrap();
    let client = lsp
        .get(root_path.uri(), &buffer.lsp_lang)
        .context("no lsp client")?;
    client.input_channel.send(input)?;
    Ok(())
}

pub fn lsp_send_with_lang(lsp_lang: LspLang, input: LspInput) -> anyhow::Result<()> {
    let global = GLOBAL.lock().unwrap();
    let root_path = &global.root_path;

    let mut lsp = LSP.lock().unwrap();
    let client = lsp
        .get(root_path.uri(), &lsp_lang)
        .context("no lsp client")?;
    client.input_channel.send(input)?;
    Ok(())
}

pub fn lsp_try_recv(buffer_id: u32) -> anyhow::Result<LspOutput> {
    let global = GLOBAL.lock().unwrap();
    let root_path = &global.root_path;

    let buffers = lock!(buffers);
    let buffer = buffers.get(buffer_id)?;

    let mut lsp = LSP.lock().unwrap();
    let client = lsp
        .get(root_path.uri(), &buffer.lsp_lang)
        .context("no lsp client found")?;
    let result = client.output_channel.try_recv()?;
    Ok(result)
}

#[derive(Default)]
pub struct LspSystem {
    clients: HashMap<(Url, LspLang), LspClient>,
}

impl LspSystem {
    pub fn get(&mut self, root_path: Url, lang: &LspLang) -> Option<&mut LspClient> {
        let key = (root_path.clone(), lang.clone());
        if let Some(cmd) = lang.cmd() {
            let client = self
                .clients
                .entry(key)
                .or_insert_with(|| LspClient::new(root_path.clone(), cmd).unwrap());
            Some(client)
        } else {
            None
        }
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
        buffer_id: u32,
        version: i32,
        text: String,
    },
    RequestCompletion {
        buffer_id: u32,
        row: u32,
        col: u32,
    },
    RequestCompletionResolve {
        buffer_id: u32,
        item: CompletionItem,
    },
    OpenFile {
        uri: Url,
        content: String,
    },
    CloseFile {
        uri: Url,
    },
    SavedFile {
        uri: Url,
        content: String,
    },
}

#[derive(Debug)]
pub enum LspOutput {
    Completion(Vec<LspCompletion>),
    CompletionResolve(LspCompletion),
    Diagnostics,
}

#[derive(Debug, Clone)]
pub struct LspCompletion {
    pub original_item: CompletionItem,
    pub label: String,
    pub data: CompletionData,
}

#[derive(Debug, Clone)]
pub enum CompletionData {
    Simple(String),
    Edits(Vec<TextEdit>),
}

#[derive(Debug, Clone)]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}

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
                    synchronization: Some(TextDocumentSyncClientCapabilities {
                        dynamic_registration: Some(false),
                        will_save: Some(false),
                        will_save_wait_until: Some(false),
                        did_save: Some(true),
                    }),
                    completion: Some(CompletionClientCapabilities {
                        dynamic_registration: Some(false),
                        completion_item: Some(CompletionItemCapability {
                            snippet_support: Some(false),
                            commit_characters_support: None,
                            documentation_format: None,
                            deprecated_support: None,
                            preselect_support: None,
                            tag_support: None,
                            insert_replace_support: None,
                            resolve_support: Some(CompletionItemCapabilityResolveSupport {
                                properties: vec!["additionalTextEdits".into()],
                            }),
                            insert_text_mode_support: None,
                        }),
                        completion_item_kind: None,
                        context_support: Some(false),
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
                    code_action: Some(CodeActionClientCapabilities {
                        dynamic_registration: Some(false),
                        code_action_literal_support: None,
                        is_preferred_support: None,
                        disabled_support: None,
                        data_support: Some(true),
                        resolve_support: Some(CodeActionCapabilityResolveSupport {
                            properties: vec![],
                        }),
                        honors_change_annotations: None,
                    }),
                    code_lens: None,
                    document_link: None,
                    color_provider: None,
                    rename: None,
                    publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
                        related_information: Some(true),
                        tag_support: Some(TagSupport {
                            value_set: vec![DiagnosticTag::DEPRECATED, DiagnosticTag::UNNECESSARY],
                        }),
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

            while let Some(lsp_input) = c_rx.recv().await {
                let r = Self::process_input(&mut stdin, lsp_input).await;
                if let Err(e) = r {
                    println!("{}", e);
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
                let notification: serde_json::Result<serde_json::Value> =
                    serde_json::from_str(&msg);
                if let Ok(Output::Success(suc)) = output {
                    println!("{}", suc.result);
                    if suc.id == Id::Num(ID_INIT) {
                        init_tx.send(())?;
                    } else if suc.id == Id::Num(ID_COMPLETION) {
                        let completion =
                            serde_json::from_value::<lsp_types::CompletionResponse>(suc.result)?;
                        let completions = match completion {
                            CompletionResponse::Array(arr) => convert_completions(arr),
                            CompletionResponse::List(list) => convert_completions(list.items),
                        };
                        tx.send(LspOutput::Completion(completions))?;
                    } else if suc.id == Id::Num(ID_COMPLETION_RESOLVE) {
                        let item: CompletionItem = serde_json::from_value(suc.result)?;
                        tx.send(LspOutput::CompletionResolve(
                            convert_completion(item).unwrap(),
                        ))?;
                    }
                } else if let Ok(notification) = notification {
                    if notification.get("method").unwrap() == "textDocument/publishDiagnostics" {
                        let params: PublishDiagnosticsParams =
                            serde_json::from_value(notification.get("params").unwrap().clone())
                                .unwrap();
                        let diagnostics = params.diagnostics;
                        process_diagnostics(params.uri.clone(), diagnostics);
                        tx.send(LspOutput::Diagnostics)?;
                    } else {
                        println!("{:?}", notification);
                    }
                } else {
                    println!("fail : {}", msg);
                }
            }
        });

        Ok(Self {
            process: lsp,
            output_channel: rx,
            input_channel: c_tx,
        })
    }

    async fn process_input(mut stdin: &mut ChildStdin, lsp_input: LspInput) -> anyhow::Result<()> {
        match lsp_input {
            LspInput::RequestCompletion {
                row,
                col,
                buffer_id,
            } => {
                let (path, version, text) = {
                    let buffers = lock!(buffers);
                    let buffer = buffers.get(buffer_id)?;
                    (
                        buffer.source.path().context("path")?,
                        buffer.buffer.version.fetch_add(1, Ordering::SeqCst),
                        buffer.buffer.text(),
                    )
                };
                let url = path.uri();
                let edits = lsp_types::DidChangeTextDocumentParams {
                    text_document: VersionedTextDocumentIdentifier {
                        uri: url.clone(),
                        version,
                    },
                    content_changes: vec![TextDocumentContentChangeEvent {
                        range: None,
                        range_length: None,
                        text,
                    }],
                };
                send_notify_async::<_, lsp_types::notification::DidChangeTextDocument>(
                    &mut stdin, edits,
                )
                .await?;
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
                    context: Some(CompletionContext {
                        trigger_kind: CompletionTriggerKind::INVOKED,
                        trigger_character: None,
                    }),
                };
                send_request_async::<_, lsp_types::request::Completion>(
                    &mut stdin,
                    ID_COMPLETION,
                    completion,
                )
                .await
                .unwrap();
            }
            LspInput::RequestCompletionResolve { item, .. } => {
                request_resolve_completion_item(&mut stdin, item)
                    .await
                    .unwrap();
            }
            LspInput::OpenFile { uri: url, content } => {
                notify_did_open(&mut stdin, url, content).await.unwrap();
            }
            LspInput::CloseFile { uri } => {
                notify_did_close(&mut stdin, uri).await.unwrap();
            }
            LspInput::SavedFile { uri, content } => {
                notify_did_save(&mut stdin, uri, content).await.unwrap();
            }
            LspInput::Edit {
                version: _v,
                text: _,
                buffer_id: _,
            } => {}
        }
        Ok(())
    }
}

fn convert_completions(mut input: Vec<CompletionItem>) -> Vec<LspCompletion> {
    input
        .drain(..)
        .filter_map(|c| convert_completion(c))
        .collect()
}

fn convert_completion(c: CompletionItem) -> Option<LspCompletion> {
    let clone = c.clone();
    if let Some(insert_text) = c.insert_text {
        Some(LspCompletion {
            original_item: clone,
            label: c.label,
            data: CompletionData::Simple(insert_text),
        })
    } else if let Some(text_edit) = c.text_edit {
        let mut edits = vec![];
        match text_edit {
            CompletionTextEdit::Edit(e) => edits.push(TextEdit {
                range: e.range,
                new_text: e.new_text,
            }),
            CompletionTextEdit::InsertAndReplace(_) => {
                unimplemented!("insert and replace")
            }
        }
        if let Some(additionals) = c.additional_text_edits {
            for a in additionals {
                edits.push(TextEdit {
                    range: a.range,
                    new_text: a.new_text,
                })
            }
        }
        Some(LspCompletion {
            original_item: clone,
            label: c.label,
            data: CompletionData::Edits(edits),
        })
    } else {
        None
    }
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
            id: Id::Num(id),
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

// lsp notify dud save
async fn notify_did_save<T: AsyncWrite + std::marker::Unpin>(
    stdin: &mut T,
    uri: Url,
    content: String,
) -> anyhow::Result<()> {
    let params = lsp_types::DidSaveTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
        text: Some(content),
    };
    send_notify_async::<_, lsp_types::notification::DidSaveTextDocument>(stdin, params).await
}

// lsp notify did close
async fn notify_did_close<T: AsyncWrite + std::marker::Unpin>(
    stdin: &mut T,
    uri: Url,
) -> anyhow::Result<()> {
    let params = lsp_types::DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
    };
    send_notify_async::<_, lsp_types::notification::DidCloseTextDocument>(stdin, params).await
}

// lsp notify did open
async fn notify_did_open<T: AsyncWrite + std::marker::Unpin>(
    stdin: &mut T,
    uri: Url,
    text: String,
) -> anyhow::Result<()> {
    let params = lsp_types::DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri,
            language_id: "rust".to_string(),
            version: 0,
            text,
        },
    };
    send_notify_async::<_, lsp_types::notification::DidOpenTextDocument>(stdin, params).await
}

// lsp request resolve completion item
async fn request_resolve_completion_item<T: AsyncWrite + std::marker::Unpin>(
    stdin: &mut T,
    item: CompletionItem,
) -> anyhow::Result<()> {
    send_request_async::<_, lsp_types::request::ResolveCompletionItem>(
        stdin,
        ID_COMPLETION_RESOLVE,
        item,
    )
    .await
}

fn process_diagnostics(default_uri: Url, diagnostics: Vec<Diagnostic>) {
    let mut buffers = lock!(mut buffers);

    let mut cleared = Vec::new();
    for diagnostic in diagnostics {
        let mut uri = default_uri.clone();
        if let Some(infos) = &diagnostic.related_information {
            for info in infos {
                uri = info.location.uri.clone();
            }
        }
        let buf = buffers.buffers.iter_mut().find(|(_, b)| {
            if let BufferSource::File { path } = &b.source {
                uri.as_str().to_lowercase() == path.uri().as_str().to_lowercase()
            } else {
                false
            }
        });
        if let Some((id, buf)) = buf {
            if !cleared.contains(id) {
                buf.buffer.diagnostics.0.clear();
                cleared.push(*id);
            }
            let bounds: Bounds = (&diagnostic.range).into_with_buf(&buf.buffer);
            buf.buffer.diagnostics.0.push(crate::buffer::Diagnostic {
                bounds,
                severity: diagnostic.severity.unwrap_or(DiagnosticSeverity::ERROR),
                message: diagnostic.message,
            });
        }
    }
}
