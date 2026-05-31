use std::path::PathBuf;
use std::time::Duration;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use anyhow::Result;
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::backend::Backend;
use ratatui::style::{Color, Modifier, Style};
use ratatui::Terminal;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::auth::parse_redirect;
use crate::auth::GoogleAuth;
use crate::backend::CalendarBackend;
use crate::config::{Config, Settings};
use crate::gcal::GoogleCalendar;
use crate::local::LocalCalendar;
use crate::models::{CalendarEvent, FormState};
use crate::ui;

// ── Auth state ──────────────────────────────────────────────────

pub enum AuthState {
    Idle,
    Listening { listener: TcpListener, csrf: String },
    Message(String),
}

// ── Modes ───────────────────────────────────────────────────────

pub enum Mode {
    Normal,
    Creating(FormState),
    Editing(FormState),
    Deleting,
    Help,
    Settings,
}

#[derive(PartialEq)]
pub enum Focus {
    MenuBar,
    Calendar,
    EventList,
}

#[derive(Clone)]
pub struct ContextItem {
    pub label: String,
    pub action: MenuAction,
    pub enabled: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub enum MenuAction {
    Quit,
    Settings,
    Today,
    NewEvent,
    EditEvent,
    DeleteEvent,
    ViewDetail,
    Search,
    SignIn,
    SignOut,
    Help,
    None,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ThemeKind {
    Default,
    Light,
    Ocean,
}

pub struct Theme {
    pub selected_bg: Color,
    pub today: Style,
    pub active_border: Color,
    pub inactive_border: Color,
    pub weekend: Color,
    pub help_key: Style,
    pub dim: Style,
    pub title_bg: Color,
    pub accent_bold: Style,
}

impl ThemeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThemeKind::Default => "default",
            ThemeKind::Light => "light",
            ThemeKind::Ocean => "ocean",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "light" => ThemeKind::Light,
            "ocean" => ThemeKind::Ocean,
            _ => ThemeKind::Default,
        }
    }
}

impl Theme {
    pub fn for_kind(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Default => Self {
                selected_bg: Color::Blue,
                today: Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                active_border: Color::Cyan,
                inactive_border: Color::DarkGray,
                weekend: Color::Gray,
                help_key: Style::new().fg(Color::Cyan),
                dim: Style::new().fg(Color::DarkGray),
                title_bg: Color::DarkGray,
                accent_bold: Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            },
            ThemeKind::Light => Self {
                selected_bg: Color::LightBlue,
                today: Style::new().fg(Color::LightRed).add_modifier(Modifier::BOLD),
                active_border: Color::Black,
                inactive_border: Color::Gray,
                weekend: Color::Gray,
                help_key: Style::new().fg(Color::Black),
                dim: Style::new().fg(Color::Gray),
                title_bg: Color::White,
                accent_bold: Style::new().fg(Color::Black).add_modifier(Modifier::BOLD),
            },
            ThemeKind::Ocean => Self {
                selected_bg: Color::Rgb(30, 70, 110),
                today: Style::new().fg(Color::LightYellow).add_modifier(Modifier::BOLD),
                active_border: Color::LightCyan,
                inactive_border: Color::DarkGray,
                weekend: Color::DarkGray,
                help_key: Style::new().fg(Color::LightCyan),
                dim: Style::new().fg(Color::DarkGray),
                title_bg: Color::Rgb(20, 30, 50),
                accent_bold: Style::new().fg(Color::LightCyan).add_modifier(Modifier::BOLD),
            },
        }
    }
}

#[must_use]
pub enum Action {
    None,
    Quit,
    RefreshEvents,
}

// ── App ─────────────────────────────────────────────────────────

pub struct App {
    backend: Box<dyn CalendarBackend>,
    pub mode: Mode,
    pub view_date: NaiveDate,
    pub selected_date: NaiveDate,
    /// Events for the currently selected day (filtered from month_events)
    pub events: Vec<CalendarEvent>,
    /// All events for the currently loaded month (cache)
    pub month_events: Vec<CalendarEvent>,
    pub event_focus: usize,
    pub focus: Focus,
    pub status: String,
    pub events_loaded: bool,
    pub loading: bool,
    needs_refresh: bool,
    last_loaded_month: Option<(i32, u32)>,
    pub config_credentials_path: PathBuf,
    pub config_token_path: PathBuf,
    pub config_events_path: PathBuf,
    pub settings_path: PathBuf,
    pub auth_state: AuthState,
    pub settings_focus: usize,
    pub first_day_of_week: u8,
    pub menu_bar_focus: usize,
    pub menu_open: bool,
    pub menu_items: Vec<ContextItem>,
    pub menu_cursor: usize,
    pub search_query: Option<String>,
    pub theme_kind: ThemeKind,
    pub theme: Theme,
}

