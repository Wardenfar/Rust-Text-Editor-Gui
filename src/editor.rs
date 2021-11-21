use std::cmp::{max, min};
use std::io::Cursor;

use druid::kurbo::Line;
use druid::piet::*;
use druid::*;
use itertools::Itertools;
use lsp_types::Url;
use ropey::RopeSlice;

use crate::buffer::{Action, Bounds, Buffer, BufferSource, IntoWithBuffer, Movement};
use crate::fs::Path;
use crate::highlight::{Highlight, Region, TreeSitterHighlight};
use crate::lsp::{lsp_send, lsp_try_recv, CompletionData, LspInput, LspLang, LspOutput};
use crate::theme::Style;
use crate::{AppState, LocalPath, THEME};

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
            lsp_send(data.root_path.uri(), LspLang::Rust, lsp_input);
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

        lsp_send(
            root_path,
            LspLang::Rust,
            LspInput::OpenFile {
                uri,
                content: self.buffer.text().into(),
            },
        );
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
        global_start_char: usize,
        slice: RopeSlice<'a>,
        cuts: &Vec<Cut>,
    ) -> Vec<TextPart<'a>> {
        let mut parts = Vec::new();

        for cut in cuts {
            let start_byte = cut.start_byte;
            let end_byte = cut.end_byte;

            let start_char = slice.byte_to_char(start_byte);
            let end_char = slice.byte_to_char(end_byte);
            let line_slice = slice.slice(start_char..end_char);

            let mut builder = text_layout(ctx, env, line_slice.as_str().unwrap(), &cut.style);

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
                slice: line_slice,
                start_char: global_start_char + start_char,
                end_char: global_start_char + end_char,
                style: style.clone(),
            });
        }
        parts
    }

    fn find_cuts(line_bytes_len: usize, regions: &Vec<Region>) -> Vec<Cut> {
        let mut cuts = Vec::new();

        let mut last_index = 0;
        for r in regions.iter().sorted_by_key(|r| r.start_byte) {
            if r.start_byte > last_index {
                cuts.push(Cut {
                    start_byte: last_index,
                    end_byte: r.start_byte,
                    style: Style::default(),
                });
            }
            cuts.push(Cut {
                start_byte: r.start_byte,
                end_byte: r.end_byte,
                style: r.style.clone(),
            });
            last_index = r.end_byte;
        }
        cuts.push(Cut {
            start_byte: last_index,
            end_byte: line_bytes_len,
            style: Style::default(),
        });
        cuts
    }
}

impl Widget<AppState> for TextEditor {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut AppState, _env: &Env) {
        match event {
            Event::KeyDown(key) => {
                let is_shift = key.mods.shift();
                let dirty = match &key.code {
                    Code::Space if key.mods.ctrl() => {
                        if let BufferSource::File { uri } = &self.buffer.source {
                            lsp_send(
                                data.root_path.uri(),
                                LspLang::Rust,
                                LspInput::RequestCompletion {
                                    uri: uri.clone(),
                                    row: self.buffer.row() as u32,
                                    col: self.buffer.col() as u32,
                                },
                            );
                        }
                        false
                    }
                    Code::F1 => {
                        let c = self
                            .buffer
                            .sorted_completions()
                            .first()
                            .map(|c| (*c).clone());
                        if let Some(c) = c {
                            lsp_send(
                                data.root_path.uri(),
                                LspLang::Rust,
                                LspInput::RequestCompletionResolve {
                                    item: c.original_item,
                                },
                            );
                            true
                        } else {
                            false
                        }
                    }
                    Code::Escape => {
                        println!("{:?}", self.buffer.text());
                        false
                    }
                    Code::ArrowDown => self.buffer.move_cursor(Movement::Down, is_shift),
                    Code::ArrowLeft => self.buffer.move_cursor(Movement::Left, is_shift),
                    Code::ArrowRight => self.buffer.move_cursor(Movement::Right, is_shift),
                    Code::ArrowUp => self.buffer.move_cursor(Movement::Up, is_shift),
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

        if let Ok(data) = lsp_try_recv(data.root_path.uri(), LspLang::Rust) {
            match data {
                LspOutput::Completion(completions) => {
                    self.buffer.completions = completions;
                    ctx.request_paint();
                }
                LspOutput::CompletionResolve(c) => {
                    let cursor = self.buffer.cursor();
                    match &c.data {
                        CompletionData::Simple(text) => {
                            self.buffer.insert(cursor.head, &text);
                        }
                        CompletionData::Edits(edits) => {
                            edits
                                .iter()
                                .sorted_by_key(|e| {
                                    let bounds: Bounds = (&e.range).into_with_buf(&self.buffer);
                                    bounds.0
                                })
                                .rev()
                                .for_each(|e| {
                                    self.buffer.remove_chars(&e.range);
                                    self.buffer.insert(&e.range.start, &e.new_text);
                                });
                        }
                    };
                    self.calculate_highlight();
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

        let cursor = self.buffer.cursor().head;
        self.layouts = vec![];
        let rope = self.buffer.rope();
        let mut y = 0.0;
        for line in 0..rope.len_lines() {
            let bounds = self.buffer.line_bounds(line);
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
                        Some(Region {
                            index: r.index,
                            start_byte: start - byte_start,
                            end_byte: end - byte_start,
                            style: r.style.clone(),
                        })
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();

            let cuts = Self::find_cuts(byte_end - byte_start, &regions);
            let parts = Self::build_parts(ctx, env, bounds.0, slice, &cuts);

            let max_height = parts
                .iter()
                .map(|l| l.layout.size().height)
                .max_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap();

            let mut x = 0.0;
            for part in &parts {
                let sel_min = max(part.start_char, self.buffer.cursor().min())
                    .saturating_sub(part.start_char);
                let sel_max =
                    min(part.end_char, self.buffer.cursor().max()).saturating_sub(part.start_char);

                if sel_min < sel_max {
                    let rects = part.layout.rects_for_range(sel_min..sel_max);
                    ctx.with_save(|ctx| {
                        ctx.transform(Affine::translate(Vec2::new(x, y)));
                        for r in rects {
                            ctx.fill(r, &THEME.scope("ui.selection").bg())
                        }
                    });
                }

                ctx.draw_text(&part.layout, Point::new(x, y));

                if part.start_char <= cursor && cursor <= part.end_char {
                    let char_idx = cursor - part.start_char;
                    let byte_idx = part.slice.char_to_byte(char_idx);
                    let hit = part.layout.hit_test_text_position(byte_idx);
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

        let text = self
            .buffer
            .sorted_completions()
            .iter()
            .take(8)
            .map(|c| &c.label)
            .join("\n");

        let layout = text_layout(ctx, env, &text, &THEME.scope("ui.text"))
            .build()
            .unwrap();

        let rect = Rect::new(
            cursor_point.0,
            cursor_point.1,
            cursor_point.0 + layout.size().width,
            cursor_point.1 + layout.size().height,
        );
        ctx.fill(rect, &THEME.scope("ui.popup").bg());
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
    pub start_byte: usize,
    pub end_byte: usize,
    pub style: Style,
}

fn text_layout(ctx: &mut PaintCtx, _env: &Env, text: &str, style: &Style) -> D2DTextLayoutBuilder {
    ctx.text()
        .new_text_layout(text.to_string())
        .text_color(style.fg())
        .font(
            FontFamily::new_unchecked(style.font_family()),
            style.font_size(),
        )
}
