use std::sync::Arc;

use taffy::prelude::*;
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

    pub fn elem_gap(&self) -> f32 { self.cell_w * 0.5 }

    pub fn elem_w(&self, elem: &Elem) -> f32 {
        let icon_w = elem.icon.as_ref()
            .map(|pm| self.icon_w(pm) + self.elem_gap())
            .unwrap_or(0.0);
        icon_w + self.text_w(&elem.text)
    }

    pub fn span_w(&self, span: &Span) -> f32 {
        let n = span.elems.len();
        if n == 0 {
            return 0.0;
        }
        let content: f32 = span.elems.iter().map(|e| self.elem_w(e)).sum();
        let gaps = (n.saturating_sub(1)) as f32 * self.elem_gap();
        content + gaps + 2.0 * span.pad_x
    }
}

// === Taffy-based layout ===

fn length(v: f32) -> LengthPercentage {
    LengthPercentage::Length(v)
}

pub fn lay(zones: &[Zone], bar_w: f32, bar_h: f32, track_pad: f32, m: &Metrics) -> Frame {
    let mut tree = TaffyTree::<()>::new();
    let elem_gap = m.elem_gap();

    // Track (zone_idx, span_idx, elem_idx) → taffy node for reading back positions
    struct SpanInfo {
        node: NodeId,
        span_idx: usize,
        zone_idx: usize,
        elem_nodes: Vec<NodeId>,
    }
    let mut span_infos: Vec<SpanInfo> = Vec::new();

    let mut zone_nodes = Vec::new();

    for (zi, zone) in zones.iter().enumerate() {
        let mut span_nodes = Vec::new();

        for (si, span) in zone.spans.iter().enumerate() {
            // Elem leaves
            let mut elem_nodes = Vec::new();
            for elem in &span.elems {
                let ew = m.elem_w(elem);
                let node = tree.new_leaf(Style {
                    size: Size {
                        width: Dimension::Length(ew),
                        height: Dimension::Length(m.cell_h),
                    },
                    ..Default::default()
                }).unwrap();
                elem_nodes.push(node);
            }

            // Span container: row, padding, gap between elems
            let span_node = tree.new_with_children(
                Style {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: Some(AlignItems::Center),
                    padding: taffy::prelude::Rect {
                        left: length(span.pad_x),
                        right: length(span.pad_x),
                        top: length(span.pad_y),
                        bottom: length(span.pad_y),
                    },
                    gap: Size {
                        width: length(elem_gap),
                        height: length(0.0),
                    },
                    ..Default::default()
                },
                &elem_nodes,
            ).unwrap();

            span_infos.push(SpanInfo {
                node: span_node,
                span_idx: si,
                zone_idx: zi,
                elem_nodes,
            });
            span_nodes.push(span_node);
        }

        // Zone container: row with alignment
        let justify = match zone.align {
            Align::Left => JustifyContent::FlexStart,
            Align::Center => JustifyContent::Center,
            Align::Right => JustifyContent::FlexEnd,
        };

        let zone_node = tree.new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                justify_content: Some(justify),
                align_items: Some(AlignItems::Center),
                // Zone is absolutely positioned to overlay with other zones
                position: Position::Absolute,
                inset: taffy::prelude::Rect {
                    left: LengthPercentageAuto::Length(0.0),
                    right: LengthPercentageAuto::Length(0.0),
                    top: LengthPercentageAuto::Length(0.0),
                    bottom: LengthPercentageAuto::Length(0.0),
                },
                size: Size {
                    width: Dimension::Percent(1.0),
                    height: Dimension::Percent(1.0),
                },
                gap: Size {
                    width: length(zone.gap),
                    height: length(0.0),
                },
                ..Default::default()
            },
            &span_nodes,
        ).unwrap();

        zone_nodes.push(zone_node);
    }

    // Root: the bar, with track padding as horizontal inset
    let root = tree.new_with_children(
        Style {
            display: Display::Flex,
            size: Size {
                width: Dimension::Length(bar_w),
                height: Dimension::Length(bar_h),
            },
            padding: taffy::prelude::Rect {
                left: length(track_pad),
                right: length(track_pad),
                top: length(0.0),
                bottom: length(0.0),
            },
            ..Default::default()
        },
        &zone_nodes,
    ).unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    // Read back positions and build Frame
    let mut spans = Vec::new();
    let mut hits = Vec::new();

    for info in &span_infos {
        let span = &zones[info.zone_idx].spans[info.span_idx];
        let sl = tree.layout(info.node).unwrap();
        let zl = tree.layout(zone_nodes[info.zone_idx]).unwrap();

        let span_rect = Rect {
            x: zl.location.x + sl.location.x,
            y: zl.location.y + sl.location.y,
            w: sl.size.width,
            h: sl.size.height,
        };

        let mut felems = Vec::new();
        for (ei, &enode) in info.elem_nodes.iter().enumerate() {
            let el = tree.layout(enode).unwrap();
            let elem = &span.elems[ei];

            let elem_rect = Rect {
                x: span_rect.x + el.location.x,
                y: span_rect.y + el.location.y,
                w: el.size.width,
                h: el.size.height,
            };

            felems.push(FElem {
                rect: elem_rect,
                text: elem.text.clone(),
                fg: elem.fg,
                icon: elem.icon.clone(),
            });

            if span.path.is_none() {
                if let Some(ref p) = elem.path {
                    hits.push(Hit { rect: elem_rect, path: p.clone() });
                }
            }
        }

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
    }

    Frame { spans, hits }
}
