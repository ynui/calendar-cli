use chrono::{Datelike, Local, NaiveDate, Weekday};
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;
use unicode_bidi::BidiInfo;

use crate::app::{char_to_byte, App, AuthState, Focus, Mode, ViewMode};
use crate::models::{CalendarEvent, FormState};

fn bidi(text: &str) -> String {
    if text.is_empty() {
        return text.to_string();
    }
    let bidi_info = BidiInfo::new(text, None);
    match bidi_info.paragraphs.first() {
        Some(para) => bidi_info.reorder_line(para, para.range.clone()).into_owned(),
        None => text.to_string(),
    }
}

// ── Main render ─────────────────────────────────────────────────

pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    let main = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas::<2>(area);
    let [content, status_bar] = main;

    let evt_area = render_content(frame, content, app);
    render_status(frame, status_bar, app);

    if app.menu_open {
        render_menu_dropdown(frame, app);
    }

    // Search bar: show at bottom of event list area when active
    if app.search_query.is_some() {
        render_search_bar(frame, app, evt_area);
    }

    match &app.mode {
        Mode::Creating(form) => render_form(frame, " New Event ", form, app),
        Mode::Editing(form) => render_form(frame, " Edit Event ", form, app),
        Mode::Deleting => render_delete_dialog(frame, app),
        Mode::ConfirmingQuit => render_confirm_quit(frame, app),
        Mode::Help => render_help(frame, app),
        Mode::Settings => render_settings(frame, app),
        Mode::JumpToDate(value, cursor) => render_jump_date(frame, value, *cursor, app),
        Mode::ViewingDetail(event) => render_event_detail(frame, event, app),
        Mode::ViewingEvents(events, cursor) => render_event_list_popup(frame, events, *cursor, app),
        _ => {}
    }

    match &app.auth_state {
        AuthState::Listening { .. } => render_auth_dialog(frame, "Waiting for authorization...", app),
        AuthState::Message(msg) => render_auth_dialog(frame, msg, app),
        _ => {}
    }
}

// ── Content area ────────────────────────────────────────────────

fn render_content(frame: &mut Frame, area: Rect, app: &App) -> Rect {
    if app.view_mode == ViewMode::Week {
        return render_week_view(frame, area, app);
    }
    let panels = Layout::horizontal([
        Constraint::Length(38),
        Constraint::Min(1),
    ])
    .areas::<2>(area);
    let [cal_area, evt_area] = panels;

    render_calendar(frame, cal_area, app);
    render_event_list(frame, evt_area, app);
    evt_area
}

// ── Status bar ──────────────────────────────────────────────────

fn render_status(frame: &mut Frame, area: Rect, app: &App) {
    let now = Local::now();
    let time = now.format(" %a %b %d %Y %H:%M:%S ").to_string();

    let text = bidi(&status_text(app));
    let mut spans: Vec<Span> = text
        .split("  ")
        .filter(|s| !s.is_empty())
        .enumerate()
        .flat_map(|(i, part)| {
            let mut out = Vec::new();
            if i > 0 {
                out.push(Span::raw("  "));
            }
            if part.starts_with('[') {
                let close = part.find(']').unwrap_or(part.len());
                let key = &part[1..close];
                let rest = &part[close + 1..];
                out.push(Span::styled(format!("[{}]", key), app.theme.help_key));
                out.push(Span::styled(rest, Style::new().fg(Color::White)));
            } else {
                out.push(Span::styled(part, Style::new().fg(Color::White)));
            }
            out
        })
        .collect();

    // Push time to the right
    let mut dance = String::new();
    if app.dance_style != crate::app::DanceStyle::None {
        let frames = app.dance_style.frames();
        let frame = frames[app.frame % frames.len()];
        dance = format!(" {} ", frame);
    }
    let used: usize = spans.iter().map(|s| s.content.len()).sum();
    let pad = area.width.saturating_sub(used as u16 + dance.len() as u16 + time.len() as u16);
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad as usize)));
    }
    if !dance.is_empty() {
        spans.push(Span::styled(dance, Style::new().fg(Color::Cyan)));
    }
    spans.push(Span::styled(time, Style::new().fg(Color::Yellow)));

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::new().bg(Color::Black)),
        area,
    );
}