impl App {
    pub fn new(backend: Box<dyn CalendarBackend>, config: &Config) -> Self {
        let today = Local::now().naive_local().date();
        let settings = config.load_settings();
        let theme_kind = settings.theme_kind();
        let theme = Theme::for_kind(theme_kind);
        Self {
            backend,
            mode: Mode::Normal,
            view_date: NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap(),
            selected_date: today,
            events: Vec::new(),
            month_events: Vec::new(),
            event_focus: 0,
            focus: Focus::Calendar,
            status: String::new(),
            events_loaded: false,
            loading: false,
            needs_refresh: false,
            last_loaded_month: None,
            config_credentials_path: config.credentials_path.clone(),
            config_token_path: config.token_path.clone(),
            config_events_path: config.events_path(),
            settings_path: config.settings_path.clone(),
            auth_state: AuthState::Idle,
            settings_focus: 0,
            first_day_of_week: settings.first_day_of_week,
            menu_bar_focus: 0,
            menu_open: false,
            menu_items: Vec::new(),
            menu_cursor: 0,
            search_query: None,
            theme_kind,
            theme,
        }
    }

    pub async fn run<B: Backend + 'static>(&mut self, terminal: &mut Terminal<B>) -> Result<()>
    where
        B::Error: Send + Sync + std::error::Error + 'static,
    {
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

        std::thread::spawn(move || {
            loop {
                match crossterm::event::read() {
                    Ok(event) => {
                        if event_tx.send(event).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        self.refresh_events().await?;

        loop {
            self.try_complete_auth().await;

            // Deferred refresh: draw loading state, then fetch
            if self.needs_refresh {
                self.loading = true;
                terminal.draw(|f| ui::render(f, self))?;
                let _ = self.refresh_events_inner().await;
                self.loading = false;
                self.needs_refresh = false;
            }

            terminal.draw(|f| ui::render(f, self))?;

            // Timeout allows the clock in the menu bar to update live
            let event = tokio::time::timeout(Duration::from_millis(500), event_rx.recv())
                .await
                .ok()
                .flatten();

            if let Some(event) = event {
                use crossterm::event::Event;
                match event {
                    Event::Key(key) => {
                        if let Action::Quit = self.handle_key(key).await? {
                            break;
                        }
                    }
                    Event::Resize(_, _) => {}
                    _ => {}
                }
            }
        }

        Ok(())
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<Action> {
        if matches!(key.code, KeyCode::Esc) && matches!(self.auth_state, AuthState::Message(_)) {
            self.auth_state = AuthState::Idle;
        }
        match &mut self.mode {
            Mode::Normal => self.handle_normal_key(key).await,
            Mode::Creating(form) => {
                if handle_form_key(key, form) {
                    self.save_event().await
                } else if is_cancel(key) {
                    self.mode = Mode::Normal;
                    Ok(Action::None)
                } else {
                    Ok(Action::None)
                }
            }
            Mode::Editing(form) => {
                if handle_form_key(key, form) {
                    self.update_event().await
                } else if is_cancel(key) {
                    self.mode = Mode::Normal;
                    Ok(Action::None)
                } else {
                    Ok(Action::None)
                }
            }
            Mode::Deleting => match key.code {
                KeyCode::Enter => self.delete_event().await,
                KeyCode::Esc => {
                    self.mode = Mode::Normal;
                    Ok(Action::None)
                }
                _ => Ok(Action::None),
            },
            Mode::Help => {
                if matches!(key.code, KeyCode::Esc) {
                    self.mode = Mode::Normal;
                }
                Ok(Action::None)
            }
            Mode::Settings => {
                match key.code {
                    KeyCode::Esc => {
                        self.auth_state = AuthState::Idle;
                        self.mode = Mode::Normal;
                    }
                    KeyCode::Down | KeyCode::Tab => {
                        self.settings_focus = (self.settings_focus + 1).min(4);
                    }
                    KeyCode::Up | KeyCode::BackTab => {
                        self.settings_focus = self.settings_focus.saturating_sub(1);
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => match self.settings_focus {
                        0 => {}
                        1 => {
                            if self.config_token_path.exists() {
                                self.do_sign_out_google();
                                self.status = "✓ Signed out of Google Calendar".into();
                                self.refresh_events().await?;
                            } else if self.config_credentials_path.exists()
                                && !matches!(self.auth_state, AuthState::Listening { .. })
                            {
                                self.start_auth().await;
                            } else {
                                self.status =
                                    "No credentials.json found in ~/.config/calendar-cli/".into();
                            }
                        }
                        2 => {
                            self.first_day_of_week = if self.first_day_of_week == 0 { 1 } else { 0 };
                            let settings = Settings {
                                first_day_of_week: self.first_day_of_week,
                                theme: self.theme_kind.as_str().to_string(),
                            };
                            if let Ok(s) = serde_json::to_string_pretty(&settings) {
                                let _ = std::fs::write(&self.settings_path, s);
                            }
                            self.refresh_events().await?;
                        }
                        3 => {
                            let kinds = [ThemeKind::Default, ThemeKind::Light, ThemeKind::Ocean];
                            let idx = kinds.iter().position(|k| *k == self.theme_kind).unwrap_or(0);
                            self.theme_kind = kinds[(idx + 1) % kinds.len()];
                            self.theme = Theme::for_kind(self.theme_kind);
                            let settings = Settings {
                                first_day_of_week: self.first_day_of_week,
                                theme: self.theme_kind.as_str().to_string(),
                            };
                            if let Ok(s) = serde_json::to_string_pretty(&settings) {
                                let _ = std::fs::write(&self.settings_path, s);
                            }
                        }
                        4 => {
                            if Self::is_cal_registered() {
                                if let Err(e) = Self::unregister_cal() {
                                    self.status = format!("✗ Failed to unregister cal: {}", e);
                                } else {
                                    self.status = "✓ cal command unregistered".into();
                                }
                            } else {
                                match Self::register_cal() {
                                    Ok(msg) => self.status = msg,
                                    Err(e) => self.status = format!("✗ Failed to register cal: {}", e),
                                }
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
                Ok(Action::None)
            }
        }
    }

    async fn handle_normal_key(&mut self, key: KeyEvent) -> Result<Action> {
        // If menu overlay is open, handle menu navigation
        if self.menu_open {
            return self.handle_menu_key(key).await;
        }

        // Search mode: intercept keys for building query
        if self.search_query.is_some() {
            return self.handle_search_key(key);
        }

        match key.code {
            KeyCode::Char('/') => {
                self.search_query = Some(String::new());
                self.apply_search_filter();
            }
            // Tab cycles focus
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::MenuBar => Focus::Calendar,
                    Focus::Calendar => Focus::EventList,
                    Focus::EventList => Focus::MenuBar,
                };
            }
            KeyCode::BackTab => {
                self.focus = match self.focus {
                    Focus::MenuBar => Focus::EventList,
                    Focus::Calendar => Focus::MenuBar,
                    Focus::EventList => Focus::Calendar,
                };
            }
            KeyCode::Esc => return Ok(Action::Quit),

            _ => match self.focus {
                Focus::MenuBar => self.handle_bar_key(key),
                Focus::Calendar => self.handle_calendar_key(key).await?,
                Focus::EventList => self.handle_eventlist_key(key)?,
            },
        }
        Ok(Action::None)
    }

    fn handle_search_key(&mut self, key: KeyEvent) -> Result<Action> {
        match key.code {
            KeyCode::Esc => {
                self.search_query = None;
                self.filter_events_for_date(self.selected_date);
            }
            KeyCode::Enter => {
                if !self.events.is_empty() && self.event_focus < self.events.len() {
                    let event = self.events[self.event_focus].clone();
                    self.search_query = None;
                    if let Some(start) = event.start {
                        let new_date = start.date();
                        self.selected_date = new_date;
                        self.view_date = NaiveDate::from_ymd_opt(
                            new_date.year(),
                            new_date.month(),
                            1,
                        ).unwrap();
                        self.event_focus = 0;
                        self.needs_refresh = true;
                        self.focus = Focus::Calendar;
                    }
                } else {
                    self.search_query = None;
                }
            }
            KeyCode::Up => {
                if !self.events.is_empty() {
                    self.event_focus = self.event_focus.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if !self.events.is_empty() {
                    self.event_focus = (self.event_focus + 1).min(self.events.len() - 1);
                }
            }
            KeyCode::Backspace => {
                if let Some(query) = &mut self.search_query {
                    query.pop();
                }
                self.apply_search_filter();
            }
            KeyCode::Char(c) if !c.is_control() => {
                if let Some(query) = &mut self.search_query {
                    query.push(c);
                }
                self.apply_search_filter();
            }
            _ => {}
        }
        Ok(Action::None)
    }

    fn handle_bar_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left => {
                self.menu_bar_focus = self.menu_bar_focus.saturating_sub(1);
            }
            KeyCode::Right => {
                self.menu_bar_focus = (self.menu_bar_focus + 1).min(3);
            }
            KeyCode::Down | KeyCode::Enter => {
                self.open_menu_for_bar();
            }
            _ => {}
        }
    }

    fn open_menu_for_bar(&mut self) {
        self.menu_items = self.bar_menu_items();
        if !self.menu_items.is_empty() && self.menu_items.iter().any(|i| i.enabled) {
            self.menu_cursor = self.menu_items.iter().position(|i| i.enabled).unwrap_or(0);
            self.menu_open = true;
        }
    }

    async fn handle_calendar_key(&mut self, key: KeyEvent) -> Result<()> {
        macro_rules! mv {
            ($days:expr) => {{
                self.move_by_days($days);
                self.needs_refresh = true;
            }};
        }
        match key.code {
            KeyCode::Left => mv!(-1),
            KeyCode::Right => mv!(1),
            KeyCode::Up => mv!(-7),
            KeyCode::Down => mv!(7),
            KeyCode::Char('[') | KeyCode::PageUp => {
                self.view_date = prev_month(self.view_date);
                self.clamp_selected_date();
                self.needs_refresh = true;
            }
            KeyCode::Char(']') | KeyCode::PageDown => {
                self.view_date = next_month(self.view_date);
                self.clamp_selected_date();
                self.needs_refresh = true;
            }
            KeyCode::Enter => {
                self.menu_items = self.date_context_items();
                self.menu_cursor = 0;
                self.menu_open = true;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_eventlist_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Up => {
                if !self.events.is_empty() {
                    self.event_focus = self.event_focus.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if !self.events.is_empty() {
                    self.event_focus = (self.event_focus + 1).min(self.events.len() - 1);
                }
            }
            KeyCode::Enter => {
                if !self.events.is_empty() {
                    self.menu_items = self.event_context_items();
                    self.menu_cursor = 0;
                    self.menu_open = true;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_menu_key(&mut self, key: KeyEvent) -> Result<Action> {
        match key.code {
            KeyCode::Up => {
                if !self.menu_items.is_empty() {
                    self.menu_cursor = self.menu_cursor.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if !self.menu_items.is_empty() {
                    self.menu_cursor = (self.menu_cursor + 1).min(self.menu_items.len() - 1);
                }
            }
            KeyCode::Enter => {
                if self.menu_cursor < self.menu_items.len() {
                    let item = self.menu_items[self.menu_cursor].clone();
                    self.menu_open = false;
                    if item.enabled {
                        return self.execute_menu_action(item.action).await;
                    }
                }
            }
            KeyCode::Esc => {
                self.menu_open = false;
            }
            _ => {}
        }
        Ok(Action::None)
    }

    fn bar_menu_items(&self) -> Vec<ContextItem> {
        match self.menu_bar_focus {
            0 => vec![
                ContextItem { label: "Settings".into(), action: MenuAction::Settings, enabled: true },
                ContextItem { label: "Quit".into(), action: MenuAction::Quit, enabled: true },
            ],
            1 => vec![
                ContextItem { label: "Go to Today".into(), action: MenuAction::Today, enabled: true },
                ContextItem { label: "New Event".into(), action: MenuAction::NewEvent, enabled: true },
                ContextItem { label: "Search Events".into(), action: MenuAction::Search, enabled: true },
            ],
            2 => {
                if self.config_token_path.exists() {
                    vec![ContextItem { label: "Sign Out of Google".into(), action: MenuAction::SignOut, enabled: true }]
                } else if self.config_credentials_path.exists() {
                    vec![ContextItem { label: "Sign In to Google".into(), action: MenuAction::SignIn, enabled: true }]
                } else {
                    vec![ContextItem { label: "No credentials available".into(), action: MenuAction::None, enabled: false }]
                }
            }
            3 => vec![
                ContextItem { label: "About / Keybindings".into(), action: MenuAction::Help, enabled: true },
            ],
            _ => vec![],
        }
    }

    fn date_context_items(&self) -> Vec<ContextItem> {
        let mut items = vec![
            ContextItem { label: "New Event".into(), action: MenuAction::NewEvent, enabled: true },
        ];
        if self.selected_date != Local::now().naive_local().date() {
            items.push(ContextItem { label: "Go to Today".into(), action: MenuAction::Today, enabled: true });
        }
        items
    }

    fn event_context_items(&self) -> Vec<ContextItem> {
        let mut items = vec![
            ContextItem { label: "View Detail".into(), action: MenuAction::ViewDetail, enabled: true },
        ];
        if self.selected_event().is_some() {
            items.push(ContextItem { label: "Edit".into(), action: MenuAction::EditEvent, enabled: true });
            items.push(ContextItem { label: "Delete".into(), action: MenuAction::DeleteEvent, enabled: true });
        }
        items
    }

    async fn execute_menu_action(&mut self, action: MenuAction) -> Result<Action> {
        match action {
            MenuAction::Quit => return Ok(Action::Quit),
            MenuAction::Settings => {
                self.settings_focus = 0;
                self.mode = Mode::Settings;
            }
            MenuAction::Today => {
                let today = Local::now().naive_local().date();
                self.selected_date = today;
                self.view_date = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
                self.event_focus = 0;
                return self.and_refresh().await;
            }
            MenuAction::NewEvent => {
                let form = FormState::new(self.selected_date);
                self.mode = Mode::Creating(form);
            }
            MenuAction::EditEvent => {
                if let Some(event) = self.selected_event() {
                    let form = FormState::from_event(event);
                    self.mode = Mode::Editing(form);
                }
            }
            MenuAction::DeleteEvent => {
                if self.selected_event().is_some() {
                    self.mode = Mode::Deleting;
                }
            }
            MenuAction::ViewDetail => {
                if let Some(event) = self.selected_event() {
                    let time = event
                        .start
                        .map(|t| t.format("%H:%M").to_string())
                        .unwrap_or_default();
                    let desc = event
                        .description
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .map(|s| format!(" — {}", s))
                        .unwrap_or_default();
                    self.status = format!("{} — {}{}", event.summary, time, desc);
                }
            }
            MenuAction::SignIn => {
                if self.config_credentials_path.exists()
                    && !matches!(self.auth_state, AuthState::Listening { .. })
                {
                    self.start_auth().await;
                }
            }
            MenuAction::SignOut => {
                self.do_sign_out_google();
                self.status = "✓ Signed out of Google Calendar".into();
                self.refresh_events().await?;
            }
            MenuAction::Search => {
                self.search_query = Some(String::new());
                self.focus = Focus::Calendar;
                self.apply_search_filter();
            }
            MenuAction::Help => {
                self.mode = Mode::Help;
            }
            MenuAction::None => {}
        }
        Ok(Action::None)
    }

    // ── Auth ─────────────────────────────────────────────────

    /// Start the OAuth flow: load credentials, open browser, bind TCP listener.
    async fn start_auth(&mut self) {
        let auth = match GoogleAuth::load(
            &self.config_credentials_path,
            self.config_token_path.clone(),
        )
        .await
        {
            Ok(a) => a,
            Err(e) => {
                self.auth_state =
                    AuthState::Message(format!("✗ Failed to load credentials: {}", e));
                return;
            }
        };

        let (url, csrf) = auth.generate_auth_url();
        let _ = std::process::Command::new("open").arg(&url).spawn();

        let listener = match TcpListener::bind("127.0.0.1:8080").await {
            Ok(l) => l,
            Err(e) => {
                self.auth_state =
                    AuthState::Message(format!("✗ Failed to bind port 8080: {}", e));
                return;
            }
        };

        self.auth_state = AuthState::Listening { listener, csrf };
    }

    /// Non-blocking: try to accept a redirect on the TCP listener.
    async fn try_complete_auth(&mut self) {
        let (listener, csrf) = match std::mem::replace(&mut self.auth_state, AuthState::Idle) {
            AuthState::Listening { listener, csrf } => (listener, csrf),
            _ => return,
        };

        // Try non-blocking accept
        let mut stream = match tokio::time::timeout(Duration::from_millis(0), listener.accept()).await
        {
            Ok(Ok((stream, _))) => stream,
            _ => {
                self.auth_state = AuthState::Listening { listener, csrf };
                return;
            }
        };

        // Read the HTTP redirect
        let mut buf = vec![0; 4096];
        let n = match stream.read(&mut buf).await {
            Ok(n) => n,
            Err(_) => {
                self.auth_state = AuthState::Message("✗ Failed to read OAuth redirect".into());
                return;
            }
        };
        let request = String::from_utf8_lossy(&buf[..n]);

        let (code, state) = match parse_redirect(&request) {
            Ok(pair) => pair,
            Err(e) => {
                self.auth_state = AuthState::Message(format!("✗ Invalid redirect: {}", e));
                return;
            }
        };

        if state != csrf {
            self.auth_state = AuthState::Message("✗ CSRF mismatch — possible attack".into());
            return;
        }

        // Send success response to browser
        let response = "\
            HTTP/1.1 200 OK\r\n\
            Content-Type: text/html; charset=utf-8\r\n\
            Content-Length: 101\r\n\r\n\
            <html><body><h1>Authorization successful!</h1>\
            <p>You can close this window and return to the terminal.</p></body></html>";
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.flush().await;

        // Exchange code and switch backend
        match self.complete_auth(&code).await {
            Ok(()) => {
                self.auth_state =
                    AuthState::Message("✓ Successfully signed in to Google Calendar!".into());
                self.mode = Mode::Normal;
            }
            Err(e) => {
                self.auth_state = AuthState::Message(format!("✗ {}", e));
            }
        }
    }

    /// Exchange authorization code for a token and switch to Google backend.
    async fn complete_auth(&mut self, code: &str) -> Result<()> {
        let auth = GoogleAuth::load(&self.config_credentials_path, self.config_token_path.clone())
            .await?;
        // Skip CSRF check here — already verified in try_complete_auth
        let token_response = auth.exchange_code_raw(code).await?;
        auth.store_token(&token_response)?;

        let auth = GoogleAuth::load(&self.config_credentials_path, self.config_token_path.clone())
            .await?;
        self.backend = Box::new(GoogleCalendar::new(auth));
        self.refresh_events().await?;
        Ok(())
    }

    pub fn do_sign_out_google(&mut self) {
        let _ = std::fs::remove_file(&self.config_token_path);
        self.backend = Box::new(LocalCalendar::new(self.config_events_path.clone()));
    }

    // ── Cal command registration ──────────────────────────────

    fn cal_wrapper_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".local")
            .join("bin")
            .join("cal")
    }

    pub fn is_cal_registered() -> bool {
        // Check ~/.local/bin first
        let path = Self::cal_wrapper_path();
        if std::fs::read_to_string(&path).ok().map_or(false, |s| s.contains("calendar-cli")) {
            return true;
        }
        // Also check PATH dirs
        if let Some(path_var) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path_var) {
                let candidate = dir.join("cal");
                if candidate.exists() {
                    if let Ok(content) = std::fs::read_to_string(&candidate) {
                        if content.contains("calendar-cli") {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    pub fn register_cal() -> anyhow::Result<String> {
        let exe = std::env::current_exe()?;
        let wrapper = format!("#!/bin/sh\nexec {} \"$@\"\n", exe.display());

        // Find a writable directory that's in PATH
        let path_var = std::env::var_os("PATH").unwrap_or_default();
        let mut installed = None;

        // Try ~/.local/bin first (always writable)
        let local_bin = dirs::home_dir().unwrap_or_default().join(".local").join("bin");
        let local_path = local_bin.join("cal");
        std::fs::create_dir_all(&local_bin)?;
        std::fs::write(&local_path, &wrapper)?;
        #[cfg(unix)]
        std::fs::set_permissions(&local_path, std::fs::Permissions::from_mode(0o755))?;

        let local_in_path = std::env::split_paths(&path_var).any(|d| d == local_bin);
        if local_in_path {
            installed = Some("~/.local/bin/cal".to_string());
        }

        // Also try to install to a PATH directory (whichever comes first and is writable)
        for dir in std::env::split_paths(&path_var) {
            if dir == local_bin { continue; }
            if !dir.is_absolute() { continue; }
            let candidate = dir.join("cal");
            // Skip if it already exists and isn't ours
            if candidate.exists() && !std::fs::read_to_string(&candidate).ok()
                .map_or(false, |s| s.contains("calendar-cli"))
            {
                continue;
            }
            // Try to create the wrapper
            if std::fs::write(&candidate, &wrapper).is_ok() {
                #[cfg(unix)]
                let _ = std::fs::set_permissions(&candidate, std::fs::Permissions::from_mode(0o755));
                installed = Some(format!("{}", candidate.display()));
                break;
            }
        }

        match installed {
            Some(path) => Ok(format!("✓ cal command registered at {}", path)),
            None => Ok(format!(
                "✓ cal registered at ~/.local/bin/cal  (add ~/.local/bin to your PATH)"
            )),
        }
    }

    pub fn unregister_cal() -> anyhow::Result<()> {
        // Remove from ~/.local/bin
        let path = Self::cal_wrapper_path();
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        // Also remove from any PATH dir if it's our wrapper
        if let Some(path_var) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path_var) {
                let candidate = dir.join("cal");
                if candidate.exists() {
                    if let Ok(content) = std::fs::read_to_string(&candidate) {
                        if content.contains("calendar-cli") {
                            let _ = std::fs::remove_file(&candidate);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    // ── Movement ─────────────────────────────────────────────

    fn move_by_days(&mut self, days: i64) {
        let new = self.selected_date + chrono::Duration::days(days);
        self.selected_date = new;
        if new.month() != self.view_date.month() || new.year() != self.view_date.year() {
            self.view_date =
                NaiveDate::from_ymd_opt(new.year(), new.month(), 1).unwrap();
        }
        self.event_focus = 0;
    }

    async fn and_refresh(&mut self) -> Result<Action> {
        self.needs_refresh = true;
        Ok(Action::None)
    }

    fn clamp_selected_date(&mut self) {
        let last = num_days_in_month(self.view_date.year(), self.view_date.month());
        let d = self.selected_date.day().min(last);
        self.selected_date =
            NaiveDate::from_ymd_opt(self.view_date.year(), self.view_date.month(), d).unwrap();
    }

    // ── Backend operations ───────────────────────────────────

    pub async fn refresh_events(&mut self) -> Result<()> {
        let range_end = last_day_of_month(self.view_date);
        let month_events = self
            .backend
            .list_events_range(self.view_date, range_end)
            .await?;
        self.month_events = month_events;
        self.last_loaded_month = Some((self.view_date.year(), self.view_date.month()));
        self.filter_events_for_date(self.selected_date);
        Ok(())
    }

    fn filter_events_for_date(&mut self, date: NaiveDate) {
        self.events = self
            .month_events
            .iter()
            .filter(|e| {
                match (e.start, e.end) {
                    (Some(s), Some(e)) => s.date() <= date && date <= e.date(),
                    (Some(s), None) => s.date() == date,
                    (None, _) => false,
                }
            })
            .cloned()
            .collect();
        self.event_focus = self.event_focus.min(self.events.len().saturating_sub(1));
        self.events_loaded = true;
    }

    async fn refresh_events_inner(&mut self) -> Result<()> {
        let current_month = (self.view_date.year(), self.view_date.month());
        if self.last_loaded_month == Some(current_month) && self.events_loaded {
            // Month is cached — just re-filter for the new selected day
            self.filter_events_for_date(self.selected_date);
            return Ok(());
        }
        // Month changed — fetch the full month
        self.refresh_events().await
    }

    async fn save_event(&mut self) -> Result<Action> {
        let form = match &self.mode {
            Mode::Creating(f) => f,
            _ => return Ok(Action::None),
        };
        match form_to_event(form, self.selected_date) {
            Ok((summary, description, start, end)) => {
                match self
                    .backend
                    .create_event(&summary, description.as_deref(), start, end)
                    .await
                {
                    Ok(_) => {
                        self.status = format!("✓ Created: {}", summary);
                        self.mode = Mode::Normal;
                        self.refresh_events().await?;
                        Ok(Action::RefreshEvents)
                    }
                    Err(e) => {
                        self.status = format!("✗ Error: {}", e);
                        Ok(Action::None)
                    }
                }
            }
            Err(msg) => {
                self.status = msg;
                Ok(Action::None)
            }
        }
    }

    async fn update_event(&mut self) -> Result<Action> {
        let (form, event_id) = match &self.mode {
            Mode::Editing(f) => (f, self.selected_event().map(|e| e.id.clone())),
            _ => return Ok(Action::None),
        };
        let event_id = match event_id {
            Some(id) => id,
            None => return Ok(Action::None),
        };
        match form_to_event(form, self.selected_date) {
            Ok((summary, description, start, end)) => {
                match self
                    .backend
                    .update_event(&event_id, &summary, description.as_deref(), start, end)
                    .await
                {
                    Ok(_) => {
                        self.status = format!("✓ Updated: {}", summary);
                        self.mode = Mode::Normal;
                        self.refresh_events().await?;
                        Ok(Action::RefreshEvents)
                    }
                    Err(e) => {
                        self.status = format!("✗ Error: {}", e);
                        Ok(Action::None)
                    }
                }
            }
            Err(msg) => {
                self.status = msg;
                Ok(Action::None)
            }
        }
    }

    async fn delete_event(&mut self) -> Result<Action> {
        let event_id = match self.selected_event() {
            Some(e) => e.id.clone(),
            None => return Ok(Action::None),
        };
        match self.backend.delete_event(&event_id).await {
            Ok(_) => {
                self.status = "✓ Event deleted".into();
                self.mode = Mode::Normal;
                self.refresh_events().await?;
                Ok(Action::RefreshEvents)
            }
            Err(e) => {
                self.status = format!("✗ Error: {}", e);
                Ok(Action::None)
            }
        }
    }

    // ── Search ───────────────────────────────────────────────

    fn apply_search_filter(&mut self) {
        let query = match &self.search_query {
            Some(q) if !q.is_empty() => q.to_lowercase(),
            Some(_) => {
                self.filter_events_for_date(self.selected_date);
                return;
            }
            None => {
                self.filter_events_for_date(self.selected_date);
                return;
            }
        };
        self.events = self
            .month_events
            .iter()
            .filter(|e| {
                e.summary.to_lowercase().contains(&query)
                    || e.description.as_deref().unwrap_or("").to_lowercase().contains(&query)
            })
            .cloned()
            .collect();
        self.event_focus = self.event_focus.min(self.events.len().saturating_sub(1));
        self.events_loaded = true;
    }

    // ── Helpers ──────────────────────────────────────────────

    pub fn selected_event(&self) -> Option<&CalendarEvent> {
        self.events.get(self.event_focus)
    }
}

// ── Form key handler ────────────────────────────────────────────

fn handle_form_key(key: KeyEvent, form: &mut FormState) -> bool {
    match key.code {
        KeyCode::Tab | KeyCode::Down => {
            form.focus = (form.focus + 1) % form.fields.len();
        }
        KeyCode::BackTab | KeyCode::Up => {
            form.focus = if form.focus == 0 {
                form.fields.len() - 1
            } else {
                form.focus - 1
            };
        }
        KeyCode::Left => {
            let f = &mut form.fields[form.focus];
            f.cursor = f.cursor.saturating_sub(1);
        }
        KeyCode::Right => {
            let f = &mut form.fields[form.focus];
            f.cursor = (f.cursor + 1).min(f.value.len());
        }
        KeyCode::Backspace => {
            let f = &mut form.fields[form.focus];
            if f.cursor > 0 {
                f.value.remove(f.cursor - 1);
                f.cursor -= 1;
            }
        }
        KeyCode::Delete => {
            let f = &mut form.fields[form.focus];
            if f.cursor < f.value.len() {
                f.value.remove(f.cursor);
            }
        }
        KeyCode::Home => {
            form.fields[form.focus].cursor = 0;
        }
        KeyCode::End => {
            form.fields[form.focus].cursor = form.fields[form.focus].value.len();
        }
        KeyCode::Char(' ') => {
            let f = &mut form.fields[form.focus];
            f.value.insert(f.cursor, ' ');
            f.cursor += 1;
        }
        KeyCode::Char(c) if !c.is_control() => {
            let f = &mut form.fields[form.focus];
            f.value.insert(f.cursor, c);
            f.cursor += 1;
        }
        KeyCode::Enter => return true,
        _ => {}
    }
    false
}

fn is_cancel(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Esc)
}

// ── Form parsing ────────────────────────────────────────────────

fn form_to_event(
    form: &FormState,
    default_date: NaiveDate,
) -> std::result::Result<(String, Option<String>, NaiveDateTime, NaiveDateTime), String> {
    if form.fields.is_empty() {
        return Err("No fields in form".into());
    }

    let summary = form.fields[0].value.trim().to_string();
    if summary.is_empty() {
        return Err("Title is required".into());
    }

    let date_str = form.fields.get(1).map(|f| f.value.trim()).unwrap_or("");
    let date = if date_str.is_empty() {
        default_date
    } else {
        NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
            .map_err(|_| format!("Invalid date format — use YYYY-MM-DD"))?
    };

    let start_str = form.fields.get(2).map(|f| f.value.trim()).unwrap_or("09:00");
    let end_str = form.fields.get(3).map(|f| f.value.trim()).unwrap_or("10:00");

    let desc_str = form.fields.get(4).map(|f| f.value.trim()).unwrap_or("");
    let description = if desc_str.is_empty() { None } else { Some(desc_str.to_string()) };

    let start_time = chrono::NaiveTime::parse_from_str(start_str, "%H:%M")
        .map_err(|_| format!("Invalid start time — use HH:MM"))?;
    let end_time = chrono::NaiveTime::parse_from_str(end_str, "%H:%M")
        .map_err(|_| format!("Invalid end time — use HH:MM"))?;

    let start = date.and_time(start_time);
    let end = date.and_time(end_time);

    if end <= start {
        return Err("End time must be after start time".into());
    }

    Ok((summary, description, start, end))
}

// ── Date helpers ────────────────────────────────────────────────

fn prev_month(date: NaiveDate) -> NaiveDate {
    let (y, m) = if date.month() == 1 {
        (date.year() - 1, 12)
    } else {
        (date.year(), date.month() - 1)
    };
    NaiveDate::from_ymd_opt(y, m, 1).unwrap()
}

fn next_month(date: NaiveDate) -> NaiveDate {
    let (y, m) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    NaiveDate::from_ymd_opt(y, m, 1).unwrap()
}

fn last_day_of_month(date: NaiveDate) -> NaiveDate {
    let (y, m) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    NaiveDate::from_ymd_opt(y, m, 1).unwrap().pred_opt().unwrap()
}

fn num_days_in_month(year: i32, month: u32) -> u32 {
    if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .map(|d| d.pred_opt().unwrap().day())
    .unwrap()
}
