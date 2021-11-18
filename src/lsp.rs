use std::collections::HashMap;
use std::io::Write;
use std::process;

use anyhow::Context;
use jsonrpc_core::Output;
use lsp_types::{
    CompletionItem, CompletionResponse, TextDocumentContentChangeEvent, Url,
    VersionedTextDocumentIdentifier,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

const ID_INIT: u64 = 0;
const ID_COMPLETION: u64 = 1;

#[derive(Debug)]
pub struct LspClient {
    process: tokio::process::Child,
    pub input_channel: mpsc::UnboundedSender<LspInput>,
    pub output_channel: mpsc::UnboundedReceiver<LspOutput>,
}

#[derive(Debug)]
pub enum LspInput {
    Edit {
        url: Url,
        version: i32,
        text: String,
    },
    RequestCompletion {
        url: Url,
        row: u32,
        col: u32,
    },
    OpenFile {
        url: Url,
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
    pub insert_text: String,
}

impl LspClient {
    pub fn new(cmd: String) -> anyhow::Result<LspClient> {
        let mut lsp = tokio::process::Command::new(&cmd)
            .stdin(process::Stdio::piped())
            .stdout(process::Stdio::piped())
            .stderr(process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        #[allow(deprecated)]
        let init = lsp_types::InitializeParams {
            process_id: Some(u32::from(process::id())),
            root_path: None,
            root_uri: None,
            initialization_options: None,
            capabilities: lsp_types::ClientCapabilities::default(),
            trace: None,
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
                .await?;
            // Wait initialize
            init_rx.recv().await.unwrap();

            let mut version = 0;
            let mut edited_text = String::new();

            while let Some(lsp_input) = c_rx.recv().await {
                match lsp_input {
                    LspInput::RequestCompletion { row, col, url } => {
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
                            context: None,
                        };
                        send_request_async::<_, lsp_types::request::Completion>(
                            &mut stdin,
                            ID_COMPLETION,
                            completion,
                        )
                        .await?;
                    }
                    LspInput::OpenFile { url, content } => {
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
                        .await?;
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
                    println!("{}", suc.result);
                    if suc.id == jsonrpc_core::id::Id::Num(ID_INIT) {
                        init_tx.send(())?;
                    } else if suc.id == jsonrpc_core::id::Id::Num(ID_COMPLETION) {
                        let completion =
                            serde_json::from_value::<lsp_types::CompletionResponse>(suc.result)?;
                        let completions = match completion {
                            CompletionResponse::Array(arr) => convert_completions(arr),
                            CompletionResponse::List(list) => convert_completions(list.items),
                        };
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
            c.insert_text.map(|insert_text| LspCompletion {
                label: c.label,
                insert_text,
            })
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
