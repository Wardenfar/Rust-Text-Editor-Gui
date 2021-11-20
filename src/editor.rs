use anyhow::Context;
use std::cmp::{max, min};
use std::fs::File;
use std::io::Cursor;
use std::path::PathBuf;

use druid::kurbo::Line;
use druid::piet::*;
use druid::*;
use itertools::Itertools;
use lsp_types::Url;
use ropey::RopeSlice;

use crate::buffer::{Action, Buffer, BufferSource, Movement};
use crate::fs::Path;
use crate::highlight::{Highlight, Region, TreeSitterHighlight};
use crate::lsp::{CompletionData, LspClient, LspCompletion, LspInput, LspLang, LspOutput};
use crate::theme::Style;
use crate::{AppState, FontFamily, FontWeight, LocalPath, LSP, THEME};

pub struct TextEditor {
    buffer: Buffer,
    layouts: Vec<D2DTextLayout>,
    regions: Vec<Region>,
    highlight: TreeSitterHighlight,
}

impl TextEditor {
    fn do_action(&mut self, action: Action, data: &mut AppState) -> bool {
        let lsp_input = self.buffer.do_action(action);
        if let Some(lsp_input) = lsp_input {
            let mut lsp = LSP.lock().unwrap();
            lsp.get(data.root_path.uri(), LspLang::Rust)
                .input_channel
                .send(lsp_input)
                .unwrap();
        }
        true
    }
}

impl TextEditor {
    pub fn new() -> Self {
        let buffer = Buffer::from_reader(Cursor::new("no file opened"), BufferSource::Text);

        let highlight = TreeSitterHighlight::new();
        let mut editor = Self {
            buffer,
            layouts: vec![],
            regions: vec![],
            highlight,
        };
        editor.calculate_highlight();
        editor
    }

    pub fn read_file(&mut self, root_path: Url, path: &LocalPath) {
        let uri = path.uri();

        self.buffer = Buffer::from_reader(path.reader(), BufferSource::File { uri: uri.clone() });

        self.calculate_highlight();

        let mut lsp = LSP.lock().unwrap();
        lsp.get(root_path, LspLang::Rust)
            .input_channel
            .send(LspInput::OpenFile {
                uri,
                content: self.buffer.text().into(),
            })
            .context("lsp: open file")
            .unwrap();
    }

    pub fn calculate_highlight(&mut self) {
        let regions = self
            .highlight
            .parse(self.buffer.rope().slice(..).as_str().unwrap().as_bytes());
        self.regions = regions;
    }

    fn build_parts<'a>(
        ctx: &mut PaintCtx,
        env: &Env,
        global_start: usize,
        slice: RopeSlice<'a>,
        cuts: &Vec<Cut>,
    ) -> Vec<TextPart<'a>> {
        let mut parts = Vec::new();

        for cut in cuts {
            let start = cut.start;
            let end = cut.end;

            let mut builder = text_layout(
                ctx,
                env,
                slice.slice(start..end).as_str().unwrap(),
                &cut.style,
            );

            let style = &cut.style;

            builder = builder.range_attribute(.., TextAttribute::TextColor(style.fg()));

            if style.bold() {
                builder = builder.range_attribute(.., TextAttribute::Weight(FontWeight::BOLD));
            }
            if style.italic() {
                builder = builder.range_attribute(.., TextAttribute::Style(FontStyle::Italic));
            }
            if style.underline() {
                builder = builder.range_attribute(.., TextAttribute::Underline(true));
            }

            let layout = builder.build().unwrap();

            parts.push(TextPart {
                layout,
                slice: slice.slice(start..end),
                start_char: global_start + cut.start,
                end_char: global_start + cut.end,
                style: style.clone(),
            });
        }
        parts
    }

    fn find_cuts(line_size: usize, regions: &Vec<Region>) -> Vec<Cut> {
        let mut cuts = Vec::new();

        let mut last_index = 0;
        for r in regions.iter().sorted_by_key(|r| r.start_byte) {
            if r.start_byte > last_index {
                cuts.push(Cut {
                    start: last_index,
                    end: r.start_byte,
                    style: Style::default(),
                });
            }
            cuts.push(Cut {
                start: r.start_byte,
                end: r.end_byte,
                style: r.style.clone(),
            });
            last_index = r.end_byte;
        }
        cuts.push(Cut {
            start: last_index,
            end: line_size,
            style: Style::default(),
        });
        cuts
    }
}

