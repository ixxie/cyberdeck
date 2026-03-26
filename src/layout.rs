use std::sync::Arc;

use tiny_skia::Pixmap;
use unicode_width::UnicodeWidthChar;

use crate::color::Rgba;
use crate::icons::IconSet;

// === Build layer: what views construct ===

#[derive(Clone)]
pub struct Elem {
    pub text: String,
    pub fg: Rgba,
    pub icon: Option<Arc<Pixmap>>,
    pub path: Option<String>,
}

impl Elem {
    pub fn text(text: impl Into<String>) -> Self {
        Self { text: text.into(), fg: Rgba::default(), icon: None, path: None }
    }

    pub fn fg(mut self, fg: Rgba) -> Self {
        self.fg = fg;
        self
    }

    pub fn icon(mut self, pm: Arc<Pixmap>) -> Self {
        self.icon = Some(pm);
        self
    }

    pub fn path(mut self, p: impl Into<String>) -> Self {
        self.path = Some(p.into());
        self
    }
}

#[derive(Clone)]
pub struct Span {
    pub elems: Vec<Elem>,
    pub bg: Option<Rgba>,
    pub radius: f32,
    pub pad_x: f32,
    pub pad_y: f32,
    pub path: Option<String>,
    pub opacity: f32,
}

impl Span {
    pub fn new(elems: Vec<Elem>) -> Self {
        Self { elems, bg: None, radius: 0.0, pad_x: 0.0, pad_y: 0.0, path: None, opacity: 1.0 }
    }

    pub fn bg(mut self, bg: Rgba) -> Self {
        self.bg = Some(bg);
        self
    }

    pub fn radius(mut self, r: f32) -> Self {
        self.radius = r;
        self
    }

    pub fn pad(mut self, px: f32, py: f32) -> Self {
        self.pad_x = px;
        self.pad_y = py;
        self
    }

    pub fn path(mut self, p: impl Into<String>) -> Self {
        self.path = Some(p.into());
        self
    }

    pub fn opacity(mut self, o: f32) -> Self {
        self.opacity = o;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Align { Left, Center, Right }

pub struct Zone {
    pub align: Align,
    pub spans: Vec<Span>,
    pub gap: f32,
}

impl Zone {
    pub fn left(spans: Vec<Span>, gap: f32) -> Self {
        Self { align: Align::Left, spans, gap }
    }

    pub fn center(spans: Vec<Span>, gap: f32) -> Self {
        Self { align: Align::Center, spans, gap }
    }

    pub fn right(spans: Vec<Span>, gap: f32) -> Self {
        Self { align: Align::Right, spans, gap }
    }
}

// === Computed layer: what renderer consumes ===

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

pub struct FElem {
    pub rect: Rect,
    pub text: String,
    pub fg: Rgba,
    pub icon: Option<Arc<Pixmap>>,
}

pub struct FSpan {
    pub rect: Rect,
    pub bg: Option<Rgba>,
    pub radius: f32,
    pub opacity: f32,
    pub elems: Vec<FElem>,
}

pub struct Hit {
    pub rect: Rect,
    pub path: String,
}

pub struct Frame {
    pub spans: Vec<FSpan>,
    pub hits: Vec<Hit>,
}

impl Frame {
    pub fn hit(&self, x: f32, y: f32) -> Option<&str> {
        self.hits.iter().rev()
            .find(|h| h.rect.contains(x, y))
            .map(|h| h.path.as_str())
    }
}

// === Measurement ===

pub struct Metrics<'a> {
    pub cell_w: f32,
    pub cell_h: f32,
    pub scale: f32,
    pub icons: &'a IconSet,
}

impl<'a> Metrics<'a> {
    pub fn text_w(&self, text: &str) -> f32 {
        let mut w = 0.0;
        for ch in text.chars() {
            if ch == '\x01' || ch == '\x02' {
                continue;
            }
            if IconSet::is_icon_char(ch) {
                if let Some(pm) = self.icons.icon_for_char(ch) {
                    w += pm.width() as f32 / self.scale;
                } else {
                    w += self.cell_w;
                }
            } else {
                w += self.cell_w * ch.width().unwrap_or(1) as f32;
            }
        }
        w
    }

    pub fn icon_w(&self, pm: &Pixmap) -> f32 {
        pm.width() as f32 / self.scale
    }

    pub fn elem_w(&self, elem: &Elem) -> f32 {
        let icon_w = elem.icon.as_ref()
            .map(|pm| self.icon_w(pm) + self.cell_w * 0.5)
            .unwrap_or(0.0);
        icon_w + self.text_w(&elem.text)
    }

    pub fn span_w(&self, span: &Span) -> f32 {
        let n = span.elems.len();
        if n == 0 {
            return 0.0;
        }
        let content: f32 = span.elems.iter().map(|e| self.elem_w(e)).sum();
        let gaps = (n.saturating_sub(1)) as f32 * self.cell_w * 0.5;
        content + gaps + 2.0 * span.pad_x
    }

    fn zone_w(&self, zone: &Zone) -> f32 {
        let n = zone.spans.len();
        if n == 0 {
            return 0.0;
        }
        let content: f32 = zone.spans.iter().map(|s| self.span_w(s)).sum();
        let gaps = (n.saturating_sub(1)) as f32 * zone.gap;
        content + gaps
    }
}

// === Layout algorithm ===

pub fn lay(zones: &[Zone], bar_w: f32, bar_h: f32, m: &Metrics) -> Frame {
    let mut spans = Vec::new();
    let mut hits = Vec::new();
    let elem_gap = m.cell_w * 0.5;

    for zone in zones {
        let zone_w = m.zone_w(zone);

        // Zone anchor x
        let zone_x = match zone.align {
            Align::Left => 0.0,
            Align::Center => (bar_w - zone_w) / 2.0,
            Align::Right => bar_w - zone_w,
        };

        let mut sx = zone_x;
        for (si, span) in zone.spans.iter().enumerate() {
            if si > 0 {
                sx += zone.gap;
            }
            let span_w = m.span_w(span);
            let span_h = m.cell_h + 2.0 * span.pad_y;
            let span_y = (bar_h - span_h) / 2.0;
            let span_rect = Rect { x: sx, y: span_y, w: span_w, h: span_h };

            // Position elems within span
            let mut ex = sx + span.pad_x;
            let mut felems = Vec::new();
            for (ei, elem) in span.elems.iter().enumerate() {
                if ei > 0 {
                    ex += elem_gap;
                }
                let ew = m.elem_w(elem);
                let elem_rect = Rect { x: ex, y: span_y + span.pad_y, w: ew, h: m.cell_h };

                felems.push(FElem {
                    rect: elem_rect,
                    text: elem.text.clone(),
                    fg: elem.fg,
                    icon: elem.icon.clone(),
                });

                // Elem-level hit area (only if span has no path)
                if span.path.is_none() {
                    if let Some(ref p) = elem.path {
                        hits.push(Hit { rect: elem_rect, path: p.clone() });
                    }
                }

                ex += ew;
            }

            // Span-level hit area
            if let Some(ref p) = span.path {
                hits.push(Hit { rect: span_rect, path: p.clone() });
            }

            spans.push(FSpan {
                rect: span_rect,
                bg: span.bg,
                radius: span.radius,
                opacity: span.opacity,
                elems: felems,
            });

            sx += span_w;
        }
    }

    Frame { spans, hits }
}
