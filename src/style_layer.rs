use crate::buffer::Index;
use crate::theme::Style;
use crate::BufferData;

#[derive(Default, Clone, Debug)]
pub struct Span {
    pub start: Index,
    pub end: Index,
    pub style: Style,
}

pub trait StyleLayer {
    fn spans(&mut self, buffer: &BufferData, min: Index, max: Index) -> anyhow::Result<Vec<Span>>;
}

pub fn style_for_range(
    layers: &[&[Span]],
    min: Index,
    max: Index,
    initial_cuts: Vec<Index>,
) -> anyhow::Result<Vec<Span>> {
    let mut spans = Vec::new();
    for layer in layers {
        spans.extend(*layer);
    }
    // list of all span min or max
    let mut cuts: Vec<Index> = spans.iter().map(|span| span.start).collect();
    cuts.extend(spans.iter().map(|span| span.end));
    cuts.push(min);
    cuts.push(max);
    cuts.extend(initial_cuts);
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
                if let Some(wavy_underline) = &span.style.wavy_underline {
                    current_span.style.wavy_underline = Some(wavy_underline.clone());
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
    fn spans(&mut self, buf: &BufferData, _min: Index, _max: Index) -> anyhow::Result<Vec<Span>> {
        let mut spans = Vec::new();
        for diag in buf.buffer.diagnostics.0.iter() {
            let mut span = Span::default();
            span.start = diag.bounds.0;
            span.end = diag.bounds.1;

            let color = diag.color();

            span.style.background = Some(color.clone().with_alpha(0.10));
            span.style.wavy_underline = Some(color);
            spans.push(span);
        }
        Ok(spans)
    }
}
