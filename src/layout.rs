use std::sync::Arc;

use taffy::prelude::*;
use taffy::style::Overflow;
use taffy::geometry::Point as TaffyPoint;
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

/// Bar content: left, center, and right span groups with a shared gap.
pub struct BarContent {
    pub left: Vec<Span>,
    pub center: Vec<Span>,
    pub right: Vec<Span>,
    pub gap: f32,
    /// Fixed widths for left/right zones (set after measurement).
    /// When set, the zone uses this width instead of auto-sizing from content.
    pub left_w: Option<f32>,
    pub right_w: Option<f32>,
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
    /// Natural (unconstrained) content width — may exceed rect.w when flex-shrunk.
    pub content_w: f32,
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

pub struct Metrics {
    pub cell_w: f32,
    pub cell_h: f32,
    pub scale: f32,
    pub elem_widths: Vec<f32>,
    pub span_widths: Vec<f32>,
    pub elem_gap: f32,
}

impl Metrics {
    /// Measure all spans up front using the renderer's font shaping.
    pub fn measure(
        content: &BarContent,
        cell_w: f32,
        cell_h: f32,
        scale: f32,
        font_scale: f32,
        renderer: &mut crate::render::Renderer,
        icons: &IconSet,
    ) -> Self {
        let elem_gap = cell_w * 0.5;
        let mut elem_widths = Vec::new();
        let mut span_widths = Vec::new();

        let all_spans: Vec<&Span> = content.left.iter()
            .chain(content.center.iter())
            .chain(content.right.iter())
            .collect();

        for span in &all_spans {
            let n = span.elems.len();
            let mut content_w = 0.0;
            for elem in &span.elems {
                let icon_gap = cell_w;
                let icon_w = elem.icon.as_ref()
                    .map(|pm| pm.width() as f32 / scale + icon_gap)
                    .unwrap_or(0.0);
                let text_w = renderer.measure_text(&elem.text, icons, scale, font_scale);
                let ew = icon_w + text_w;
                elem_widths.push(ew);
                content_w += ew;
            }
            let gaps = n.saturating_sub(1) as f32 * elem_gap;
            let sw = if n == 0 { 0.0 } else { content_w + gaps + 2.0 * span.pad_x };
            span_widths.push(sw);
        }

        Self { cell_w, cell_h, scale, elem_widths, span_widths, elem_gap }
    }

    pub fn elem_w_at(&self, idx: usize) -> f32 {
        self.elem_widths.get(idx).copied().unwrap_or(0.0)
    }

    pub fn span_w_at(&self, idx: usize) -> f32 {
        self.span_widths.get(idx).copied().unwrap_or(0.0)
    }

    /// Sum of span widths for a range of the flat span index.
    pub fn spans_w(&self, range: std::ops::Range<usize>, gap: f32) -> f32 {
        let w: f32 = range.clone().filter_map(|i| self.span_widths.get(i).copied()).sum();
        let n = range.len();
        w + n.saturating_sub(1) as f32 * gap
    }
}

// === Taffy-based layout ===

fn length(v: f32) -> LengthPercentage {
    LengthPercentage::Length(v)
}

pub fn lay(content: &BarContent, bar_w: f32, bar_h: f32, track_pad: f32, m: &Metrics) -> Frame {
    let mut tree = TaffyTree::<()>::new();
    let elem_gap = m.elem_gap;
    let gap = content.gap;

    // Collect all span groups: left=0, center=1, right=2
    let groups: [&[Span]; 3] = [&content.left, &content.center, &content.right];

    struct SpanInfo {
        node: NodeId,
        span_idx: usize,
        group_idx: usize,
        flat_idx: usize,
        elem_nodes: Vec<NodeId>,
    }
    let mut span_infos: Vec<SpanInfo> = Vec::new();
    let mut group_nodes: Vec<NodeId> = Vec::new();
    let mut elem_idx = 0usize;
    let mut flat_span_idx = 0usize;

    for (gi, spans) in groups.iter().enumerate() {
        let mut span_nodes = Vec::new();

        for (si, span) in spans.iter().enumerate() {
            let mut elem_nodes = Vec::new();
            for _elem in &span.elems {
                let ew = m.elem_w_at(elem_idx);
                elem_idx += 1;
                let node = tree.new_leaf(Style {
                    size: Size {
                        width: Dimension::Length(ew),
                        height: Dimension::Length(m.cell_h),
                    },
                    flex_shrink: 1.0,
                    ..Default::default()
                }).unwrap();
                elem_nodes.push(node);
            }

            let span_node = tree.new_with_children(
                Style {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: Some(AlignItems::Center),
                    flex_shrink: 1.0,
                    overflow: TaffyPoint {
                        x: Overflow::Hidden,
                        y: Overflow::Hidden,
                    },
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
                group_idx: gi,
                flat_idx: flat_span_idx,
                elem_nodes,
            });
            flat_span_idx += 1;
            span_nodes.push(span_node);
        }

        let (flex_grow, flex_shrink, justify, fixed_w) = match gi {
            0 => (0.0, 0.0, JustifyContent::FlexStart, content.left_w),
            1 => (1.0, 1.0, JustifyContent::Center, None),
            _ => (0.0, 0.0, JustifyContent::FlexEnd, content.right_w),
        };

        let width = match fixed_w {
            Some(w) => Dimension::Length(w),
            None => Dimension::Auto,
        };

        let group_node = tree.new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                justify_content: Some(justify),
                align_items: Some(AlignItems::Center),
                overflow: TaffyPoint {
                    x: Overflow::Hidden,
                    y: Overflow::Hidden,
                },
                flex_grow,
                flex_shrink,
                size: Size {
                    width,
                    height: Dimension::Percent(1.0),
                },
                gap: Size {
                    width: length(gap),
                    height: length(0.0),
                },
                ..Default::default()
            },
            &span_nodes,
        ).unwrap();

        group_nodes.push(group_node);
    }

    // Root: left | center (flex:1) | right
    let root = tree.new_with_children(
        Style {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
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
            gap: Size {
                width: length(gap),
                height: length(0.0),
            },
            ..Default::default()
        },
        &group_nodes,
    ).unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    // Read back positions and build Frame
    let mut spans = Vec::new();
    let mut hits = Vec::new();

    for info in &span_infos {
        let src_span = &groups[info.group_idx][info.span_idx];
        let sl = tree.layout(info.node).unwrap();
        let gl = tree.layout(group_nodes[info.group_idx]).unwrap();

        let span_rect = Rect {
            x: gl.location.x + sl.location.x,
            y: gl.location.y + sl.location.y,
            w: sl.size.width,
            h: sl.size.height,
        };

        let mut felems = Vec::new();
        for (ei, &enode) in info.elem_nodes.iter().enumerate() {
            let el = tree.layout(enode).unwrap();
            let elem = &src_span.elems[ei];

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

            if src_span.path.is_none() {
                if let Some(ref p) = elem.path {
                    hits.push(Hit { rect: elem_rect, path: p.clone() });
                }
            }
        }

        if let Some(ref p) = src_span.path {
            hits.push(Hit { rect: span_rect, path: p.clone() });
        }

        spans.push(FSpan {
            rect: span_rect,
            bg: src_span.bg,
            radius: src_span.radius,
            opacity: src_span.opacity,
            elems: felems,
            content_w: m.span_w_at(info.flat_idx),
        });
    }

    Frame { spans, hits }
}