fn status_text(app: &App) -> String {
    if !app.status.is_empty() {
        return app.status.clone();
    }
    if app.search_query.is_some() {
        return "[Esc] Cancel  [Enter] Jump  [Up/Down] Navigate  Type to search".into();
    }
    match app.mode {
        Mode::Normal if app.menu_open => "[Enter] Select  [Esc] Cancel".into(),
        Mode::Normal => {
            if app.view_mode == ViewMode::Week {
                "? for help  [w] Month view".into()
            } else {
                "? for help  [w] Week view".into()
            }
        }
        Mode::JumpToDate(_, _) => "[Enter] Jump  [Esc] Cancel".into(),
        Mode::Creating(_) | Mode::Editing(_) => "[Tab] Next  [Enter] Save  [Esc] Cancel".into(),
        Mode::Deleting => "[Enter] Confirm  [Esc] Cancel".into(),
        Mode::ConfirmingQuit => "[y] Yes  [n] No  [Enter] Yes  [Esc] No".into(),
        Mode::Help => "[Esc] Close".into(),
        Mode::Settings => "[Enter] Execute  [Esc] Back".into(),
        Mode::ViewingDetail(_) => "[Esc] Close".into(),
        Mode::ViewingEvents(_, _) => "[Up/Down] Navigate  [Enter] Details  [Esc] Close".into(),
    }
}

// ── Calendar grid ───────────────────────────────────────────────

fn render_calendar(frame: &mut Frame, area: Rect, app: &App) {
    let year = app.view_date.year();
    let month = app.view_date.month();

    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    let last = month_last_day(year, month);

    let start_dow = if app.first_day_of_week == 0 {
        first.weekday().num_days_from_monday() as usize
    } else {
        first.weekday().num_days_from_sunday() as usize
    };

    let mut lines: Vec<Line> = Vec::new();

    // Day-of-week header — each slot 5 chars (includes trailing space)
    let days: &[&str; 7] = if app.first_day_of_week == 0 {
        &["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"]
    } else {
        &["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"]
    };
    let mut dow_spans = Vec::new();
    for (i, d) in days.iter().enumerate() {
        let is_weekend = if app.first_day_of_week == 0 {
            i >= 5
        } else {
            i == 0 || i == 6
        };
        let s = if is_weekend {
            Style::new().fg(app.theme.weekend)
        } else {
            Style::new().fg(Color::Gray)
        };
        dow_spans.push(Span::styled(format!(" {}  ", d), s));
    }
    lines.push(Line::from(dow_spans));

    // Build weeks — always 6 rows for consistent height
    let prev_last = month_last_day(year - if month == 1 { 1 } else { 0 }, if month == 1 { 12 } else { month - 1 });
    let mut day: u32 = 1;
    let mut next_day: u32 = 1;
    let dim = Style::new().fg(Color::DarkGray);
    for week in 0..6 {
        let mut row_spans = Vec::new();
        for dow in 0..7 {
            if week == 0 && dow < start_dow {
                let pd = prev_last.day() - (start_dow as u32 - 1 - dow as u32);
                row_spans.push(Span::styled(format!(" {:>2}  ", pd), dim));
                continue;
            }
            if day > last.day() {
                row_spans.push(Span::styled(format!(" {:>2}  ", next_day), dim));
                next_day += 1;
                continue;
            }

            let current = NaiveDate::from_ymd_opt(year, month, day).unwrap();
            let style = day_style(current, app);
            let has_dot = has_event_on(current, &app.month_events);

            // Each cell is exactly 5 chars (space + 2-char day + dot/space + space)
            let cell = if has_dot {
                format!(" {:>2}· ", day)
            } else {
                format!(" {:>2}  ", day)
            };

            row_spans.push(Span::styled(cell, style));
            day += 1;
        }
        lines.push(Line::from(row_spans));
    }

    let is_focused = matches!(app.focus, Focus::Calendar);
    let title = format!(" {} {} ", first.format("%B"), year);
    let title_line = Line::from(Span::styled(title, Style::new().bold().fg(Color::White)));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if is_focused {
            Style::new().fg(app.theme.active_border)
        } else {
            Style::new().fg(app.theme.inactive_border)
        })
        .title(title_line)
        .title_alignment(Alignment::Left);

    let paragraph = Paragraph::new(Text::from(lines)).block(block);
    frame.render_widget(paragraph, area);
}

