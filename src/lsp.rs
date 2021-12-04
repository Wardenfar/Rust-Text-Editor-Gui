use std::collections::HashMap;
use std::io::Write;
use std::process;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context;
use jsonrpc_core::id::Id;
use jsonrpc_core::Output;
use lsp_types::request::Request;
use lsp_types::*;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::ChildStdin;
use tokio::sync::mpsc;

use crate::buffer::{Bounds, IntoWithBuffer};
use crate::lsp_ext::{InlayHint, InlayKind};
use crate::{lock, lsp_ext, Path};

#[derive(Debug, Clone, Hash, Eq, PartialEq, Deserialize, Serialize)]
pub enum LspLang {
    Rust,
    Json,
    Python,
    PlainText,
}

impl LspLang {
    pub fn cmd(&self) -> Option<Command> {
        let config = lock!(conf);

        for server in &config.lsp.servers {
            if &server.lang == self {
                let parts = &server.command;
                let mut cmd = std::process::Command::new(&parts[0]);
                cmd.args(parts.iter().skip(1));
                return Some(cmd);
            }
        }

        None
    }
}

pub fn lsp_send(buffer_id: u32, input: LspInput) -> anyhow::Result<()> {
    let global = lock!(global);
    let root_path = &global.root_path;

    let buffers = lock!(buffers);
    let buffer = buffers.get(buffer_id)?;

    let mut lsp = lock!(mut lsp);
    let client = lsp
        .get(root_path.uri(), &buffer.lsp_lang)
        .context("no lsp client")?;
    client.input_channel.send(input)?;
    Ok(())
}

pub fn lsp_send_with_lang(lsp_lang: LspLang, input: LspInput) -> anyhow::Result<()> {
    let global = lock!(global);
    let root_path = &global.root_path;

    let mut lsp = lock!(mut lsp);
    let client = lsp
        .get(root_path.uri(), &lsp_lang)
        .context("no lsp client")?;
    client.input_channel.send(input)?;
    Ok(())
}

pub fn lsp_try_recv(buffer_id: u32) -> anyhow::Result<LspOutput> {
    let global = lock!(global);
    let root_path = &global.root_path;

    let buffers = lock!(buffers);
    let buffer = buffers.get(buffer_id)?;

    let mut lsp = lock!(mut lsp);
    let client = lsp
        .get(root_path.uri(), &buffer.lsp_lang)
        .context("no lsp client found")?;
    let result = client.output_channel.try_recv()?;
    Ok(result)
}

#[derive(Default)]
pub struct LspSystem {
    clients: HashMap<(Url, LspLang), LspClient>,
    counter: AtomicU64,
    requests: HashMap<u64, SentRequest>,
}

pub struct SentRequest {
    pub method: String,
    pub uri: Url,
}

impl LspSystem {
    pub fn new_request(&mut self, method: String, uri: Url) -> u64 {
        let id = self.counter.fetch_add(1, Ordering::SeqCst);
        self.requests.insert(id, SentRequest { method, uri });
        id
    }

    pub fn get_request(&mut self, id: u64) -> Option<SentRequest> {
        self.requests.remove(&id)
    }

