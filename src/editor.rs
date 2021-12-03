use std::cmp::{max, min};
use std::collections::HashMap;
use std::time::Duration;

use anyhow::Context;
use druid::kurbo::Line;
use druid::piet::*;
use druid::*;
use itertools::Itertools;
use ropey::RopeSlice;

use crate::buffer::{Action, Bounds, Handle, Index, IntoWithBuffer, Movement};
use crate::draw::{drawable_text, Drawable, DrawableText};
use crate::highlight::TreeSitterHighlight;
use crate::lsp::{lsp_send, lsp_try_recv, CompletionData, LspInput, LspOutput};
use crate::style_layer::{style_for_range, DiagStyleLayer, Span, StyleLayer};
use crate::theme::Style;
use crate::{curr_buf, lock, AppState, BufferSource, Path, THEME};

pub const LINE_SPACING: f64 = 4.0;
pub const SCROLL_GAP: usize = 4;
pub const HALF_LINE_SPACING: f64 = LINE_SPACING / 2.0;
pub const DEFAULT_BACKGROUND_COLOR: Color = Color::rgb8(0x2f, 0x2f, 0x2f);
pub const DEFAULT_FOREGROUND_COLOR: Color = Color::rgb8(0xcc, 0xcc, 0xcc);
pub const DEFAULT_TEXT_SIZE: f64 = 18.0;
lazy_static::lazy_static! {
    pub static ref DEFAULT_TEXT_FONT: String = String::from("Fira Code");
}

pub struct TextEditor {
    last_buffer_id: Option<u32>,
    char_points: Vec<(Point, Index)>,
    highlight: Option<TreeSitterHighlight>,
    highlight_spans: Vec<Span>,
    scroll_line: usize,
    last_line_painted: usize,
}