fn day_style(date: NaiveDate, app: &App) -> Style {
    let today = Local::now().naive_local().date();
    let is_weekend = matches!(date.weekday(), Weekday::Sat | Weekday::Sun);

    let mut style = Style::new();

    if date == app.selected_date {
        style = style
            .bg(app.theme.selected_bg)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        return style;
    }

    if date == today {
        style = app.theme.today;
        if is_weekend {
            style = style.fg(Color::Yellow);
        }
        return style;
    }

    if is_weekend {
        style = style.fg(app.theme.weekend);
    } else {
        style = style.fg(Color::White);
    }

    style
}

fn month_last_day(year: i32, month: u32) -> NaiveDate {
    if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap().pred_opt().unwrap()
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap().pred_opt().unwrap()
    }
}

fn has_event_on(date: NaiveDate, events: &[crate::models::CalendarEvent]) -> bool {
    events.iter().any(|e| match (e.start, e.end) {
        (Some(s), Some(e)) => s.date() <= date && date <= e.date(),
        (Some(s), None) => s.date() == date,
        (None, _) => false,
    })
}

// ── Event list ──────────────────────────────────────────────────

fn render_event_list(frame: &mut Frame, area: Rect, app: &App) {
    let is_focused = matches!(app.focus, Focus::EventList);

    let is_searching = app.search_query.is_some();
    let count_str = if app.events.is_empty() {
        String::new()
    } else {
        format!("({})", app.events.len())
    };

    let title = if is_searching {
        let q = app.search_query.as_deref().unwrap_or("");
        let q_bidi = bidi(q);
        if q_bidi != q {
            format!(" Search: \"{}\" {} ", q_bidi, count_str)
        } else {
            format!(" Search: \"{}\" {} ", q, count_str)
        }
    } else {
        let day_name = app.selected_date.format("%a");
        format!(
            " {}, {} {} ",
            day_name,
            app.selected_date.format("%b %d"),
            count_str
        )
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if is_focused {
            Style::new().fg(app.theme.active_border)
        } else {
            Style::new().fg(app.theme.inactive_border)
        })
        .title(title);

    if app.events.is_empty() || app.loading {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let msg = if app.loading {
            " Loading…"
        } else if app.events_loaded {
            " No events"
        } else {
            " Loading…"
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(msg, app.theme.dim.add_modifier(Modifier::ITALIC)))),
            inner,
        );
        return;
    }

    let items: Vec<ListItem> = app
        .events
        .iter()
        .enumerate()
        .map(|(i, event)| {
            let time_str = match (event.start, event.end) {
                (Some(s), Some(e)) => format!("{}-{}", s.format("%H:%M"), e.format("%H:%M")),
                (Some(s), None) => format!("{}–", s.format("%H:%M")),
                (None, _) => "      ".into(),
            };

            let is_selected = i == app.event_focus;
            let style = if is_selected && is_focused {
                Style::new().bg(app.theme.selected_bg).fg(Color::White).add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::new().bg(Color::DarkGray)
            } else {
                Style::new()
            };

            let desc = event
                .description
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|s| {
                    let max = 30usize;
                    let truncated: String = s.chars().take(max).collect();
                    if s.len() > max {
                        format!(" {}", truncated) + "…"
                    } else {
                        format!(" {}", truncated)
                    }
                })
                .unwrap_or_default();

            let content = Line::from(vec![
                Span::styled(format!(" {}", time_str), Style::new().fg(Color::Gray)),
                Span::raw(" "),
                Span::styled(bidi(&event.summary), Style::new()),
                Span::styled(bidi(&desc), Style::new().fg(Color::DarkGray)),
            ]);

            ListItem::new(content).style(style)
        })
        .collect();

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Calculate how many items fit
    let available = inner.height.saturating_sub(1);
    let items_height = items.len() as u16;

    if items_height <= available {
        let list = List::new(items).highlight_symbol("");
        frame.render_widget(list, inner);
    } else {
        // Only show visible items
        let scroll = app.event_focus.saturating_sub((available - 1) as usize / 2);
        let visible: Vec<ListItem> = items.into_iter().skip(scroll).take(available as usize).collect();
        let list = List::new(visible).highlight_symbol("");
        frame.render_widget(list, inner);

        // Scroll indicator
        if scroll + (available as usize) < app.events.len() {
            let indicator = Paragraph::new(Line::from(Span::styled(
                format!(" ↓ {} more ", app.events.len() - scroll - available as usize),
                app.theme.dim,
            )));
            let indicator_area = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
            frame.render_widget(indicator, indicator_area);
        }
    }

}