    pub fn get(&mut self, root_path: Url, lang: &LspLang) -> Option<&mut LspClient> {
        let key = (root_path.clone(), lang.clone());
        if let Some(cmd) = lang.cmd() {
            let client = self
                .clients
                .entry(key)
                .or_insert_with(|| LspClient::new(lang.clone(), root_path.clone(), cmd).unwrap());
            Some(client)
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub struct LspClient {
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
    InlayHints {
        uri: Url,
    },
}

#[derive(Debug)]
pub enum LspOutput {
    Completion(Vec<LspCompletion>),
    CompletionResolve(LspCompletion),
    InlayHints,
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
    fn new(lang: LspLang, root_path: Url, cmd: Command) -> anyhow::Result<LspClient> {
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

        let lang_clone = lang.clone();
        tokio::spawn(async move {
            send_request_async_with_id::<_, lsp_types::request::Initialize>(&mut stdin, 0, init)
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
                let r = Self::process_input(&lang_clone, &mut stdin, lsp_input).await;
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
                    if let Id::Num(id) = suc.id {
                        if id == 0 {
                            init_tx.send(())?;
                        } else {
                            let request = {
                                let mut lsp = lock!(mut lsp);
                                lsp.get_request(id).unwrap()
                            };
                            match request.method.as_str() {
                                lsp_types::request::Completion::METHOD => {
                                    let completion =
                                        serde_json::from_value::<lsp_types::CompletionResponse>(
                                            suc.result,
                                        )?;
                                    let completions = match completion {
                                        CompletionResponse::Array(arr) => convert_completions(arr),
                                        CompletionResponse::List(list) => {
                                            convert_completions(list.items)
                                        }
                                    };
                                    tx.send(LspOutput::Completion(completions))?;
                                }
                                lsp_types::request::ResolveCompletionItem::METHOD => {
                                    let item: CompletionItem = serde_json::from_value(suc.result)?;
                                    tx.send(LspOutput::CompletionResolve(
                                        convert_completion(item).unwrap(),
                                    ))?;
                                }
                                lsp_ext::InlayHints::METHOD => {
                                    let item: Vec<InlayHint> = serde_json::from_value(suc.result)?;
                                    process_inlay_hints(request.uri, item);
                                    tx.send(LspOutput::InlayHints)?;
                                }
                                _ => {}
                            }
                        }
                    }
                } else if let Ok(notification) = notification {
                    if let Some(method) = notification.get("method") {
                        if method == "textDocument/publishDiagnostics" {
                            let params: PublishDiagnosticsParams =
                                serde_json::from_value(notification.get("params").unwrap().clone())
                                    .unwrap();
                            let diagnostics = params.diagnostics;
                            process_diagnostics(params.uri.clone(), diagnostics);
                            tx.send(LspOutput::Diagnostics)?;
                        } else {
                            println!("{} {:?}", method, notification);
                        }
                    } else {
                        println!("{:?}", notification);
                    }
                } else {
                    println!("fail : {}", msg);
                }
            }
        });

        Ok(Self {
            output_channel: rx,
            input_channel: c_tx,
        })
    }

    async fn process_input(
        lang: &LspLang,
        mut stdin: &mut ChildStdin,
        lsp_input: LspInput,
    ) -> anyhow::Result<()> {
        match lsp_input {
            LspInput::RequestCompletion {
                row,
                col,
                buffer_id,
            } => {
                let url = notify_did_change(&mut stdin, buffer_id).await.unwrap();
                request_completion(&mut stdin, row, col, url).await;
            }
            LspInput::RequestCompletionResolve { item, .. } => {
                request_resolve_completion_item(&mut stdin, item)
                    .await
                    .unwrap();
            }
            LspInput::OpenFile { uri: url, content } => {
                notify_did_open(&mut stdin, url.clone(), content)
                    .await
                    .unwrap();
                request_inlay_hints(&mut stdin, url).await.unwrap();
            }
            LspInput::CloseFile { uri } => {
                notify_did_close(&mut stdin, uri).await.unwrap();
            }
            LspInput::SavedFile { uri, content } => {
                let id = {
                    let buffers = lock!(buffers);
                    buffers
                        .get_by_uri(uri.clone())
                        .context("buffer not found")?
                        .id
                };
                notify_did_change(&mut stdin, id).await.unwrap();
                notify_did_save(&mut stdin, uri.clone(), content)
                    .await
                    .unwrap();
                if let LspLang::Rust = lang {
                    request_inlay_hints(&mut stdin, uri).await.unwrap();
                }
            }
            LspInput::InlayHints { uri } => {
                if let LspLang::Rust = lang {
                    request_inlay_hints(&mut stdin, uri).await.unwrap();
                }
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

fn process_inlay_hints(uri: Url, hints: Vec<InlayHint>) {
    let mut buffers = lock!(mut buffers);
    let buf = buffers.get_by_uri_mut(uri);

    if let Some(buf) = buf {
        buf.buffer.inlay_hints.clear();
        for hint in hints {
            let pos = match &hint.kind {
                InlayKind::TypeHint => hint.range.end,
                InlayKind::ParameterHint => hint.range.start,
                InlayKind::ChainingHint => hint.range.end,
            };
            let idx = (&pos).into_with_buf(&buf.buffer);
            buf.buffer.inlay_hints.push((idx, hint));
        }
    }
}

fn convert_completions(mut input: Vec<CompletionItem>) -> Vec<LspCompletion> {
    input
        .drain(..)
        .filter_map(|c| convert_completion(c))
        .collect()
}

async fn request_completion(mut stdin: &mut &mut ChildStdin, row: u32, col: u32, uri: Url) {
    let completion = lsp_types::CompletionParams {
        text_document_position: lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: uri.clone() },
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
    send_request_async::<_, lsp_types::request::Completion>(&mut stdin, uri, completion)
        .await
        .unwrap();
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
        if let Some(additional_edits) = c.additional_text_edits {
            for edit in additional_edits {
                edits.push(TextEdit {
                    range: edit.range,
                    new_text: edit.new_text,
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

async fn notify_did_change(mut stdin: &mut &mut ChildStdin, buffer_id: u32) -> anyhow::Result<Url> {
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
    send_notify_async::<_, lsp_types::notification::DidChangeTextDocument>(&mut stdin, edits)
        .await?;
    Ok(url)
}

async fn send_request_async_with_id<
    T: AsyncWrite + std::marker::Unpin,
    R: lsp_types::request::Request,
>(
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

async fn send_request_async<T: AsyncWrite + std::marker::Unpin, R: lsp_types::request::Request>(
    t: &mut T,
    uri: Url,
    params: R::Params,
) -> anyhow::Result<()>
where
    R::Params: serde::Serialize,
{
    let id = {
        let mut lsp = lock!(mut lsp);
        let id = lsp.new_request(R::METHOD.into(), uri);
        id
    };
    send_request_async_with_id::<_, R>(t, id, params).await
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
        Url::parse("none://none")?,
        item,
    )
    .await
}

// lsp inlay hint request
async fn request_inlay_hints<T: AsyncWrite + std::marker::Unpin>(
    stdin: &mut T,
    uri: Url,
) -> anyhow::Result<()> {
    let params = lsp_ext::InlayHintsParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
    };
    send_request_async::<_, lsp_ext::InlayHints>(stdin, uri, params).await
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

        let buf = buffers.get_by_uri_mut(uri);
        if let Some(buf) = buf {
            if !cleared.contains(&buf.id) {
                buf.buffer.diagnostics.0.clear();
                cleared.push(buf.id);
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
