use std::cmp::{max, min};

use std::time::Duration;

use druid::kurbo::Line;
use druid::piet::*;
use druid::*;
use itertools::Itertools;

use ropey::RopeSlice;

use crate::buffer::{Action, Bounds, Index, IntoWithBuffer, Movement};

use crate::highlight::{Highlight, Region, TreeSitterHighlight};
use crate::lsp::{lsp_send, lsp_try_recv, CompletionData, LspInput, LspOutput};
use crate::theme::Style;
use crate::{curr_buf, lock, AppState, THEME};

pub const LINE_SPACING: f64 = 4.0;
pub const SCROLL_GAP: usize = 4;
pub const HALF_LINE_SPACING: f64 = LINE_SPACING / 2.0;

pub struct TextEditor {
    last_buffer_id: Option<u32>,
    char_points: Vec<(Point, Index)>,
    regions: Vec<Region>,
    highlight: TreeSitterHighlight,
    scroll_line: usize,
    last_line_painted: usize,
}

impl TextEditor {
    fn do_action(&mut self, action: Action, _data: &mut AppState) -> anyhow::Result<bool> {
        let (action, id) = {
            let mut buffers = lock!(buffers);
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
                let mut buffers = lock!(buffers);
                let buf = buffers.get_mut_curr()?;
                buf.buffer.completions = completions;
                ctx.request_paint();
            }
            LspOutput::CompletionResolve(c) => {
                match c.data {
                    CompletionData::Simple(text) => {
                        let mut buffers = lock!(buffers);
                        let buf = buffers.get_mut_curr()?;
                        buf.buffer.insert(buf.buffer.cursor().head, &text);
                    }
                    CompletionData::Edits(edits) => {
                        let mut buffers = lock!(buffers);
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
                self.calculate_highlight()?;
                ctx.request_paint();
            }
        } else {
            self.calculate_highlight()?;
            ctx.request_paint();
        }

        match event {
            Event::Timer(_timer) => {
                ctx.request_timer(Duration::from_millis(100));
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
                            buf.buffer.sorted_completions().first().cloned().cloned()
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
                        let mut buffers = lock!(buffers);
                        buffers
                            .get_mut_curr()?
                            .buffer
                            .move_cursor(Movement::Down, is_shift)
                    }
                    Code::ArrowLeft => {
                        let mut buffers = lock!(buffers);
                        buffers
                            .get_mut_curr()?
                            .buffer
                            .move_cursor(Movement::Left, is_shift)
                    }
                    Code::ArrowRight => {
                        let mut buffers = lock!(buffers);
                        buffers
                            .get_mut_curr()?
                            .buffer
                            .move_cursor(Movement::Right, is_shift)
                    }
                    Code::ArrowUp => {
                        let mut buffers = lock!(buffers);
                        buffers
                            .get_mut_curr()?
                            .buffer
                            .move_cursor(Movement::Up, is_shift)
                    }
                    Code::Backspace => self.do_action(Action::Backspace, data)?,
                    Code::Delete => self.do_action(Action::Delete, data)?,
                    Code::Enter => self.do_action(Action::Insert("\n".into()), data)?,
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
                            let mut buffers = lock!(buffers);
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
        let bg = THEME.scope("ui.background").bg();
        ctx.fill(rect, &bg);

        let buffers = lock!(buffers);
        let buf = buffers.get(buffers.curr()?)?;

        ctx.save().unwrap();
        ctx.clip(rect);

        let rope = buf.buffer.rope();

        let cursor_row = buf.buffer.row();

        let mut line_numbers_layouts = Vec::new();
        self.last_line_painted = 0;
        for n in self.scroll_line..rope.len_lines() {
            let style = if n == cursor_row {
                THEME.scope("ui.linenr.selected")
            } else {
                THEME.scope("ui.linenr")
            };
            let layout = text_layout(ctx, env, &format!("{}", n + 1), &style);
            line_numbers_layouts.push(layout);
        }

        if !line_numbers_layouts.is_empty() {
            let linenr_max_width = line_numbers_layouts
                .iter()
                .map(|l| l.size().width.floor() as i64)
                .max()
                .unwrap() as f64
                + LINE_SPACING * 4.0;

            ctx.stroke(
                Line::new(
                    Point::new(linenr_max_width, 0.0),
                    Point::new(linenr_max_width, rect.height()),
                ),
                &THEME.scope("ui.popup").bg(),
                1.0,
            );

            let mut cursor_point = None;

            let cursor = buf.buffer.cursor().head;
            self.char_points = vec![];
            let mut y = HALF_LINE_SPACING;

            self.last_line_painted = 0;

            for (line_number_layout, line) in line_numbers_layouts
                .iter()
                .zip((0..).skip(self.scroll_line))
            {
                let bounds = buf.buffer.line_bounds(line);
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

                ctx.draw_text(
                    line_number_layout,
                    Point::new(
                        linenr_max_width - line_number_layout.size().width - LINE_SPACING * 2.0,
                        y,
                    ),
                );

                let mut x = linenr_max_width + LINE_SPACING * 2.0;
                for part in &parts {
                    for idx in part.start_char..part.end_char {
                        let with_offset = idx - part.start_char;
                        let rects = part.layout.rects_for_range(with_offset..=with_offset);
                        for r in rects {
                            let point = Point::new(r.x0 + x, y + (r.y0 + r.y1) / 2.0);
                            self.char_points.push((point, idx))
                        }
                    }

                    let sel_min = max(part.start_char, buf.buffer.cursor().min())
                        .saturating_sub(part.start_char);
                    let sel_max = min(part.end_char, buf.buffer.cursor().max())
                        .saturating_sub(part.start_char);

                    if sel_min < sel_max {
                        let rects = part.layout.rects_for_range(sel_min..sel_max);
                        ctx.with_save(|ctx| {
                            ctx.transform(Affine::translate(Vec2::new(x, y)));
                            for mut r in rects {
                                r.y1 += LINE_SPACING;
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
                            Point::new(curr_x, y + max_height + LINE_SPACING),
                        );
                        cursor_point = Some((curr_x, y + max_height + LINE_SPACING));
                        ctx.stroke(line, &Color::RED, 1.0);
                    }

                    x += part.layout.trailing_whitespace_width();
                }

                y += max_height + LINE_SPACING;

                if y > rect.height() {
                    self.last_line_painted = line;
                    break;
                }
            }

            if self.last_line_painted == 0 {
                let l = text_layout(ctx, env, "[]", &Style::default());
                self.last_line_painted = ((rect.height() - y) / l.size().height).round() as usize
                    + self.scroll_line
                    + line_numbers_layouts.len();
            }

            let cursor_point = cursor_point.unwrap_or((0.0, 0.0));

            let text = buf
                .buffer
                .sorted_completions()
                .iter()
                .take(8)
                .map(|c| &c.label)
                .join("\n");

            let layout = text_layout(ctx, env, &text, &THEME.scope("ui.text"));

            let rect = Rect::new(
                cursor_point.0,
                cursor_point.1,
                cursor_point.0 + layout.size().width,
                cursor_point.1 + layout.size().height,
            );
            ctx.fill(rect, &THEME.scope("ui.popup").bg());
            ctx.draw_text(&layout, Point::new(cursor_point.0, cursor_point.1));
        }
        ctx.restore().unwrap();
        Ok(())
    }
}

impl TextEditor {
    pub fn new() -> Self {
        let highlight = TreeSitterHighlight::new();
        Self {
            last_buffer_id: None,
            char_points: vec![],
            regions: vec![],
            highlight,
            scroll_line: 0,
            last_line_painted: 0,
        }
    }

    pub fn calculate_highlight(&mut self) -> anyhow::Result<()> {
        let buffers = lock!(buffers);
        let buf = buffers.get(buffers.curr()?)?;
        let regions = self
            .highlight
            .parse(buf.buffer.rope().slice(..).as_str().unwrap().as_bytes());
        self.regions = regions;
        Ok(())
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

            let layout = text_layout(ctx, env, line_slice.as_str().unwrap(), &cut.style);

            parts.push(TextPart {
                layout,
                slice: line_slice,
                start_char: global_start_char + start_char,
                end_char: global_start_char + end_char,
                style: cut.style.clone(),
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

        while let Ok(_) = self.recv_lsp_event(ctx) {}
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

pub fn text_layout(ctx: &mut PaintCtx, _env: &Env, text: &str, style: &Style) -> D2DTextLayout {
    let mut builder = ctx
        .text()
        .new_text_layout(text.to_string())
        .text_color(style.fg())
        .font(
            FontFamily::new_unchecked(style.font_family()),
            style.font_size(),
        );

    if style.bold() {
        builder = builder.range_attribute(.., TextAttribute::Weight(FontWeight::BOLD));
    }
    if style.italic() {
        builder = builder.range_attribute(.., TextAttribute::Style(FontStyle::Italic));
    }
    if style.underline() {
        builder = builder.range_attribute(.., TextAttribute::Underline(true));
    }

    builder.build().unwrap()
}