// ── Search bar ─────────────────────────────────────────────────

fn render_search_bar(frame: &mut Frame, app: &App, content: Rect) {
    let query = app.search_query.as_deref().unwrap_or("");
    let area = Rect::new(
        content.x,
        content.y + content.height.saturating_sub(2),
        content.width.min(60),
        1,
    );
    let query_visual = if query.is_empty() {
        " ".to_string()
    } else {
        bidi(query)
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Search: ", Style::new().fg(Color::White)),
            Span::styled(query_visual, Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("▌", Style::new().fg(Color::White)),
        ]))
        .style(Style::new().bg(app.theme.selected_bg)),
        area,
    );
}

// ── Jump to Date ────────────────────────────────────────────────

fn render_jump_date(frame: &mut Frame, value: &str, cursor: usize, app: &App) {
    let area = frame.area();
    let popup = centered_rect(34, 5, area);

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Jump to Date ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(app.theme.active_border));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let byte_cursor = char_to_byte(value, cursor);
    let before = &value[..byte_cursor];
    let after = &value[byte_cursor..];

    let label = Span::styled(" Date: ", app.theme.accent_bold);
    let cursor_ch = after.chars().next().map(|c| c.to_string()).unwrap_or_else(|| " ".into());
    let after_rest = after.chars().skip(1).collect::<String>();

    let mut spans = vec![label];
    if !before.is_empty() {
        spans.push(Span::styled(bidi(before), Style::new().bg(Color::DarkGray).fg(Color::White)));
    }
    spans.push(Span::styled(cursor_ch, Style::new().bg(app.theme.active_border).fg(Color::Black)));
    if !after_rest.is_empty() {
        spans.push(Span::styled(bidi(&after_rest), Style::new().bg(Color::DarkGray).fg(Color::White)));
    }
    spans.push(Span::styled(
        "  (e.g. 2026-05-31, May 31, tomorrow, +3)",
        Style::new().fg(Color::DarkGray),
    ));

    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
}

// ── Event Detail popup ──────────────────────────────────────────

fn render_event_detail(frame: &mut Frame, event: &CalendarEvent, app: &App) {
    let area = frame.area();
    let popup = centered_rect(60, 12, area);

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Event Details ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(app.theme.active_border));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(" Title: ", Style::new().bold().fg(Color::Cyan)),
        Span::raw(bidi(&event.summary)),
    ]));
    if let Some(start) = event.start {
        let end_str = event.end.map(|e| e.format(" %H:%M").to_string()).unwrap_or_default();
        let time_str = bidi(&format!("{} —{}", start.format("%a %b %d, %H:%M"), end_str));
        lines.push(Line::from(vec![
            Span::styled(" Time:  ", Style::new().bold().fg(Color::Cyan)),
            Span::raw(time_str),
        ]));
    }
    if let Some(ref desc) = event.description
        && !desc.is_empty() {
            lines.push(Line::from(Span::styled(" Description: ", Style::new().bold().fg(Color::Cyan))));
            for line in desc.lines() {
                lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::raw(bidi(line)),
                ]));
            }
        }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(" [Esc] Close", Style::new().fg(Color::DarkGray))));

    let paragraph = Paragraph::new(Text::from(lines));
    frame.render_widget(paragraph, inner);
}