impl TextEditor {
    fn do_action(&mut self, action: Action, _data: &mut AppState) -> anyhow::Result<bool> {
        let (action, id) = {
            let mut buffers = lock!(mut buffers);
            let buf = buffers.get_mut_curr()?;
            (buf.buffer.do_action(action), buffers.curr()?)
        };
        if let Some(action) = action {
            lsp_send(id, action)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn fix_scroll(&mut self) -> anyhow::Result<()> {
        let buffers = lock!(buffers);
        let buf = buffers.get(buffers.curr()?)?;
        let cursor_row = buf.buffer.row();
        if buf.buffer.rope().len_lines() <= SCROLL_GAP * 2 {
            self.scroll_line = 0;
        } else if cursor_row.saturating_sub(SCROLL_GAP) < self.scroll_line {
            self.scroll_line = cursor_row.saturating_sub(SCROLL_GAP)
        } else if cursor_row.saturating_add(SCROLL_GAP) > self.last_line_painted {
            self.scroll_line = cursor_row
                .saturating_add(SCROLL_GAP)
                .saturating_sub(self.last_line_painted.saturating_sub(self.scroll_line))
        }
        Ok(())
    }

    fn recv_lsp_event(&mut self, ctx: &mut EventCtx) -> anyhow::Result<()> {
        let id = curr_buf!(id);
        let evt = lsp_try_recv(id)?;

        match evt {
            LspOutput::Completion(completions) => {
                let mut buffers = lock!(mut buffers);
                let buf = buffers.get_mut_curr()?;
                buf.buffer.completions = completions;
                ctx.request_paint();
            }
            LspOutput::CompletionResolve(c) => {
                match c.data {
                    CompletionData::Simple(text) => {
                        let mut buffers = lock!(mut buffers);
                        let buf = buffers.get_mut_curr()?;
                        buf.buffer.insert(buf.buffer.cursor().head, &text);
                    }
                    CompletionData::Edits(edits) => {
                        let mut buffers = lock!(mut buffers);
                        let buf = buffers.get_mut_curr()?;
                        edits
                            .iter()
                            .sorted_by_key(|e| {
                                let bounds: Bounds = (&e.range).into_with_buf(&buf.buffer);
                                bounds.0
                            })
                            .rev()
                            .for_each(|e| {
                                buf.buffer.remove_chars(&e.range);
                                buf.buffer.insert(&e.range.start, &e.new_text);
                            });
                    }
                };
                self.calculate_highlight()?;
                ctx.request_paint();
            }
            LspOutput::Diagnostics => {
                ctx.request_paint();
            }
            LspOutput::InlayHints => {
                ctx.request_paint();
            }
        }
        Ok(())
    }

    fn process(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut AppState,
    ) -> anyhow::Result<()> {
        let id = curr_buf!(id);
        let old = self.last_buffer_id.replace(id);
        if let Some(old) = old {
            if id != old {
                self.highlight = TreeSitterHighlight::new(curr_buf!(lang));
                self.calculate_highlight()?;
                ctx.request_paint();
            }
        } else {
            self.highlight = TreeSitterHighlight::new(curr_buf!(lang));
            self.calculate_highlight()?;
            ctx.request_paint();
        }

        match event {
            Event::Timer(_timer) => {
                self.recv_lsp_event(ctx).err().map(|_ignore| {});
                ctx.request_timer(Duration::from_millis(250));
            }
            Event::KeyDown(key) => {
                let is_shift = key.mods.shift();
                let dirty = match &key.code {
                    Code::Space if key.mods.ctrl() => {
                        let id = curr_buf!(id);
                        let row = curr_buf!(row);
                        let col = curr_buf!(col);
                        lsp_send(
                            id,
                            LspInput::RequestCompletion {
                                buffer_id: id,
                                row: row as u32,
                                col: col as u32,
                            },
                        )?;
                        false
                    }
                    Code::F1 => {
                        let c = {
                            let buffers = lock!(buffers);
                            let buf = buffers.get_curr()?;
                            buf.buffer.sorted_completions()?.first().cloned().cloned()
                        };
                        let id = curr_buf!(id);
                        if let Some(c) = c {
                            lsp_send(
                                id,
                                LspInput::RequestCompletionResolve {
                                    buffer_id: id,
                                    item: c.original_item,
                                },
                            )?;
                            true
                        } else {
                            false
                        }
                    }
                    Code::ArrowDown => {
                        let mut buffers = lock!(mut buffers);
                        buffers
                            .get_mut_curr()?
                            .buffer
                            .move_cursor(Movement::Down, is_shift)
                    }
                    Code::ArrowLeft => {
                        let mut buffers = lock!(mut buffers);
                        buffers
                            .get_mut_curr()?
                            .buffer
                            .move_cursor(Movement::Left, is_shift)
                    }
                    Code::ArrowRight => {
                        let mut buffers = lock!(mut buffers);
                        buffers
                            .get_mut_curr()?
                            .buffer
                            .move_cursor(Movement::Right, is_shift)
                    }
                    Code::ArrowUp => {
                        let mut buffers = lock!(mut buffers);
                        buffers
                            .get_mut_curr()?
                            .buffer
                            .move_cursor(Movement::Up, is_shift)
                    }
                    Code::Backspace => self.do_action(Action::Backspace, data)?,
                    Code::Delete => self.do_action(Action::Delete, data)?,
                    Code::Enter => self.do_action(Action::Insert("\n".into()), data)?,
                    Code::KeyS if key.mods.ctrl() => {
                        let uri = curr_buf!(uri);

                        if let Some(uri) = uri {
                            let id = curr_buf!(id);
                            let buffers = lock!(buffers);
                            // get buffer rope
                            let buf = buffers.get_curr()?;
                            let rope = buf.buffer.rope();
                            // if buffer source is a file
                            if let BufferSource::File { path } = &buf.source {
                                rope.write_to(path.writer())?;
                                lsp_send(
                                    id,
                                    LspInput::SavedFile {
                                        uri,
                                        content: buf.buffer.text(),
                                    },
                                )?;
                            }
                        }

                        false
                    }
                    _ => {
                        let code = key.key.legacy_charcode();
                        if code == 0 {
                            false
                        } else {
                            let char = char::from_u32(code);
                            if let Some(char) = char {
                                self.do_action(Action::Insert(String::from(char)), data)?
                            } else {
                                false
                            }
                        }
                    }
                };
                if dirty {
                    self.calculate_highlight()?;
                }
                self.fix_scroll()?;
                ctx.request_paint();
            }
            Event::Wheel(e) => {
                if e.wheel_delta.y < 0.0 {
                    self.scroll(-3)?;
                    ctx.request_paint();
                }
                if e.wheel_delta.y > 0.0 {
                    self.scroll(3)?;
                    ctx.request_paint();
                }
            }
            Event::MouseDown(e) => {
                if e.button.is_left() {
                    let found = self
                        .char_points
                        .iter()
                        .sorted_by_key(|(p, _)| p.distance(e.pos.clone()) as i64)
                        .next()
                        .map(|(_, idx)| idx.clone());
                    if let Some(idx) = found {
                        {
                            let mut buffers = lock!(mut buffers);
                            buffers
                                .get_mut_curr()?
                                .buffer
                                .move_cursor(Movement::Index(idx), e.mods.shift());
                        }
                        self.fix_scroll()?;
                        ctx.request_paint()
                    }
                }
                ctx.request_focus();
            }
            _ => {}
        }
        Ok(())
    }

    fn _paint(&mut self, ctx: &mut PaintCtx, env: &Env) -> anyhow::Result<()> {
        let rect = ctx.size().to_rect();
        let bg = THEME
            .scope("ui.background")
            .background
            .unwrap_or(DEFAULT_BACKGROUND_COLOR);
        ctx.fill(rect, &bg);

        let buffers = lock!(buffers);
        let buf = buffers.get(buffers.curr()?)?;

        let virtual_texts = buf.buffer.virtual_texts();

        ctx.save().unwrap();
        ctx.clip(rect);

        let rope = buf.buffer.rope();

        let cursor_row = buf.buffer.row();

        let mut line_numbers_texts = Vec::new();
        self.last_line_painted = 0;
        for n in self.scroll_line..rope.len_lines() {
            let style = if n == cursor_row {
                THEME.scope("ui.linenr.selected")
            } else {
                THEME.scope("ui.linenr")
            };
            let draw_text = drawable_text(ctx, env, &format!("{}", n + 1), &style);
            line_numbers_texts.push(draw_text);
        }

        if !line_numbers_texts.is_empty() {
            let linenr_max_width = line_numbers_texts
                .iter()
                .map(|dtext| dtext.width().floor() as i64)
                .max()
                .unwrap() as f64
                + LINE_SPACING * 4.0;

            ctx.stroke(
                Line::new(
                    Point::new(linenr_max_width, 0.0),
                    Point::new(linenr_max_width, rect.height()),
                ),
                &THEME
                    .scope("ui.popup")
                    .background
                    .unwrap_or(DEFAULT_BACKGROUND_COLOR),
                1.0,
            );

            let mut cursor_point = None;

            let cursor = buf.buffer.cursor().head;
            self.char_points = vec![];
            let mut y = HALF_LINE_SPACING;

            self.last_line_painted = 0;

            let mut spans_layers = vec![];
            spans_layers.push(self.highlight_spans.as_slice());
            let diags_layer = DiagStyleLayer().spans(buf, 0, rope.len_chars())?;
            spans_layers.push(&diags_layer);

            for (line_number_text, line) in
                line_numbers_texts.iter().zip((0..).skip(self.scroll_line))
            {
                let bounds = buf.buffer.line_bounds(line);

                let mut hints: HashMap<Index, DrawableText> = Default::default();
                for v in &virtual_texts {
                    if let Handle::Char(idx) = v.handle {
                        if idx >= bounds.0 && idx < bounds.1 {
                            let draw_text = drawable_text(ctx, env, &v.text, &v.style);
                            hints.insert(idx, draw_text);
                        }
                    }
                }

                let mut spans = style_for_range(
                    &spans_layers,
                    bounds.0,
                    bounds.1,
                    hints.keys().copied().collect(),
                )?;

                let mut draw_texts = spans
                    .iter()
                    .flat_map(|s| -> anyhow::Result<_> {
                        Ok(drawable_text(
                            ctx,
                            env,
                            &buf.buffer.text_slice(s.start..s.end)?,
                            &s.style,
                        ))
                    })
                    .collect::<Vec<_>>();

                for v in &virtual_texts {
                    if let Handle::LineEnd(line_idx) = v.handle {
                        if line_idx == line {
                            let draw_text = drawable_text(ctx, env, &v.text, &v.style);
                            draw_texts.push(draw_text);
                            spans.push(Span {
                                start: bounds.1,
                                end: bounds.1,
                                style: v.style.clone(),
                            })
                        }
                    }
                }

                let max_height = draw_texts
                    .iter()
                    .map(|l| l.height())
                    .max_by(|a, b| a.partial_cmp(b).unwrap())
                    .unwrap_or(line_number_text.height());

                line_number_text.draw(
                    ctx,
                    linenr_max_width - line_number_text.width() - LINE_SPACING * 2.0,
                    y,
                );

                let mut spans_with_texts = spans.into_iter().zip(draw_texts).collect_vec();

                for (idx, text) in hints {
                    let pos = spans_with_texts.iter().position(|(s, _)| s.start == idx);
                    let data = (
                        Span {
                            start: idx,
                            end: idx,
                            style: Style::default(),
                        },
                        text,
                    );
                    if let Some(pos) = pos {
                        spans_with_texts.insert(pos, data);
                    } else {
                        spans_with_texts.push(data);
                    }
                }

                let mut x = linenr_max_width + LINE_SPACING * 2.0;
                for (span, draw_text) in spans_with_texts {
                    let slice = rope.slice(span.start..span.end);
                    for idx in span.start..span.end {
                        if idx - span.start + 1 < slice.len_chars() {
                            let byte_start = slice.char_to_byte(idx - span.start);
                            let byte_end = slice.char_to_byte(idx - span.start + 1);
                            let rects = draw_text.text_layout.rects_for_range(byte_start..byte_end);
                            for r in rects {
                                let point = Point::new(r.x0 + x, y + (r.y0 + r.y1) / 2.0);
                                self.char_points.push((point, idx))
                            }
                        }
                    }

                    let sel_min =
                        max(span.start, buf.buffer.cursor().min()).saturating_sub(span.start);
                    let sel_max =
                        min(span.end, buf.buffer.cursor().max()).saturating_sub(span.start);

                    if sel_min < sel_max {
                        let rects = draw_text.text_layout.rects_for_range(sel_min..sel_max);
                        ctx.with_save(|ctx| {
                            ctx.transform(Affine::translate(Vec2::new(x, y)));
                            for mut r in rects {
                                r.y1 += LINE_SPACING;
                                ctx.fill(
                                    r,
                                    &THEME
                                        .scope("ui.selection")
                                        .background
                                        .unwrap_or(DEFAULT_BACKGROUND_COLOR),
                                )
                            }
                        });
                    }

                    draw_text.draw(ctx, x, y);

                    if span.start <= cursor && cursor <= span.end {
                        let char_idx = cursor - span.start;
                        let byte_idx = slice.char_to_byte(char_idx);
                        let hit = draw_text.text_layout.hit_test_text_position(byte_idx);
                        let curr_x = x + hit.point.x;
                        let line = Line::new(
                            Point::new(curr_x, y),
                            Point::new(curr_x, y + max_height + LINE_SPACING),
                        );
                        cursor_point = Some((curr_x, y + max_height + LINE_SPACING));
                        ctx.stroke(line, &Color::RED, 1.0);
                    }

                    x += draw_text.text_layout.trailing_whitespace_width();
                }

                y += max_height + LINE_SPACING;

                if y > rect.height() {
                    self.last_line_painted = line;
                    break;
                }
            }

            if self.last_line_painted == 0 {
                let draw_text = drawable_text(ctx, env, "[]", &Style::default());
                self.last_line_painted = ((rect.height() - y) / draw_text.height()).round()
                    as usize
                    + self.scroll_line
                    + line_numbers_texts.len();
            }

            let cursor_point = cursor_point.unwrap_or((0.0, 0.0));

            let text = buf
                .buffer
                .sorted_completions()
                .unwrap_or_else(|_| vec![])
                .iter()
                .take(8)
                .map(|c| &c.label)
                .join("\n");

            let draw_text = drawable_text(ctx, env, &text, &THEME.scope("ui.text"));

            let rect = Rect::new(
                cursor_point.0,
                cursor_point.1,
                cursor_point.0 + draw_text.width(),
                cursor_point.1 + draw_text.height(),
            );
            ctx.fill(
                rect,
                &THEME
                    .scope("ui.popup")
                    .background
                    .unwrap_or(DEFAULT_BACKGROUND_COLOR),
            );
            draw_text.draw(ctx, cursor_point.0, cursor_point.1);
        }
        ctx.restore().unwrap();
        Ok(())
    }
}

impl TextEditor {
    pub fn new() -> Self {
        Self {
            last_buffer_id: None,
            char_points: vec![],
            highlight: None,
            highlight_spans: vec![],
            scroll_line: 0,
            last_line_painted: 0,
        }
    }

    pub fn calculate_highlight(&mut self) -> anyhow::Result<()> {
        let highlight = self.highlight.as_mut().context("no highlight")?;
        let buffers = lock!(buffers);
        let buf = buffers.get_curr()?;
        let rope = buf.buffer.rope();
        let min = 0;
        let max = rope.len_chars();
        self.highlight_spans = highlight.spans(buf, min, max)?;
        Ok(())
    }

    fn scroll(&mut self, scroll: isize) -> anyhow::Result<()> {
        let buffers = lock!(buffers);
        let buf = buffers.get(buffers.curr()?)?;

        if scroll < 0 {
            self.scroll_line = self.scroll_line.saturating_sub(scroll.abs() as usize)
        }
        if scroll > 0 {
            self.scroll_line = self.scroll_line.saturating_add(scroll as usize)
        }

        self.scroll_line = min(self.scroll_line, buf.buffer.rope().len_lines() - 1);
        Ok(())
    }
}

impl Widget<AppState> for TextEditor {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut AppState, _env: &Env) {
        if let Err(e) = self.process(ctx, event, data) {
            println!("{}", e);
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &AppState,
        _env: &Env,
    ) {
        ctx.request_timer(Duration::from_millis(100));
    }

    fn update(&mut self, _ctx: &mut UpdateCtx, _old_data: &AppState, _data: &AppState, _env: &Env) {
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
        if let Err(e) = self._paint(ctx, env) {
            println!("failed to paint : {}", e)
        }
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
