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
use crate::{AppState, EDITOR_FONT, FONT};

pub struct TextEditor {
    buffer: Buffer,
    layouts: Vec<D2DTextLayout>,
}

impl TextEditor {
    pub fn new() -> Self {
        Self {
            buffer: Buffer::from_reader(Cursor::new(include_str!("../file.json"))),
            layouts: vec![],
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
        let cursor = self.buffer.cursor();
        println!("{}", cursor);
        self.layouts = vec![];
        let rope = self.buffer.rope();
        let mut y = 0.0;
        for line in 0..rope.len_lines() {
            let bounds = self.buffer.line_bounds(line);
            let mut builder = text_layout(ctx, env, rope.slice(bounds.0..bounds.1));
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
