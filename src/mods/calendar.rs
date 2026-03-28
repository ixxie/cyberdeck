use chrono::{Datelike, Local};
use serde_json::{json, Value};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::color::Rgba;
use crate::config::KeyHintDef;
use crate::layout::Elem;
use crate::mods::{InteractiveModule, KeyResult};

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let now = Local::now();
    json!({
        "hour": now.format("%H").to_string(),
        "minute": now.format("%M").to_string(),
        "second": now.format("%S").to_string(),
        "date": now.format("%a %d %b %Y").to_string(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CalendarLevel {
    Week,
    Month,
    Year,
}

pub struct CalendarDeep {
    pub level: CalendarLevel,
    pub year: i32,
    pub month: u32,
    pub week_offset: i32,
    pub year_page: i32,
    pub cursor: usize,
}

impl CalendarDeep {
    pub fn new() -> Self {
        let now = Local::now();
        Self {
            level: CalendarLevel::Week,
            year: now.year(),
            month: now.month(),
            week_offset: 0,
            cursor: now.weekday().num_days_from_monday() as usize,
            year_page: 0,
        }
    }
}

impl InteractiveModule for CalendarDeep {
    fn render_center(&self, fg: Rgba, _data: &serde_json::Value) -> Vec<Vec<Elem>> {
        let highlight_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.72) as u8);
        let idle_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.44) as u8);
        let today = Local::now().date_naive();

        match self.level {
            CalendarLevel::Week => self.render_week(fg, highlight_fg, idle_fg, today),
            CalendarLevel::Month => self.render_month(fg, highlight_fg, idle_fg, today),
            CalendarLevel::Year => self.render_year(fg, highlight_fg, idle_fg, today),
        }
    }

    fn cursor(&self) -> Option<usize> {
        // +1 to account for the context/header pill in week and month views
        match self.level {
            CalendarLevel::Week | CalendarLevel::Month => Some(self.cursor + 1),
            CalendarLevel::Year => Some(self.cursor),
        }
    }

    fn breadcrumb(&self) -> Vec<String> {
        match self.level {
            CalendarLevel::Week => vec!["Calendar".into()],
            CalendarLevel::Month => vec!["Calendar".into(), format!("{}", self.year)],
            CalendarLevel::Year => vec!["Calendar".into(), "Years".into()],
        }
    }

    fn key_hints(&self) -> Vec<KeyHintDef> {
        match self.level {
            CalendarLevel::Week => vec![
                KeyHintDef { key: "←→".into(), action: String::new(), label: "day".into(), icon: None },
                KeyHintDef { key: "↑".into(), action: String::new(), label: "months".into(), icon: None },
                KeyHintDef { key: "t".into(), action: String::new(), label: "today".into(), icon: None },
                KeyHintDef { key: "Esc".into(), action: "back".into(), label: "back".into(), icon: None },
            ],
            CalendarLevel::Month => vec![
                KeyHintDef { key: "←→".into(), action: String::new(), label: "month".into(), icon: None },
                KeyHintDef { key: "↑".into(), action: String::new(), label: "years".into(), icon: None },
                KeyHintDef { key: "↓⏎".into(), action: String::new(), label: "weeks".into(), icon: None },
                KeyHintDef { key: "t".into(), action: String::new(), label: "today".into(), icon: None },
                KeyHintDef { key: "Esc".into(), action: "back".into(), label: "back".into(), icon: None },
            ],
            CalendarLevel::Year => vec![
                KeyHintDef { key: "←→".into(), action: String::new(), label: "year".into(), icon: None },
                KeyHintDef { key: "↓⏎".into(), action: String::new(), label: "months".into(), icon: None },
                KeyHintDef { key: "t".into(), action: String::new(), label: "today".into(), icon: None },
                KeyHintDef { key: "Esc".into(), action: "back".into(), label: "back".into(), icon: None },
            ],
        }
    }

    fn handle_key(&mut self, event: &KeyEvent, _data: &serde_json::Value) -> KeyResult {
        match self.level {
            CalendarLevel::Week => match event.keysym {
                Keysym::Left => {
                    if self.cursor == 0 {
                        self.week_offset -= 1;
                        self.cursor = 6;
                    } else {
                        self.cursor -= 1;
                    }
                    KeyResult::Handled
                }
                Keysym::Right => {
                    if self.cursor >= 6 {
                        self.week_offset += 1;
                        self.cursor = 0;
                    } else {
                        self.cursor += 1;
                    }
                    KeyResult::Handled
                }
                Keysym::Up => {
                    let today = Local::now().date_naive();
                    let week_start = today
                        - chrono::Duration::days(today.weekday().num_days_from_monday() as i64)
                        + chrono::Duration::weeks(self.week_offset as i64);
                    self.year = week_start.year();
                    self.month = week_start.month();
                    self.cursor = (week_start.month() - 1) as usize;
                    self.level = CalendarLevel::Month;
                    KeyResult::Handled
                }
                _ if event.utf8.as_deref() == Some("t") => {
                    self.reset();
                    KeyResult::Handled
                }
                _ => KeyResult::Ignored,
            },
            CalendarLevel::Month => match event.keysym {
                Keysym::Left => {
                    if self.cursor == 0 {
                        self.year -= 1;
                        self.cursor = 11;
                    } else {
                        self.cursor -= 1;
                    }
                    KeyResult::Handled
                }
                Keysym::Right => {
                    if self.cursor >= 11 {
                        self.year += 1;
                        self.cursor = 0;
                    } else {
                        self.cursor += 1;
                    }
                    KeyResult::Handled
                }
                Keysym::Up => {
                    self.cursor = 2;
                    self.level = CalendarLevel::Year;
                    KeyResult::Handled
                }
                Keysym::Down | Keysym::Return => {
                    let selected_month = (self.cursor + 1) as u32;
                    let target = chrono::NaiveDate::from_ymd_opt(
                        self.year, selected_month, 1,
                    ).unwrap();
                    let today = Local::now().date_naive();
                    let today_monday = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
                    let target_monday = target - chrono::Duration::days(target.weekday().num_days_from_monday() as i64);
                    let weeks = (target_monday - today_monday).num_weeks() as i32;
                    self.week_offset = weeks;
                    self.level = CalendarLevel::Week;
                    KeyResult::Handled
                }
                _ if event.utf8.as_deref() == Some("t") => {
                    self.reset();
                    KeyResult::Handled
                }
                _ => KeyResult::Ignored,
            },
            CalendarLevel::Year => match event.keysym {
                Keysym::Left => {
                    if self.cursor == 0 {
                        self.year_page -= 5;
                        self.cursor = 4;
                    } else {
                        self.cursor -= 1;
                    }
                    KeyResult::Handled
                }
                Keysym::Right => {
                    if self.cursor >= 4 {
                        self.year_page += 5;
                        self.cursor = 0;
                    } else {
                        self.cursor += 1;
                    }
                    KeyResult::Handled
                }
                Keysym::Down | Keysym::Return => {
                    let center_year = self.year + self.year_page;
                    self.year = center_year + (self.cursor as i32 - 2);
                    self.year_page = 0;
                    self.cursor = 0;
                    self.level = CalendarLevel::Month;
                    KeyResult::Handled
                }
                _ if event.utf8.as_deref() == Some("t") => {
                    self.reset();
                    KeyResult::Handled
                }
                _ => KeyResult::Ignored,
            },
        }
    }

    fn reset(&mut self) {
        let now = Local::now();
        self.level = CalendarLevel::Week;
        self.year = now.year();
        self.month = now.month();
        self.week_offset = 0;
        self.cursor = now.weekday().num_days_from_monday() as usize;
        self.year_page = 0;
    }
}

