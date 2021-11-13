use crate::theme::Style;
use crate::THEME;
use tree_sitter::{Language, Parser, Query, QueryCursor};

extern "C" {
    fn tree_sitter_json() -> Language;
}

fn json_lang() -> Parser {
    let mut parser = Parser::new();
    let language = unsafe { tree_sitter_json() };
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

impl TreeSitterHighlight {
    pub fn new() -> Self {
        let parser = json_lang();
        let query = Query::new(
            parser.language().unwrap(),
            include_str!("../runtime/queries/json/highlights.scm"),
        )
        .unwrap();
        Self { parser, query }
    }
}

impl Highlight for TreeSitterHighlight {
    fn parse(&mut self, input: &[u8]) -> Vec<Region> {
        let tree = self.parser.parse(input, None).unwrap();
        let mut cur = QueryCursor::new();
        let kind = self.query.capture_names();

        let mut regions = vec![];

        let matches = cur.matches(&self.query, tree.root_node(), input);
        for m in matches {
            let name = kind[m.pattern_index].as_str();

            println!("{}", kind[m.pattern_index]);
            for cap in m.captures {
                let start = cap.node.range().start_byte;
                let end = cap.node.range().end_byte;
                regions.push(Region {
                    index: m.pattern_index,
                    start_byte: start,
                    end_byte: end,
                    style: THEME.scope(name),
                })
            }
        }

        let regions = regions
            .iter()
            .filter(|r| {
                !regions.iter().any(|r_top| {
                    r_top.start_byte <= r.start_byte
                        && r_top.end_byte >= r.end_byte
                        && r_top.index < r.index
                })
            })
            .map(|r| r.clone())
            .collect::<Vec<_>>();

        println!("{:?}", regions);

        regions
    }
}
