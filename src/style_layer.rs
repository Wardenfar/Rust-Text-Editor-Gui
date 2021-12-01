use crate::buffer::{Index, IntoWithBuffer};
use crate::lock;
use crate::theme::Style;
use druid::Color;

#[derive(Default, Clone, Debug)]
pub struct Span {
    pub start: Index,
    pub end: Index,
    pub style: Style,
}

pub trait StyleLayer {
    fn spans(&mut self, buffer_id: u32, min: Index, max: Index) -> anyhow::Result<Vec<Span>>;
}

pub fn style_for_range(layers: &[&[Span]], min: Index, max: Index) -> anyhow::Result<Vec<Span>> {
    let mut spans = Vec::new();
    for layer in layers {
        spans.extend(*layer);
    }
    // list of all span min or max
    let mut cuts: Vec<Index> = spans.iter().map(|span| span.start).collect();
    cuts.extend(spans.iter().map(|span| span.end));
    cuts.push(min);
    cuts.push(max);
    let mut cuts: Vec<Index> = cuts.into_iter().filter(|&x| min <= x && x <= max).collect();
    cuts.sort();
    cuts.dedup();

    let mut current_span = Span::default();
    current_span.start = min;
    current_span.end = min;
    let mut final_spans = Vec::new();
    for cut in cuts.iter().skip(1) {
        current_span.end = *cut;
        for span in &spans {
            // if span is between current_span.start and cut
            // set modifiers if the option is some
            if span.start <= current_span.start && span.end >= current_span.end {
                if let Some(foreground) = &span.style.foreground {
                    current_span.style.foreground = Some(foreground.clone());
                }
                if let Some(background) = &span.style.background {
                    current_span.style.background = Some(background.clone());
                }
                if let Some(underline) = &span.style.underline {
                    current_span.style.underline = Some(underline.clone());
                }
                if let Some(italic) = &span.style.italic {
                    current_span.style.italic = Some(italic.clone());
                }
                if let Some(bold) = &span.style.bold {
                    current_span.style.bold = Some(bold.clone());
                }
            }
        }
        final_spans.push(current_span.clone());
        current_span = Span {
            start: *cut,
            end: *cut,
            style: Style::default(),
        };
    }
    Ok(final_spans)
}

pub struct DiagStyleLayer();

impl StyleLayer for DiagStyleLayer {
    fn spans(&mut self, buffer_id: u32, _min: Index, _max: Index) -> anyhow::Result<Vec<Span>> {
        let buffers = lock!(buffers);
        let buf = buffers.get(buffer_id)?;
        let mut spans = Vec::new();
        for diagnostic in buf.buffer.diagnostics.iter() {
            let mut span = Span::default();
            span.start = (&diagnostic.range.start).into_with_buf(&buf.buffer);
            span.end = (&diagnostic.range.end).into_with_buf(&buf.buffer);
            span.style.foreground = Some(Color::RED);
            span.style.underline = Some(true);
            span.style.italic = Some(true);
            spans.push(span);
        }
        Ok(spans)
    }
}