// ── Event list popup (for week view) ────────────────────────────

fn render_event_list_popup(frame: &mut Frame, events: &[CalendarEvent], cursor: usize, app: &App) {
    let area = frame.area();
    let height = (events.len() as u16 + 4).min(area.height.saturating_sub(4));
    let popup = centered_rect(50, height, area);

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Events ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(app.theme.active_border));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let items: Vec<ListItem> = events
        .iter()
        .enumerate()
        .map(|(i, event)| {
            let is_selected = i == cursor;
            let time = event.start.map(|s| s.format(" %H:%M").to_string()).unwrap_or_default();
            let title = bidi(&event.summary);
            let label = format!("{}{}", time, title);
            let style = if is_selected {
                Style::new()
                    .bg(app.theme.selected_bg)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(Color::White)
            };
            ListItem::new(label).style(style)
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, inner);
}


fn week_start(date: NaiveDate, first_day_of_week: u8) -> NaiveDate {
    let dow = if first_day_of_week == 0 {
        date.weekday().num_days_from_monday()
    } else {
        date.weekday().num_days_from_sunday()
    };
    date - chrono::Duration::days(dow as i64)
}

fn render_week_view(frame: &mut Frame, area: Rect, app: &App) -> Rect {
    let start = week_start(app.selected_date, app.first_day_of_week);

    let title = format!(" Week of {} ", start.format("%a, %b %d, %Y"));
    let is_focused = matches!(app.focus, Focus::Calendar);

    let block = Block::default()
        .title(Span::styled(title, Style::new().bold().fg(Color::White)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if is_focused {
            Style::new().fg(app.theme.active_border)
        } else {
            Style::new().fg(app.theme.inactive_border)
        })
        .title_alignment(Alignment::Left);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // 7 equal columns for the days
    let cols = Layout::horizontal([Constraint::Ratio(1, 7); 7]).areas::<7>(inner);

    let today = Local::now().naive_local().date();
    for (i, col) in cols.iter().enumerate() {
        let date = start + chrono::Duration::days(i as i64);
        let is_selected = date == app.selected_date;
        let is_today = date == today;

        let day_events: Vec<&CalendarEvent> = app.month_events
            .iter()
            .filter(|e| e.start.is_some_and(|s| s.date() == date))
            .collect();

        render_week_day(frame, *col, date, &day_events, is_selected, is_today, app);
    }

    inner
}

fn render_week_day(
    frame: &mut Frame,
    area: Rect,
    date: NaiveDate,
    events: &[&CalendarEvent],
    is_selected: bool,
    is_today: bool,
    app: &App,
) {
    let mut lines: Vec<Line> = Vec::new();

    // Day header — two lines
    let day_name = bidi(&date.format("%a").to_string());
    let day_num = date.format("%d").to_string();
    let header_style = if is_today {
        Style::new().bold().fg(Color::Yellow)
    } else if is_selected {
        Style::new().bold().fg(Color::White)
    } else {
        Style::new().fg(Color::Gray)
    };
    lines.push(Line::from(Span::styled(day_name, header_style)));
    lines.push(Line::from(Span::styled(day_num, header_style)));
    lines.push(Line::from(Span::styled(
        "─".repeat(area.width.saturating_sub(1) as usize),
        Style::new().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));

    // Events for the day
    if events.is_empty() {
        lines.push(Line::from(Span::styled(
            if area.width > 10 { "·" } else { "" },
            Style::new().fg(Color::DarkGray),
        )));
    } else {
        for event in events.iter().take(4) {
            let time = event.start.map(|s| s.format(" %H:%M").to_string()).unwrap_or_default();
            let title = bidi(&event.summary);
            let label = format!("{}{}", time, title);
            let truncated: String = label
                .chars()
                .take(area.width.saturating_sub(2) as usize)
                .collect();
            lines.push(Line::from(Span::raw(truncated)));
        }
        if events.len() > 4 {
            lines.push(Line::from(Span::styled(
                format!("+{} more", events.len() - 4),
                Style::new().fg(Color::DarkGray),
            )));
        }
    }

    let bg = if is_selected { app.theme.selected_bg } else { Color::Black };
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(Style::new().bg(bg)),
        area,
    );
}

// ── Form popup ──────────────────────────────────────────────────

fn render_form(frame: &mut Frame, title: &str, form: &FormState, app: &App) {
    let area = frame.area();
    let popup = centered_rect(52, 12, area);

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(app.theme.active_border));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let mut lines: Vec<Line> = Vec::new();

    for (i, field) in form.fields.iter().enumerate() {
        let is_focused = i == form.focus;

        let label = Span::styled(
            format!(" {}: ", field.label),
            if is_focused {
                app.theme.accent_bold
            } else {
                Style::new().fg(Color::Gray)
            },
        );

        let value_line = if is_focused {
            let byte_cursor = char_to_byte(&field.value, field.cursor);
            let before = &field.value[..byte_cursor];
            let after = &field.value[byte_cursor..];
            let mut spans = vec![label];
            if !before.is_empty() {
                spans.push(Span::styled(
                    bidi(before),
                    Style::new().bg(Color::DarkGray).fg(Color::White),
                ));
            }
            let cursor_ch = after.chars().next().map(|c| c.to_string()).unwrap_or_else(|| " ".into());
            spans.push(Span::styled(
                cursor_ch,
                Style::new().bg(app.theme.active_border).fg(Color::Black),
            ));
            let after_rest = after.chars().skip(1).collect::<String>();
            if !after_rest.is_empty() {
                spans.push(Span::styled(
                    bidi(&after_rest),
                    Style::new().bg(Color::DarkGray).fg(Color::White),
                ));
            }
            Line::from(spans)
        } else {
            Line::from(vec![
                label,
                Span::styled(bidi(&field.value), Style::new().fg(Color::Gray)),
            ])
        };

        lines.push(value_line);
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  [Enter] Save  ", Style::new().fg(Color::Green)),
        Span::styled("[Esc] Cancel", Style::new().fg(Color::Red)),
    ]));

    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(Style::new().bg(Color::Black)),
        inner,
    );
}