impl Widget<AppState> for TextEditor {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut AppState, _env: &Env) {
        match event {
            Event::KeyDown(key) => {
                let dirty = match &key.code {
                    Code::Space if key.mods.ctrl() => {
                        if let BufferSource::File { uri } = &self.buffer.source {
                            let mut lsp = LSP.lock().unwrap();
                            lsp.get(data.root_path.uri(), LspLang::Rust)
                                .input_channel
                                .send(LspInput::RequestCompletion {
                                    uri: uri.clone(),
                                    row: self.buffer.row() as u32,
                                    col: self.buffer.col() as u32,
                                })
                                .context("lsp request completion")
                                .unwrap();
                        }
                        false
                    }
                    Code::F1 => {
                        let cursor = self.buffer.cursor();
                        let c = self.buffer.completions.get(0).map(|c| c.clone());
                        if let Some(c) = c {
                            match c.data {
                                CompletionData::Simple(text) => {
                                    self.buffer.insert(cursor, &text);
                                }
                                CompletionData::Edit { range, new_text } => {
                                    let bounds = self.buffer.range_to_bounds(&range);
                                    self.buffer.remove_chars(bounds.0, bounds.1);
                                    self.buffer.insert(bounds.0, &new_text);
                                }
                            };
                            true
                        } else {
                            false
                        }
                    }
                    Code::Escape => {
                        println!("{:?}", self.buffer.text());
                        false
                    }
                    Code::ArrowDown => self.buffer.move_cursor(Movement::Down),
                    Code::ArrowLeft => self.buffer.move_cursor(Movement::Left),
                    Code::ArrowRight => self.buffer.move_cursor(Movement::Right),
                    Code::ArrowUp => self.buffer.move_cursor(Movement::Up),
                    Code::Backspace => self.do_action(Action::Backspace, data),
                    Code::Delete => self.do_action(Action::Delete, data),
                    Code::Enter => self.do_action(Action::Insert("\n".into()), data),
                    _ => {
                        let code = key.key.legacy_charcode();
                        if code == 0 {
                            false
                        } else {
                            let char = char::from_u32(code);
                            if let Some(char) = char {
                                self.do_action(Action::Insert(String::from(char)), data)
                            } else {
                                false
                            }
                        }
                    }
                };
                if dirty {
                    self.calculate_highlight();
                }
                ctx.request_paint()
            }
            Event::MouseDown(_) => ctx.request_focus(),
            _ => {}
        }

        let mut lsp = LSP.lock().unwrap();
        if let Ok(data) = lsp
            .get(data.root_path.uri(), LspLang::Rust)
            .output_channel
            .try_recv()
        {
            println!("{:?}", &data);
            match data {
                LspOutput::Completion(completions) => {
                    self.buffer.completions = completions;
                    ctx.request_paint();
                }
            }
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &AppState,
        _env: &Env,
    ) {
        match event {
            LifeCycle::WidgetAdded => {
                if let Some(path) = &data.file_path {
                    self.read_file(data.root_path.uri(), path);
                }
            }
            _ => {}
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &AppState, data: &AppState, _env: &Env) {
        if old_data.file_path != data.file_path {
            if let Some(path) = &data.file_path {
                self.read_file(data.root_path.uri(), path);
                ctx.request_paint();
            }
        }
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &AppState,
        _env: &Env,
    ) -> Size {
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, _data: &AppState, env: &Env) {
        let rect = ctx.size().to_rect();
        let bg = THEME.scope("ui.background").bg();
        ctx.fill(rect, &bg);

        ctx.save().unwrap();
        ctx.clip(rect);

        let mut cursor_point = None;

        let cursor = self.buffer.cursor();
        self.layouts = vec![];
        let rope = self.buffer.rope();
        let mut y = 0.0;
        for line in 0..rope.len_lines() {
            let bounds = self.buffer.line_bounds(line);
            let line_size = bounds.1 - bounds.0;
            let slice = rope.slice(bounds.0..bounds.1);

            let byte_start = rope.char_to_byte(bounds.0);
            let byte_end = rope.char_to_byte(bounds.1);

            let regions = self
                .regions
                .iter()
                .filter_map(|r| {
                    let start = max(byte_start, r.start_byte);
                    let end = min(byte_end, r.end_byte);
                    if start < end {
                        let start_char = rope.byte_to_char(start - byte_start);
                        let end_char = rope.byte_to_char(end - byte_start);
                        Some(Region {
                            index: r.index,
                            start_byte: start_char,
                            end_byte: end_char,
                            style: r.style.clone(),
                        })
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();

            let cuts = Self::find_cuts(line_size, &regions);
            let parts = Self::build_parts(ctx, env, bounds.0, slice, &cuts);

            let max_height = parts
                .iter()
                .map(|l| l.layout.size().height)
                .max_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap();

            let mut x = 0.0;
            for part in &parts {
                ctx.draw_text(&part.layout, Point::new(x, y));

                if part.start_char <= cursor && cursor <= part.end_char {
                    let hit = part.layout.hit_test_text_position(cursor - part.start_char);
                    let curr_x = x + hit.point.x;
                    let line = Line::new(
                        Point::new(curr_x, y),
                        Point::new(curr_x, y + max_height + 4.0),
                    );
                    cursor_point = Some((curr_x, y + max_height + 4.0));
                    ctx.stroke(line, &Color::RED, 1.0);
                }

                x += part.layout.trailing_whitespace_width();
            }

            y += max_height + 4.0;
        }

        let cursor_point = cursor_point.unwrap_or((0.0, 0.0));

        let text = self.buffer.completions.iter().map(|c| &c.label).join("\n");
        let layout = text_layout(ctx, env, &text, &Style::default())
            .build()
            .unwrap();
        let rect = Rect::new(
            cursor_point.0,
            cursor_point.1,
            cursor_point.0 + layout.size().width,
            cursor_point.1 + layout.size().height,
        );
        ctx.fill(rect, &Color::BLACK);
        ctx.draw_text(&layout, Point::new(cursor_point.0, cursor_point.1));

        ctx.restore().unwrap()
    }
}

pub struct TextPart<'a> {
    pub layout: D2DTextLayout,
    pub slice: RopeSlice<'a>,
    pub start_char: usize,
    pub end_char: usize,
    pub style: Style,
}

pub struct Cut {
    pub start: usize,
    pub end: usize,
    pub style: Style,
}

fn text_layout(ctx: &mut PaintCtx, _env: &Env, text: &str, style: &Style) -> D2DTextLayoutBuilder {
    ctx.text()
        .new_text_layout(text.to_string())
        .text_color(Color::WHITE)
        .font(
            FontFamily::new_unchecked(style.font_family()),
            style.font_size(),
        )
}
