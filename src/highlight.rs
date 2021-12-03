use crate::buffer::Index;
use crate::style_layer::{Span, StyleLayer};
use crate::theme::Style;
use crate::{lock, BufferData, LspLang, THEME};
use std::collections::HashMap;
use tree_sitter::{Language, Parser, Query, QueryCursor};

extern "C" {
    fn tree_sitter_json() -> Language;
    fn tree_sitter_python() -> Language;
    fn tree_sitter_rust() -> Language;
}

fn json_lang() -> Parser {
    let mut parser = Parser::new();
    let language = unsafe { tree_sitter_json() };
    parser.set_language(language).unwrap();
    parser
}

fn python_lang() -> Parser {
    let mut parser = Parser::new();
    let language = unsafe { tree_sitter_python() };
    parser.set_language(language).unwrap();
    parser
}

fn rust_lang() -> Parser {
    let mut parser = Parser::new();
    let language = unsafe { tree_sitter_rust() };
    parser.set_language(language).unwrap();
    parser
}

pub trait Highlight {
    fn parse(&mut self, input: &[u8]) -> Vec<Region>;
}

pub struct TreeSitterHighlight {
    parser: Parser,
    query: Query,
}

#[derive(Debug, Clone)]
pub struct Region {
    pub index: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub style: Style,
}

impl LspLang {
    pub fn tree_sitter_lang(&self) -> Option<(Parser, &str)> {
        match self {
            LspLang::Json => Some((
                json_lang(),
                include_str!("../runtime/queries/json/highlights.scm"),
            )),
            LspLang::Python => Some((
                python_lang(),
                include_str!("../runtime/queries/python/highlights.scm"),
            )),
            LspLang::Rust => Some((
                rust_lang(),
                include_str!("../runtime/queries/rust/highlights.scm"),
            )),
            _ => None,
        }
    }
}

impl TreeSitterHighlight {
    pub fn new(lang: LspLang) -> Option<Self> {
        let (parser, highlight) = lang.tree_sitter_lang()?;
        let query = Query::new(parser.language().unwrap(), highlight).unwrap();
        Some(Self { parser, query })
    }
}

impl StyleLayer for TreeSitterHighlight {
    fn spans(
        &mut self,
        buffer: &BufferData,
        _min: Index,
        _max: Index,
    ) -> anyhow::Result<Vec<Span>> {
        let text = buffer.buffer.text();
        let rope = buffer.buffer.rope();
        let tree = self.parser.parse(&text, None).unwrap();
        let mut cur = QueryCursor::new();

        let mut map = HashMap::new();
        for name in self.query.capture_names() {
            if let Some(index) = self.query.capture_index_for_name(name) {
                map.insert(index, name.clone());
            }
        }

        let mut spans = vec![];

        let matches = cur.matches(&self.query, tree.root_node(), text.as_bytes());
        for m in matches {
            let name = map.get(&(m.pattern_index as u32));
            if let Some(name) = name {
                for cap in m.captures {
                    let start_byte = cap.node.range().start_byte;
                    let end_byte = cap.node.range().end_byte;

                    let start = rope.byte_to_char(start_byte);
                    let end = rope.byte_to_char(end_byte);

                    spans.push((
                        m.pattern_index,
                        Span {
                            start,
                            end,
                            style: THEME.scope(name),
                        },
                    ))
                }
            }
        }

        spans.sort_unstable_by_key(|(i, _)| *i);
        spans.reverse();
        Ok(spans.into_iter().map(|(_, span)| span).collect())
    }
}