// ── Delete confirmation ─────────────────────────────────────────

fn render_delete_dialog(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let popup = centered_rect(44, 5, area);

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Delete Event ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::Red));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let name = app.selected_event().map(|e| e.summary.as_str()).unwrap_or("this event");
    let lines = vec![
        Line::from(Span::styled(
            format!(" Delete \"{}\"?", bidi(name)),
            Style::new().fg(Color::LightRed).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [Enter] Confirm  ", Style::new().fg(Color::Green)),
            Span::styled("[Esc] Cancel", Style::new().fg(Color::Gray)),
        ]),
    ];

    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(Style::new().bg(Color::Black)),
        inner,
    );
}

fn render_confirm_quit(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let popup = centered_rect(30, 5, area);

    frame.render_widget(Clear, popup);

    let inner = popup.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(app.theme.active_border)
        .title(" Quit ");

    frame.render_widget(block, popup);

    let lines = vec![
        Line::from(Span::styled(
            " Really quit? ",
            Style::new().fg(Color::LightRed).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [y] Yes  ", Style::new().fg(Color::Green)),
            Span::styled("[n] No", Style::new().fg(Color::Gray)),
        ]),
    ];

    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(Style::new().bg(Color::Black)),
        inner,
    );
}

// ── Help overlay ────────────────────────────────────────────────

fn render_help(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let popup = centered_rect(52, 16, area);

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(app.theme.active_border));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let help_text = Text::from(vec![
        Line::from(Span::styled("  Navigation", Style::new().bold().fg(Color::Cyan))),
        Line::from(Span::raw("  Arrows    Move between days / menu items")),
        Line::from(Span::raw("  [/]       Previous / Next month")),
        Line::from(Span::raw("  w         Toggle Week / Month view")),
        Line::from(Span::raw("  Tab       Cycle Calendar / Events")),
        Line::from(Span::raw("  Enter     Context menu")),
        Line::from(Span::raw("  Esc       Quit / Close")),
        Line::from(Span::raw("")),
        Line::from(Span::styled("  Actions", Style::new().bold().fg(Color::Cyan))),
        Line::from(Span::raw("  e     Edit event        d   Delete event")),
        Line::from(Span::raw("  ?     Help              s   Settings")),
        Line::from(Span::raw("  t     Go to Today       j   Jump to Date")),
        Line::from(Span::raw("  n     New Event         /   Search")),
        Line::from(Span::raw("  q     Quit")),
    ]);

    frame.render_widget(
        Paragraph::new(help_text).style(Style::new().bg(Color::Black)),
        inner,
    );
}

