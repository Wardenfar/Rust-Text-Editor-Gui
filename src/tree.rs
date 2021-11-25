use crate::editor::{text_layout, HALF_LINE_SPACING, LINE_SPACING};
use crate::{AppState, THEME};
use druid::piet::TextLayout;
use druid::{
    BoxConstraints, Env, Event, EventCtx, KbKey, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx,
    Point, Rect, RenderContext, Size, UpdateCtx, Widget,
};

pub type ShouldRepaint = bool;

pub trait Tree {
    type Key: Clone + PartialEq;
    fn root(&self, data: &AppState) -> Self::Key;
    fn children(&self, data: &AppState, parent: &Self::Key) -> Vec<Self::Key>;
    fn refresh(&self, data: &AppState, parent: &Self::Key);
    fn item(&self, data: &AppState, key: &Self::Key) -> ItemStyle;
    fn key_down(&mut self, data: &mut AppState, selected: &Self::Key, key: &KbKey)
        -> ShouldRepaint;
}

pub struct ItemStyle {
    pub(crate) text: String,
    pub(crate) style_scope: String,
    pub(crate) level: usize,
}

pub struct TreeViewer<T: Tree> {
    tree: T,
    scroll: usize,
    selected: Option<T::Key>,
    items: Vec<T::Key>,
    opened: Vec<T::Key>,
}

impl<T: Tree> TreeViewer<T> {
    pub fn new(tree: T) -> Self {
        TreeViewer {
            tree,
            scroll: 0,
            selected: None,
            items: vec![],
            opened: vec![],
        }
    }
}

impl<T: Tree> Widget<AppState> for TreeViewer<T> {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut AppState, _env: &Env) {
        if let Event::KeyDown(e) = event {
            match &e.key {
                KbKey::Character(s) => match s.as_str() {
                    " " => {
                        if self.selected.is_some() {
                            let selected = self.selected.as_ref().unwrap().clone();
                            let index = self.opened.iter().position(|x| *x == selected);
                            if let Some(index) = index {
                                self.opened.remove(index);
                            } else {
                                self.opened.push(selected);
                            }
                            ctx.request_paint();
                        }
                    }
                    _ => {}
                },
                KbKey::ArrowDown => {
                    if self.selected.is_some() {
                        let selected = self.selected.as_ref().unwrap().clone();
                        let index = self.items.iter().position(|x| *x == selected);
                        if let Some(index) = index {
                            let next = self.items.get(index.saturating_add(1));
                            if next.is_some() {
                                let next = next.unwrap().clone();
                                self.selected = Some(next);
                                ctx.request_paint();
                            }
                        }
                    }
                }
                KbKey::ArrowUp => {
                    if self.selected.is_some() {
                        let selected = self.selected.as_ref().unwrap().clone();
                        let index = self.items.iter().position(|x| *x == selected);
                        if let Some(index) = index {
                            let prev = self.items.get(index.saturating_sub(1));
                            if prev.is_some() {
                                let prev = prev.unwrap().clone();
                                self.selected = Some(prev);
                                ctx.request_paint();
                            }
                        }
                    }
                }
                key => {
                    if self.selected.is_some() {
                        let selected = self.selected.as_ref().unwrap();
                        let repaint = self.tree.key_down(data, selected, key);
                        if repaint {
                            ctx.request_paint();
                        }
                    }
                }
            }
        }

        ctx.request_focus()
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
                self.selected = Some(self.tree.root(data));
            }
            _ => {}
        }
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

    fn paint(&mut self, ctx: &mut PaintCtx, data: &AppState, env: &Env) {
        let rect = ctx.size().to_rect();
        ctx.save().unwrap();
        ctx.clip(rect.clone());
        ctx.fill(rect, &THEME.scope("ui.background").bg());

        let root = self.tree.root(data);
        let items = self.displayed(data, &root);

        let mut y = HALF_LINE_SPACING;

        for key in items.iter().skip(self.scroll) {
            let item = self.tree.item(data, key);

            let mut style = THEME.scope(&item.style_scope);
            let mut bg = None;
            if let Some(selected) = &self.selected {
                if key == selected {
                    style = THEME.scope("tree.selected");
                    bg = Some(style.bg());
                }
            }

            let layout = text_layout(ctx, env, &item.text, &style);

            if let Some(bg) = bg {
                ctx.fill(
                    Rect::new(
                        0.0,
                        y,
                        rect.width(),
                        y + layout.size().height + HALF_LINE_SPACING,
                    ),
                    &bg,
                );
            }

            let x = item.level as f64 * 20.0;
            ctx.draw_text(&layout, Point::new(x, y));
            if y > ctx.size().height {
                break;
            }
            y += layout.size().height + LINE_SPACING;
        }

        ctx.restore().unwrap();

        self.items = items;
    }
}

impl<T: Tree> TreeViewer<T> {
    fn displayed(&self, data: &AppState, curr: &T::Key) -> Vec<T::Key> {
        let mut result = Vec::new();
        result.push(curr.clone());
        if !self.opened.contains(curr) {
            return result;
        }
        for c in self.tree.children(data, &curr) {
            result.extend(self.displayed(data, &c));
        }
        result
    }
}