impl CalendarDeep {
    fn render_week(
        &self,
        active_fg: Rgba, highlight_fg: Rgba, idle_fg: Rgba,
        today: chrono::NaiveDate,
    ) -> Vec<Vec<Elem>> {
        let dow_names = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
        let month_names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
                           "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

        let week_start = today
            - chrono::Duration::days(today.weekday().num_days_from_monday() as i64)
            + chrono::Duration::weeks(self.week_offset as i64);

        let context = format!("{} {}", month_names[(week_start.month() - 1) as usize], week_start.year());
        let mut items = vec![vec![Elem::text(context).fg(highlight_fg)]];

        for i in 0..7usize {
            let date = week_start + chrono::Duration::days(i as i64);
            let dow = date.weekday().num_days_from_monday() as usize;
            let day_fg = if i == self.cursor {
                active_fg
            } else if date == today {
                highlight_fg
            } else {
                idle_fg
            };
            let entry = format!("{} {:02}", dow_names[dow], date.day());
            items.push(vec![Elem::text(entry).fg(day_fg)]);
        }

        items
    }

    fn render_month(
        &self,
        active_fg: Rgba, highlight_fg: Rgba, idle_fg: Rgba,
        today: chrono::NaiveDate,
    ) -> Vec<Vec<Elem>> {
        let month_names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
                           "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

        let mut items = vec![vec![Elem::text(format!("{}", self.year)).fg(highlight_fg)]];

        for m in 0..12usize {
            let is_current = self.year == today.year() && (m + 1) as u32 == today.month();
            let m_fg = if m == self.cursor {
                active_fg
            } else if is_current {
                highlight_fg
            } else {
                idle_fg
            };
            items.push(vec![Elem::text(month_names[m].to_string()).fg(m_fg)]);
        }

        items
    }

    fn render_year(
        &self,
        active_fg: Rgba, highlight_fg: Rgba, idle_fg: Rgba,
        today: chrono::NaiveDate,
    ) -> Vec<Vec<Elem>> {
        let center_year = self.year + self.year_page;

        (-2..=2).enumerate().map(|(i, offset)| {
            let y = center_year + offset;
            let is_current = y == today.year();
            let y_fg = if i == self.cursor {
                active_fg
            } else if is_current {
                highlight_fg
            } else {
                idle_fg
            };
            vec![Elem::text(format!("{y}")).fg(y_fg)]
        }).collect()
    }
}
