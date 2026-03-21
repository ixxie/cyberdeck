use unicode_width::UnicodeWidthChar;

use crate::color::Rgba;

#[derive(Debug, Clone)]
pub struct RenderedWidget {
    pub text: String,
    pub fg: Option<Rgba>,
    pub bg: Option<Rgba>,
    pub path: Option<String>,
    pub is_breadcrumb: bool,
    pub icon_scale: Option<f32>,
}

impl RenderedWidget {
    pub fn new(text: String) -> Self {
        Self { text, fg: None, bg: None, path: None, is_breadcrumb: false, icon_scale: None }
    }

    pub fn with_path(mut self, path: &str) -> Self {
        self.path = Some(path.to_string());
        self
    }

    pub fn with_fg(mut self, fg: Rgba) -> Self {
        self.fg = Some(fg);
        self
    }

    pub fn with_icon_scale(mut self, scale: f32) -> Self {
        self.icon_scale = Some(scale);
        self
    }

}

#[derive(Debug, Clone)]
pub struct HitArea {
    pub start_x: f32,
    pub end_x: f32,
    pub path: String,
    pub is_breadcrumb: bool,
}

// --- Flex Layout ---

#[derive(Debug, Clone)]
pub struct LayoutItem {
    pub x: f32,
    pub width: f32,
    pub text: String,
    pub fg: Rgba,
    pub bg: Option<Rgba>,
    pub icon_scale: Option<f32>,
}

#[derive(Debug)]
pub struct Layout {
    pub items: Vec<LayoutItem>,
    pub hit_areas: Vec<HitArea>,
}

impl Layout {
    pub fn measure(text: &str, icons: &crate::icons::IconSet, cell_w: f32, scale: f32, icon_scale: Option<f32>) -> f32 {
        let iscale = icon_scale.unwrap_or(1.0);
        let mut w = 0.0;
        for ch in text.chars() {
            if ch == '\x01' || ch == '\x02' { continue; }
            if crate::icons::IconSet::is_icon_char(ch) {
                if let Some(pm) = icons.icon_for_char(ch) {
                    w += pm.width() as f32 / scale * iscale;
                } else {
                    w += cell_w;
                }
            } else {
                let cw = ch.width().unwrap_or(1) as f32;
                w += cell_w * cw;
            }
        }
        w
    }

    pub fn flex(
        groups: &[Vec<RenderedWidget>],
        bar_width: f32,
        icons: &crate::icons::IconSet,
        cell_w: f32,
        scale: f32,
        gap_px: f32,
        fg: Rgba,
        _bg: Rgba,
    ) -> Self {
        let mut items = Vec::new();
        let mut hit_areas = Vec::new();

        if groups.is_empty() {
            return Self { items, hit_areas };
        }

        let group_widths: Vec<f32> = groups.iter().map(|group| {
            let content: f32 = group.iter()
                .map(|rw| Self::measure(&rw.text, icons, cell_w, scale, rw.icon_scale))
                .sum();
            let gaps = group.len().saturating_sub(1) as f32 * gap_px;
            content + gaps
        }).collect();

        let total_width: f32 = group_widths.iter().sum();
        let num_groups = groups.len();

        let group_gap = if num_groups > 1 && total_width < bar_width {
            (bar_width - total_width) / (num_groups - 1) as f32
        } else {
            0.0
        };

        let mut x = 0.0;
        for (gi, group) in groups.iter().enumerate() {
            if gi > 0 {
                if num_groups == 3 && gi == 1 {
                    // Center group: always centered in the bar
                    x = (bar_width - group_widths[1]) / 2.0;
                } else if gi == num_groups - 1 {
                    // Last group: right-aligned
                    x = bar_width - group_widths[gi];
                } else {
                    x += group_gap;
                }
            }

            for (wi, rw) in group.iter().enumerate() {
                if wi > 0 {
                    x += gap_px;
                }

                let w_fg = rw.fg.unwrap_or(fg);
                let w_bg = rw.bg;
                let item_w = Self::measure(&rw.text, icons, cell_w, scale, rw.icon_scale);

                items.push(LayoutItem {
                    x, width: item_w,
                    text: rw.text.clone(),
                    fg: w_fg, bg: w_bg,
                    icon_scale: rw.icon_scale,
                });

                if rw.is_breadcrumb {
                    hit_areas.push(HitArea {
                        start_x: x, end_x: x + item_w,
                        path: String::new(),
                        is_breadcrumb: true,
                    });
                } else if let Some(path) = &rw.path {
                    hit_areas.push(HitArea {
                        start_x: x, end_x: x + item_w,
                        path: path.clone(),
                        is_breadcrumb: false,
                    });
                }

                x += item_w;
            }
        }

        Self { items, hit_areas }
    }

