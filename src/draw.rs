use crate::editor::{DEFAULT_FOREGROUND_COLOR, DEFAULT_TEXT_FONT, DEFAULT_TEXT_SIZE};
use crate::theme::Style;
use crate::{lock, THEME};
use druid::piet::{Text, TextAttribute, TextLayout, TextLayoutBuilder};
use druid::{
    Affine, Color, Env, FontFamily, FontStyle, FontWeight, PaintCtx, Point, RenderContext, Vec2,
};

#[cfg(windows)]
pub use druid::piet::D2DTextLayout as ITextLayout;

#[cfg(unix)]
pub use druid::piet::CairoTextLayout as ITextLayout;

pub trait Drawable {
    fn draw(&self, ctx: &mut PaintCtx, x: f64, y: f64);
    fn width(&self) -> f64;
    fn height(&self) -> f64;
}

pub struct DrawableText {
    pub background_color: Option<Color>,
    pub text_layout: ITextLayout,
    pub wave_text_layout: Option<ITextLayout>,
}

impl Drawable for DrawableText {
    fn draw(&self, ctx: &mut PaintCtx, x: f64, y: f64) {
        ctx.with_save(|ctx| {
            ctx.transform(Affine::translate(Vec2::new(x, y)));
            if let Some(color) = &self.background_color {
                let mut rect = self.text_layout.size().to_rect();
                rect.x1 += self.text_layout.trailing_whitespace_width() - (rect.x1 - rect.x0);
                ctx.fill(&rect.to_rounded_rect(3.0), color);
            }
            ctx.draw_text(&self.text_layout, Point::new(0.0, 0.0));
            if let Some(wave_text_layout) = &self.wave_text_layout {
                ctx.draw_text(wave_text_layout, Point::new(0.0, 0.0));
            }
        });
    }

    fn width(&self) -> f64 {
        self.text_layout.size().width
    }

    fn height(&self) -> f64 {
        self.text_layout.size().height
    }
}

pub fn drawable_text(ctx: &mut PaintCtx, _env: &Env, text: &str, style: &Style) -> DrawableText {
    let scale = {
        let config = lock!(conf);
        config.render.text_scale
    };

    let mut builder = ctx
        .text()
        .new_text_layout(text.to_string())
        .text_color(
            style
                .foreground
                .clone()
                .or_else(|| THEME.scope("ui.text").foreground)
                .unwrap_or(DEFAULT_FOREGROUND_COLOR)
                .clone(),
        )
        .font(
            FontFamily::new_unchecked(
                style
                    .text_font
                    .as_ref()
                    .unwrap_or(&DEFAULT_TEXT_FONT)
                    .as_str(),
            ),
            style.text_size.unwrap_or(DEFAULT_TEXT_SIZE) * scale,
        );

    if let Some(bold) = style.bold {
        if bold {
            builder = builder.range_attribute(.., TextAttribute::Weight(FontWeight::BOLD));
        }
    }
    if let Some(italic) = style.italic {
        if italic {
            builder = builder.range_attribute(.., TextAttribute::Style(FontStyle::Italic));
        }
    }
    if let Some(underline) = style.underline {
        if underline {
            builder = builder.range_attribute(.., TextAttribute::Underline(true));
        }
    }
    let text_layout = builder.build().unwrap();

    let wave_text_layout = style.wavy_underline.clone().map(|color| {
        let mut style = style.clone();
        style.foreground = Some(color);
        style.background = None;
        style.wavy_underline = None;
        let string = "_".repeat(text.chars().count());
        drawable_text(ctx, _env, string.as_str(), &style).text_layout
    });

    DrawableText {
        background_color: style.background.clone(),
        text_layout,
        wave_text_layout,
    }
}
