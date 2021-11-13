use std::cmp::{max, min};
use std::io::Cursor;
use std::ops::RangeBounds;

use druid::kurbo::Line;
use druid::piet::{
    D2DTextLayout, D2DTextLayoutBuilder, Text, TextAttribute, TextLayout, TextLayoutBuilder,
};
use druid::text::Editor;
use druid::{
    BoxConstraints, Code, Color, Env, Event, EventCtx, FontDescriptor, KeyEvent, LayoutCtx,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, RenderContext, Size, TextAlignment, UpdateCtx,
    Widget,
};
use ropey::RopeSlice;

use crate::buffer::{Buffer, Movement};
use crate::highlight::{Highlight, Region, TreeSitterHighlight};
use crate::{AppState, EDITOR_FONT, FONT};

pub struct TextEditor {
    buffer: Buffer,
    layouts: Vec<D2DTextLayout>,
    regions: Vec<Region>,
}

impl TextEditor {
    pub fn new() -> Self {
        let buffer = Buffer::from_reader(Cursor::new(include_str!("../file.json")));

        let mut highlight = TreeSitterHighlight::new();
        let regions = highlight.parse(buffer.rope().slice(..).as_str().unwrap().as_bytes());
        Self {
            buffer,
            layouts: vec![],
            regions,
        }
    }
}

impl Widget<AppState> for TextEditor {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut AppState, env: &Env) {
        match event {
            Event::KeyDown(key) => {
                match key.code {
                    Code::ArrowDown => self.buffer.move_cursor(Movement::Down),
                    Code::ArrowLeft => self.buffer.move_cursor(Movement::Left),
                    Code::ArrowRight => self.buffer.move_cursor(Movement::Right),
                    Code::ArrowUp => self.buffer.move_cursor(Movement::Up),
                    _ => {}
                }
                ctx.request_paint()
            }
            Event::MouseDown(mouse) => ctx.request_focus(),
            _ => {}
        }
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, data: &AppState, env: &Env) {
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &AppState, data: &AppState, env: &Env) {}

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &AppState,
        env: &Env,
    ) -> Size {
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &AppState, env: &Env) {
        let rect = ctx.size().to_rect();
        ctx.fill(rect, &Color::BLACK);

        let cursor = self.buffer.cursor();
        println!("{}", cursor);
        self.layouts = vec![];
        let rope = self.buffer.rope();
        let mut y = 0.0;
        for line in 0..rope.len_lines() {
            let bounds = self.buffer.line_bounds(line);

            let byte_start = rope.char_to_byte(bounds.0);
            let byte_end = rope.char_to_byte(bounds.1);

            let mut builder = text_layout(ctx, env, rope.slice(bounds.0..bounds.1));

            for r in &self.regions {
                let start = max(byte_start, r.start_byte);
                let end = min(byte_end, r.end_byte);
                if start < end {
                    let start_char = rope.byte_to_char(start - byte_start);
                    let end_char = rope.byte_to_char(end - byte_start);
                    builder = builder.range_attribute(
                        start_char..end_char,
                        TextAttribute::TextColor(Color::rgb8(r.color.0, r.color.1, r.color.2)),
                    );
                }
            }

            let layout = builder.build().unwrap();

            ctx.draw_text(&layout, Point::new(0.0, y));

            if bounds.0 <= cursor && cursor < bounds.1 {
                let pos = layout.hit_test_text_position(cursor - bounds.0);
                let x = pos.point.x;
                let line = Line::new(
                    Point::new(x, y),
                    Point::new(x, y + layout.size().height + 4.0),
                );
                ctx.stroke(line, &Color::RED, 1.0);
            }
            if cursor == bounds.1 {
                let x = layout.size().width;
                let line = Line::new(
                    Point::new(x, y),
                    Point::new(x, y + layout.size().height + 4.0),
                );
                ctx.stroke(line, &Color::RED, 1.0);
            }

            y += layout.size().height + 4.0;
            self.layouts.push(layout);
        }
    }
}

fn text_layout(ctx: &mut PaintCtx, env: &Env, text: RopeSlice) -> D2DTextLayoutBuilder {
    let font: FontDescriptor = env.get(EDITOR_FONT);
    let mut builder = ctx
        .text()
        .new_text_layout(text.as_str().unwrap().to_string())
        .text_color(Color::WHITE)
        .font(font.family, font.size);
    builder
}