// ── Menu dropdown ──────────────────────────────────────────────

fn render_menu_dropdown(frame: &mut Frame, app: &App) {
    let area = frame.area();

    let max_len = app
        .menu_items
        .iter()
        .map(|i| i.label.len())
        .max()
        .unwrap_or(10);
    let width = (max_len + 4).max(12) as u16;
    let height = app.menu_items.len() as u16 + 2;

    let dropdown_area = Rect::new(
        area.width.saturating_sub(width) / 2,
        area.height.saturating_sub(height) / 2,
        width.min(area.width),
        height.min(area.height),
    );

    frame.render_widget(Clear, dropdown_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(app.theme.active_border));

    let inner = block.inner(dropdown_area);
    frame.render_widget(block, dropdown_area);

    let items: Vec<ListItem> = app
        .menu_items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_selected = i == app.menu_cursor;
            let style = if !item.enabled {
                Style::new().fg(Color::DarkGray)
            } else if is_selected {
                Style::new()
                    .bg(app.theme.selected_bg)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(Color::White)
            };
            ListItem::new(item.label.as_str()).style(style)
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, inner);
}

// ── Settings overlay ────────────────────────────────────────────

fn render_settings(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let popup = centered_rect(64, 23, area);

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Settings ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(app.theme.active_border));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let header = Style::new().bold().fg(app.theme.active_border);
    let val = Style::new().fg(Color::White);
    let dim = Style::new().fg(Color::DarkGray);
    let action = Style::new().fg(Color::Green).add_modifier(Modifier::BOLD);
    let green = Style::new().fg(Color::Green);
    let focus_bg = Style::new().bg(app.theme.selected_bg).add_modifier(Modifier::BOLD);
    let creds_exist = app.config_credentials_path.exists();
    let token_exists = app.config_token_path.exists();
    let cal_registered = crate::app::App::is_cal_registered();

    let mut lines = Vec::new();
    lines.push(Line::from(""));

    // ── Connections ──
    lines.push(Line::from(Span::styled(" Connections", header)));

    let local_focused = app.settings_focus == 0;
    lines.push(Line::from(vec![
        Span::styled("  ✓ ", green),
        Span::styled("Local Storage", val),
    ]).style(if local_focused { focus_bg } else { Style::new() }));

    let google_focused = app.settings_focus == 1;
    if token_exists {
        lines.push(Line::from(vec![
            Span::styled("  ✓ ", green),
            Span::styled("Google Calendar", val),
            Span::raw("  "),
            Span::styled("(sign out)", action),
        ]).style(if google_focused { focus_bg } else { Style::new() }));
    } else if creds_exist {
        lines.push(Line::from(vec![
            Span::styled("  ○ ", dim),
            Span::styled("Google Calendar", dim),
            Span::raw("  "),
            Span::styled("(sign in)", action),
        ]).style(if google_focused { focus_bg } else { Style::new() }));
    } else {
        lines.push(Line::from(vec![
            Span::styled("  ○ ", dim),
            Span::styled("Google Calendar", dim),
            Span::raw("  "),
            Span::styled("(no credentials)", dim),
        ]).style(if google_focused { focus_bg } else { Style::new() }));
    }

    lines.push(Line::from(""));

    // ── Calendar ──
    lines.push(Line::from(Span::styled(" Calendar", header)));

    let dow_focused = app.settings_focus == 2;
    let dow_label = if app.first_day_of_week == 0 { "Monday" } else { "Sunday" };
    lines.push(Line::from(vec![
        Span::styled("  Start week on: ", val),
        Span::styled(dow_label, action),
    ]).style(if dow_focused { focus_bg } else { Style::new() }));

    lines.push(Line::from(""));

    // ── Appearance ──
    lines.push(Line::from(Span::styled(" Appearance", header)));

    let theme_focused = app.settings_focus == 3;
    let theme_name = match app.theme_kind {
        crate::app::ThemeKind::Default => "Default",
        crate::app::ThemeKind::Light => "Light",
        crate::app::ThemeKind::Dracula => "Dracula",
        crate::app::ThemeKind::Nord => "Nord",
        crate::app::ThemeKind::Gruvbox => "Gruvbox",
    };
    lines.push(Line::from(vec![
        Span::styled("  Theme: ", val),
        Span::styled(theme_name, action),
    ]).style(if theme_focused { focus_bg } else { Style::new() }));

    let dance_focused = app.settings_focus == 4;
    let dance_name = match app.dance_style {
        crate::app::DanceStyle::None => "None",
        crate::app::DanceStyle::Dancer => "Dancer",
        crate::app::DanceStyle::Bounce => "Bounce",
        crate::app::DanceStyle::Sway => "Sway",
        crate::app::DanceStyle::Shrug => "Shrug",
    };
    lines.push(Line::from(vec![
        Span::styled("  Dance: ", val),
        Span::styled(dance_name, action),
    ]).style(if dance_focused { focus_bg } else { Style::new() }));

    let sub = match app.dance_style {
        crate::app::DanceStyle::None => String::new(),
        _ => {
            let frames = app.dance_style.frames();
            frames[app.frame % frames.len()].to_string()
        }
    };
    if !sub.is_empty() {
        lines.last_mut().unwrap().push_span(Span::raw(format!("  {}", sub)));
    }

    lines.push(Line::from(""));

    // ── Shell ──
    lines.push(Line::from(Span::styled(" Shell", header)));

    let cal_focused = app.settings_focus == 5;
    if cal_registered {
        lines.push(Line::from(vec![
            Span::styled("  ✓ ", green),
            Span::styled("cal command", val),
            Span::raw("  "),
            Span::styled("(unregister)", action),
        ]).style(if cal_focused { focus_bg } else { Style::new() }));
    } else {
        lines.push(Line::from(vec![
            Span::styled("  ○ ", dim),
            Span::styled("cal command", dim),
            Span::raw("  "),
            Span::styled("(register)", action),
        ]).style(if cal_focused { focus_bg } else { Style::new() }));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  [\u{2191}/\u{2193}] Navigate  ", Style::new().fg(Color::Green)),
        Span::styled("[Enter] Toggle  ", Style::new().fg(Color::Green)),
        Span::styled("[Esc] Back", dim),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            format!(" calendar-cli v{}", env!("CARGO_PKG_VERSION")),
            dim.add_modifier(Modifier::ITALIC),
        ),
    ]));

    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(Style::new().bg(Color::Black)),
        inner,
    );
}