    pub fn flex_text_mode(
        prefix: Option<&RenderedWidget>,
        query: &str,
        items_list: &[(String, Vec<usize>)],
        total: usize,
        selected: usize,
        shortcuts: &str,
        bar_width: f32,
        _icons: &crate::icons::IconSet,
        cell_w: f32,
        _scale: f32,
        gap_px: f32,
        fg: Rgba,
        idle_fg: Rgba,
        _bg: Rgba,
    ) -> Self {
        let mut items = Vec::new();
        let selected = selected.min(items_list.len().saturating_sub(1));

        let mut x = 0.0;

        // Prefix icon (with icon_scale from indicator config)
        if let Some(pw) = prefix {
            let prefix_w = pw.text.chars().map(|c| c.width().unwrap_or(1) as f32).sum::<f32>() * cell_w;
            items.push(LayoutItem {
                x, width: prefix_w,
                text: pw.text.clone(), fg, bg: None, icon_scale: pw.icon_scale,
            });
            x += prefix_w + gap_px;
        }

        // Query with cursor
        let query_display = format!("{}_", query);
        let query_w = query_display.len() as f32 * cell_w;
        items.push(LayoutItem {
            x, width: query_w,
            text: query_display, fg, bg: None, icon_scale: None,
        });

        // Count on right
        let count_text = format!("{}/{}", items_list.len(), total);
        let right_text = if shortcuts.is_empty() { count_text }
            else { format!("{shortcuts}  {count_text}") };
        let right_w = right_text.chars().map(|c| c.width().unwrap_or(1) as f32).sum::<f32>() * cell_w;
        let right_x = bar_width - right_w;
        items.push(LayoutItem {
            x: right_x, width: right_w,
            text: right_text, fg: idle_fg, bg: None, icon_scale: None,
        });

        // Items centered
        let left_end = x + query_w + gap_px * 2.0;
        let right_begin = right_x - gap_px * 2.0;
        let avail = right_begin - left_end;
        let pad_px = cell_w;

        let item_widths: Vec<f32> = items_list.iter()
            .map(|(t, _)| t.chars().map(|c| c.width().unwrap_or(1) as f32).sum::<f32>() * cell_w + pad_px * 2.0)
            .collect();
        let total_items_w: f32 = item_widths.iter().sum::<f32>()
            + items_list.len().saturating_sub(1) as f32 * gap_px;

        let center_start = if total_items_w <= avail {
            left_end + (avail - total_items_w) / 2.0
        } else {
            left_end
        };

        let mut x = center_start;
        for (i, (text, _)) in items_list.iter().enumerate() {
            if i > 0 { x += gap_px; }
            let is_sel = i == selected;
            let item_fg = if is_sel { fg } else { idle_fg };
            let item_w = item_widths[i];

            items.push(LayoutItem {
                x, width: item_w,
                text: format!(" {} ", text),
                fg: item_fg, bg: None, icon_scale: None,
            });

            x += item_w;
        }

        Self { items, hit_areas: Vec::new() }
    }
}
