use lsp_types::request::Request;
use lsp_types::{Range, TextDocumentIdentifier};
use serde::{Deserialize, Serialize};

pub enum InlayHints {}

impl Request for InlayHints {
    type Params = InlayHintsParams;
    type Result = Vec<InlayHint>;
    const METHOD: &'static str = "rust-analyzer/inlayHints";
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InlayHintsParams {
    pub text_document: TextDocumentIdentifier,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum InlayKind {
    TypeHint,
    ParameterHint,
    ChainingHint,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct InlayHint {
    pub range: Range,
    pub kind: InlayKind,
    pub label: String,
}