// ── Auth dialog ─────────────────────────────────────────────────

fn render_auth_dialog(frame: &mut Frame, message: &str, app: &App) {
    let area = frame.area();
    let popup = centered_rect(60, 8, area);

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Google Sign-In ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(app.theme.active_border));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let is_loading = !message.starts_with('✓') && !message.starts_with('✗');

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            if is_loading {
                Span::styled(message, Style::new().fg(Color::Cyan))
            } else if message.starts_with('✓') {
                Span::styled(message, Style::new().fg(Color::Green).add_modifier(Modifier::BOLD))
            } else {
                Span::styled(message, Style::new().fg(Color::Red))
            },
        ]),
        Line::from(""),
        Line::from(if is_loading {
            Span::styled("  Your browser should open automatically.", Style::new().fg(Color::DarkGray))
        } else {
            Span::styled("  [Esc] Close", Style::new().fg(Color::DarkGray))
        }),
    ];

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(Style::new().bg(Color::Black))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

// ── Layout helper ───────────────────────────────────────────────

fn centered_rect(width_pct: u16, height: u16, area: Rect) -> Rect {
    let [_, v, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas::<3>(area);
    let [_, h, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Percentage(width_pct),
        Constraint::Fill(1),
    ])
    .areas::<3>(v);
    h
}
