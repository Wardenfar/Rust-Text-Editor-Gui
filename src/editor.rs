use std::cmp::{max, min};
use std::io::Cursor;

use druid::kurbo::Line;
use druid::piet::{
    D2DTextLayout, D2DTextLayoutBuilder, Text, TextAttribute, TextLayout, TextLayoutBuilder,
};
use druid::{
    BoxConstraints, Code, Color, Env, Event, EventCtx, FontDescriptor, FontStyle, LayoutCtx,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, RenderContext, Size, UpdateCtx, Widget,
};
use ropey::RopeSlice;

use crate::buffer::{Action, Buffer, Movement};
use crate::highlight::{Highlight, Region, TreeSitterHighlight};
use crate::{AppState, FontWeight, EDITOR_FONT, THEME};

pub struct TextEditor {
    buffer: Buffer,
    layouts: Vec<D2DTextLayout>,
    regions: Vec<Region>,
    highlight: TreeSitterHighlight,
}

impl TextEditor {
    pub fn new() -> Self {
        let buffer = Buffer::from_reader(Cursor::new(include_str!("../file.json")));

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

    pub fn calculate_highlight(&mut self) {
        let regions = self
            .highlight
            .parse(self.buffer.rope().slice(..).as_str().unwrap().as_bytes());
        self.regions = regions;
    }
}

impl Widget<AppState> for TextEditor {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, _data: &mut AppState, _env: &Env) {
        match event {
            Event::KeyDown(key) => {
                let dirty = match &key.code {
                    Code::Escape => {
                        println!("{:?}", self.buffer.text());
                        false
                    }
                    Code::ArrowDown => self.buffer.move_cursor(Movement::Down),
                    Code::ArrowLeft => self.buffer.move_cursor(Movement::Left),
                    Code::ArrowRight => self.buffer.move_cursor(Movement::Right),
                    Code::ArrowUp => self.buffer.move_cursor(Movement::Up),
                    Code::Backspace => self.buffer.do_action(Action::Backspace),
                    Code::Delete => self.buffer.do_action(Action::Delete),
                    Code::Enter => self.buffer.do_action(Action::Insert("\n".into())),
                    _ => {
                        let code = key.key.legacy_charcode();
                        if code == 0 {
                            false
                        } else {
                            let char = char::from_u32(code);
                            if let Some(char) = char {
                                self.buffer.do_action(Action::Insert(String::from(char)));
                                true
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
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &AppState,
        _env: &Env,
    ) {
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
        let rect = ctx.size().to_rect();
        let bg = THEME.scope("ui.background").bg();
        ctx.fill(rect, &bg);

        ctx.save();
        ctx.clip(rect);

        let cursor = self.buffer.cursor();
        self.layouts = vec![];
        let rope = self.buffer.rope();
        let mut y = 0.0;
        for line in 0..rope.len_lines() {
            let bounds = self.buffer.line_bounds(line);
            let slice = rope.slice(bounds.0..bounds.1);

            let byte_start = rope.char_to_byte(bounds.0);
            let byte_end = rope.char_to_byte(bounds.1);

            let mut builder = text_layout(ctx, env, slice);

            for r in &self.regions {
                let start = max(byte_start, r.start_byte);
                let end = min(byte_end, r.end_byte);
                if start < end {
                    let start_char = rope.byte_to_char(start - byte_start);
                    let end_char = rope.byte_to_char(end - byte_start);

                    let range = start_char..end_char;

                    builder = builder
                        .range_attribute(range.clone(), TextAttribute::TextColor(r.style.fg()));

                    if r.style.bold() {
                        builder = builder.range_attribute(
                            range.clone(),
                            TextAttribute::Weight(FontWeight::BOLD),
                        );
                    }
                    if r.style.italic() {
                        builder = builder.range_attribute(
                            range.clone(),
                            TextAttribute::Style(FontStyle::Italic),
                        );
                    }
                    if r.style.underline() {
                        builder =
                            builder.range_attribute(range.clone(), TextAttribute::Underline(true));
                    }
                }
            }

            let layout = builder.build().unwrap();

            let slice_without_trailing = slice.as_str().unwrap().trim_end().len();

            let char_width = (layout.size().width / slice_without_trailing as f64);

            ctx.draw_text(&layout, Point::new(0.0, y));

            if bounds.0 <= cursor && cursor <= bounds.1 {
                let x = char_width * (cursor - bounds.0) as f64;
                let line = Line::new(
                    Point::new(x, y),
                    Point::new(x, y + layout.size().height + 4.0),
                );
                ctx.stroke(line, &Color::RED, 1.0);
            }

            y += layout.size().height + 4.0;
            self.layouts.push(layout);
        }

        ctx.restore().unwrap()
    }
}

fn text_layout(ctx: &mut PaintCtx, env: &Env, text: RopeSlice) -> D2DTextLayoutBuilder {
    let font: FontDescriptor = env.get(EDITOR_FONT);
    ctx.text()
        .new_text_layout(text.as_str().unwrap().to_string())
        .text_color(Color::WHITE)
        .font(font.family, font.size)
}
