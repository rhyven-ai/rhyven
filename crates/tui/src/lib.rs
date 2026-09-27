//! Desktop terminal marketplace. Installation always has a review step.
use agent_market_core::{catalog, Result, Runtime};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEvent, MouseEventKind,
};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use serde_json::Value;
use std::cell::Cell;
use std::io::IsTerminal;
use std::time::{Duration, Instant};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(200);
const UPDATE_HELP: &str = "Update app upgrades only the selected installed app in this collection. It does not update the Rhyven program or other collections.\n\nAfter you confirm, Rhyven saves a recovery backup, stages the new version, applies its declared migrations and validates the result before activation. Failed staging leaves the current app and state in place.\n\nPersistent services in this collection pause during maintenance and previously enabled services resume afterward. Backups remain in the collection's recovery directory.";
const REFRESH_HELP: &str = "Auto-refresh reloads installed state and the local catalog every 2 seconds. It preserves your selection and search; approval screens stay fixed until you accept or cancel.\n\nRefresh does not install updates or contact GitHub. To fetch newer packages, run rhyven registry-sync OWNER/REPO explicitly in the same home/workspace; the TUI picks up the cache change automatically.";

#[derive(Clone, Copy, PartialEq)]
enum Operation {
    Install,
    Update,
    Remove,
}

pub struct Model {
    pub scope: String,
    pub packages: Vec<Value>,
    pub selected: usize,
    pub query: String,
    pub review: bool,
    pub message: String,
    pub scroll: u16,
    pub pane: usize,
    list_area: Cell<Rect>,
    nav_area: Cell<Rect>,
    search_area: Cell<Rect>,
    popup_area: Cell<Rect>,
    list_offset: Cell<usize>,
    searching: bool,
    installed_only: bool,
    installed: Vec<String>,
    installed_packages: Vec<Value>,
    star_labels: std::collections::BTreeMap<String, String>,
    pending: Option<(Operation, Value)>,
    service_output: String,
    last_refresh: Instant,
    refresh_error: Option<String>,
}
impl Model {
    pub fn new(runtime: &Runtime) -> Result<Self> {
        let scope = agent_market_core::collections::scope(&runtime.root)?;
        let mut model = Self { scope: scope["collection"].as_str().map(|n| format!("Collection: {n}")).unwrap_or_else(|| format!("Workspace: {}", runtime.root.display())), packages: vec![], selected: 0, query: String::new(), review: false,
            message: "Click / Enter details · i install · u update app · x remove (Installed) · ? help · q quit".into(),
            list_area: Cell::new(Rect::default()), nav_area: Cell::new(Rect::default()), search_area: Cell::new(Rect::default()), popup_area: Cell::new(Rect::default()), list_offset: Cell::new(0),
            scroll: 0, pane: 0, searching: false, installed_only: false, installed: vec![],
            installed_packages: vec![],
            star_labels: std::collections::BTreeMap::new(), pending: None, service_output: String::new(),
            last_refresh: Instant::now(), refresh_error: None };
        model.reload(runtime);
        Ok(model)
    }
    fn refresh(&mut self, runtime: &Runtime) -> Result<()> {
        // Take a coherent snapshot without waiting for another agent's maintenance.
        // Build it before replacing any visible state so a failed refresh stays usable.
        let _gate = agent_market_core::maintenance::try_lock(&runtime.root, Duration::ZERO)?;
        let selected_name = self
            .current()
            .and_then(|p| p["name"].as_str().map(str::to_owned));
        let mut latest = std::collections::BTreeMap::new();
        for p in catalog::list(&runtime.root)? {
            latest.insert(p["name"].as_str().unwrap().to_owned(), p);
        }
        let installed_packages: Vec<Value> = runtime
            .apps()?
            .as_array()
            .unwrap()
            .iter()
            .map(|p| runtime.describe(p["name"].as_str().unwrap()))
            .collect::<Result<_>>()?;
        let installed = installed_packages
            .iter()
            .map(|p| p["name"].as_str().unwrap().to_owned())
            .collect();
        let listings = runtime.call(
            "query",
            serde_json::json!({"app":"rhyven/marketplace","object":"listing","limit":1000}),
        )?;
        let star_labels = listings["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                let d = &v["data"];
                (
                    d["name"].as_str().unwrap().to_owned(),
                    d["stars"]
                        .as_u64()
                        .map(|n| format!("GitHub stars: {n} (cached)"))
                        .unwrap_or_else(|| "GitHub stars: unavailable".into()),
                )
            })
            .collect();
        self.packages = latest.into_values().collect();
        self.installed_packages = installed_packages;
        self.installed = installed;
        self.star_labels = star_labels;
        let visible = self.visible();
        let selected = selected_name
            .as_ref()
            .and_then(|name| visible.iter().position(|p| p["name"] == *name));
        self.selected =
            selected.unwrap_or_else(|| self.selected.min(visible.len().saturating_sub(1)));
        if selected_name.is_some() && selected.is_none() && self.pane != 5 {
            // Never silently substitute another app inside an open details pane.
            self.pane = 0;
            self.scroll = 0;
        }
        self.refresh_error = None;
        Ok(())
    }
    fn reload(&mut self, runtime: &Runtime) {
        if let Err(error) = self.refresh(runtime) {
            self.refresh_error = Some(error.to_string());
        }
        self.last_refresh = Instant::now();
    }
    fn tick(&mut self, runtime: &Runtime, now: Instant) {
        // The package and installed version shown for consent remain fixed.
        if !self.review && now.duration_since(self.last_refresh) >= REFRESH_INTERVAL {
            self.reload(runtime);
        }
    }
    fn apply(&self, runtime: &Runtime, operation: Operation, package: &Value) -> Result<Value> {
        let _gate = agent_market_core::maintenance::lock(&runtime.root)?;
        let name = package["name"].as_str().unwrap();
        let current = if runtime
            .apps()?
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == name)
        {
            Some(runtime.describe(name)?)
        } else {
            None
        };
        if current.as_ref() != self.installed_package(name) {
            return Err(agent_market_core::Error::new("approval_stale", "The installed app changed while you were reviewing it. Review the current version again."));
        }
        match operation {
            Operation::Install => runtime.install(package, true, false),
            Operation::Update => runtime.install(package, true, true),
            Operation::Remove => runtime.uninstall(name),
        }
    }
    fn installed_package(&self, name: &str) -> Option<&Value> {
        self.installed_packages.iter().find(|p| p["name"] == name)
    }
    fn update_for(&self, name: &str) -> Option<&Value> {
        let old = self.installed_package(name)?;
        self.packages.iter().find(|p| {
            p["name"] == name
                && catalog::version(p["version"].as_str().unwrap()).unwrap()
                    > catalog::version(old["version"].as_str().unwrap()).unwrap()
        })
    }
    fn versions(&self, name: &str) -> String {
        let installed = self
            .installed_package(name)
            .and_then(|p| p["version"].as_str())
            .unwrap_or("not installed");
        let available = self
            .packages
            .iter()
            .find(|p| p["name"] == name)
            .and_then(|p| p["version"].as_str())
            .unwrap_or("not in catalog");
        format!(
            "Installed: {installed} · Available: {available}{}",
            if self.update_for(name).is_some() {
                " · Update available"
            } else {
                ""
            }
        )
    }
    fn permission_changes(&self, package: &Value) -> String {
        let permissions = |p: &Value| -> std::collections::BTreeSet<String> {
            p["permissions"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        };
        let before = self
            .installed_package(package["name"].as_str().unwrap())
            .map(permissions)
            .unwrap_or_default();
        let after = permissions(package);
        let display = |values: Vec<String>| {
            if values.is_empty() {
                "none".into()
            } else {
                values.join(", ")
            }
        };
        format!(
            "Added permissions: {}\nRemoved permissions: {}",
            display(after.difference(&before).cloned().collect()),
            display(before.difference(&after).cloned().collect())
        )
    }
    pub fn visible(&self) -> Vec<&Value> {
        let packages = if self.installed_only {
            &self.installed_packages
        } else {
            &self.packages
        };
        packages
            .iter()
            .filter(|p| {
                format!("{} {}", p["name"], p["description"])
                    .to_lowercase()
                    .contains(&self.query.to_lowercase())
            })
            .collect()
    }
    fn begin(&mut self, operation: Operation, package: Value) {
        self.pending = Some((operation, package));
        self.review = true;
        self.scroll = 0;
    }
    fn current(&self) -> Option<Value> {
        self.visible().get(self.selected).map(|p| (*p).clone())
    }
    pub fn mouse(&mut self, event: MouseEvent, runtime: &Runtime) -> Result<()> {
        let inside = |r: Rect| r.contains((event.column, event.row).into());
        if self.review || self.pane != 0 {
            match event.kind {
                MouseEventKind::ScrollDown => self.scroll = self.scroll.saturating_add(3),
                MouseEventKind::ScrollUp => self.scroll = self.scroll.saturating_sub(3),
                MouseEventKind::Down(MouseButton::Left)
                    if !self.review && !inside(self.popup_area.get()) =>
                {
                    self.pane = 0
                }
                _ => (),
            }
            return Ok(());
        }
        match event.kind {
            MouseEventKind::ScrollDown => {
                self.key(KeyCode::Down, runtime)?;
            }
            MouseEventKind::ScrollUp => {
                self.key(KeyCode::Up, runtime)?;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if inside(self.search_area.get()) {
                    self.key(KeyCode::Char('/'), runtime)?;
                } else if inside(self.nav_area.get()) {
                    let row = event.row - self.nav_area.get().y;
                    match row {
                        1 | 3 => {
                            self.installed_only = row == 3;
                            self.selected = 0;
                            self.list_offset.set(0);
                        }
                        5 => {
                            self.key(KeyCode::Char('/'), runtime)?;
                        }
                        7 => {
                            self.key(KeyCode::Char('r'), runtime)?;
                        }
                        8 => {
                            self.key(KeyCode::Char('u'), runtime)?;
                        }
                        9 => {
                            self.key(KeyCode::Char('x'), runtime)?;
                        }
                        11 => {
                            self.key(KeyCode::Char('?'), runtime)?;
                        }
                        _ => (),
                    }
                } else {
                    let area = self.list_area.get();
                    if inside(area)
                        && event.row > area.y
                        && event.row < area.bottom().saturating_sub(1)
                    {
                        let index =
                            self.list_offset.get() + usize::from((event.row - area.y - 1) / 4);
                        if index < self.visible().len() {
                            self.selected = index;
                            self.pane = 3;
                            self.scroll = 0;
                            self.searching = false;
                        }
                    }
                }
            }
            _ => (),
        }
        Ok(())
    }
    pub fn key(&mut self, key: KeyCode, runtime: &Runtime) -> Result<bool> {
        if self.review {
            match key {
                KeyCode::Char('y') => {
                    if let Some((operation, p)) = self.pending.take() {
                        let name = p["name"].as_str().unwrap();
                        let result = self.apply(runtime, operation, &p);
                        self.message = match result {
                            Ok(result) => match operation {
                                Operation::Install => {
                                    format!("Installed {name} — available to agents")
                                }
                                Operation::Update => format!(
                                    "Updated {name} to {} · Recovery backup: {}",
                                    p["version"].as_str().unwrap(),
                                    result["recovery_backup"]
                                        .as_str()
                                        .unwrap_or("see collection recovery directory")
                                ),
                                Operation::Remove => {
                                    format!("Removed {name}; data retained for reinstall")
                                }
                            },
                            Err(e) => e.to_string(),
                        };
                    }
                    self.review = false;
                    self.pane = 0;
                    self.reload(runtime);
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    self.pending = None;
                    self.review = false;
                    self.message = "Operation cancelled".into();
                    self.reload(runtime);
                }
                KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
                KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
                _ => (),
            }
            return Ok(false);
        }
        if self.searching {
            match key {
                KeyCode::Enter | KeyCode::Esc => self.searching = false,
                KeyCode::Backspace => {
                    self.query.pop();
                    self.selected = 0;
                }
                KeyCode::Char(c) => {
                    self.query.push(c);
                    self.selected = 0;
                }
                _ => (),
            }
            return Ok(false);
        }
        match key {
            KeyCode::Esc if self.pane != 0 => self.pane = 0,
            KeyCode::Char('q') | KeyCode::Esc => return Ok(true),
            KeyCode::Tab => {
                self.installed_only = !self.installed_only;
                self.selected = 0;
                self.pane = 0;
            }
            KeyCode::Char('/') => {
                self.searching = true;
                self.query.clear();
                self.selected = 0;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(self.visible().len().saturating_sub(1));
                self.scroll = 0;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                self.scroll = 0;
            }
            KeyCode::Enter => {
                if self.current().is_some() {
                    self.pane = 3;
                    self.scroll = 0;
                }
            }
            KeyCode::Char('i') => {
                if let Some(p) = self.current() {
                    let name = p["name"].as_str().unwrap();
                    if let Some(update) = self.update_for(name).cloned() {
                        self.begin(Operation::Update, update);
                    } else if self.installed_package(name).is_some() {
                        self.pane = 3;
                        self.scroll = 0;
                    } else {
                        self.begin(Operation::Install, p);
                    }
                }
            }
            KeyCode::Char('u') => {
                if let Some(p) = self.current() {
                    if let Some(update) = self.update_for(p["name"].as_str().unwrap()).cloned() {
                        self.begin(Operation::Update, update);
                    } else if self
                        .installed_package(p["name"].as_str().unwrap())
                        .is_none()
                    {
                        self.message = "Update app upgrades an installed app. Press i to review installation first.".into();
                    } else {
                        self.message = "No newer app version in the local catalog. registry-sync fetches packages; this view refreshes automatically. ? explains updates.".into();
                    }
                } else {
                    self.message = "Select an installed app to review an update. ? explains refresh and updates.".into();
                }
            }
            KeyCode::Char('x') if self.installed_only => {
                if let Some(p) = self.current() {
                    self.begin(Operation::Remove, p);
                }
            }
            KeyCode::Char('d') => {
                self.pane = if self.pane == 3 { 0 } else { 3 };
                self.scroll = 0;
            }
            KeyCode::Char('s') => {
                self.pane = 1;
                self.scroll = 0;
            }
            KeyCode::Char('g') => {
                self.pane = 2;
                self.scroll = 0;
            }
            KeyCode::Char('?') => {
                self.pane = 5;
                self.scroll = 0;
            }
            KeyCode::Char(key @ ('b' | 't' | 'v' | 'l')) if self.installed_only => {
                if let Some(p) = self.current() {
                    let operation = match key {
                        'b' => "start",
                        't' => "stop",
                        'v' => "status",
                        _ => "logs",
                    };
                    self.service_output = match agent_market_core::services::control(
                        runtime,
                        operation,
                        p["name"].as_str(),
                    ) {
                        Ok(v) if operation == "logs" => v["text"].as_str().unwrap_or("").to_owned(),
                        Ok(v) => serde_json::to_string_pretty(&v)?,
                        Err(e) => e.to_string(),
                    };
                    self.pane = 4;
                    self.scroll = 0;
                }
            }
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::Char('r') => {
                self.reload(runtime);
                self.message = "Reloaded local state · auto-refresh every 2s · u reviews an app upgrade · ? help".into();
            }
            _ => (),
        }
        Ok(false)
    }
}
fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect()
}
const BG: Color = Color::Rgb(12, 15, 18);
const CYAN: Color = Color::Rgb(0, 217, 255);
const BLUE: Color = Color::Rgb(59, 130, 246);
const WHITE: Color = Color::Rgb(248, 250, 252);
const MUTED: Color = Color::Rgb(157, 185, 218);
const EDGE: Color = Color::Rgb(30, 62, 96);

fn panel() -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(EDGE))
        .style(Style::default().bg(BG).fg(WHITE))
}

/// The beacon's white upper chevron and faceted cyan/blue lower chevron.
/// Half-block cells give two vertical samples per terminal row without image-protocol dependencies.
fn render_logo(frame: &mut Frame, area: Rect) {
    fn in_polygon(x: f64, y: f64, points: &[(f64, f64)]) -> bool {
        let mut inside = false;
        let mut j = points.len() - 1;
        for i in 0..points.len() {
            let (xi, yi) = points[i];
            let (xj, yj) = points[j];
            if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                inside = !inside;
            }
            j = i;
        }
        inside
    }
    let pixel = |x: f64, y: f64| {
        if in_polygon(
            x,
            y,
            &[(0.5, 0.0), (0.138, 0.707), (0.5, 0.409), (0.855, 0.707)],
        ) {
            return WHITE;
        }
        if in_polygon(
            x,
            y,
            &[
                (0.5, 0.507),
                (0.922, 0.830),
                (1.0, 1.0),
                (0.704, 1.0),
                (0.5, 0.670),
                (0.291, 1.0),
                (0.0, 1.0),
                (0.080, 0.830),
            ],
        ) {
            if x < 0.5 && y < 1.0 - x * 0.66 {
                return CYAN;
            }
            if x >= 0.5 && y < 0.34 + x * 0.66 {
                return Color::Rgb(20, 166, 247);
            }
            return BLUE;
        }
        BG
    };
    for row in 0..area.height {
        for col in 0..area.width {
            let x = (f64::from(col) + 0.5) / f64::from(area.width);
            let top = pixel(
                x,
                (f64::from(row) * 2.0 + 0.5) / (f64::from(area.height) * 2.0),
            );
            let bottom = pixel(
                x,
                (f64::from(row) * 2.0 + 1.5) / (f64::from(area.height) * 2.0),
            );
            if let Some(cell) = frame.buffer_mut().cell_mut((area.x + col, area.y + row)) {
                match (top == BG, bottom == BG) {
                    (true, true) => {
                        cell.set_symbol(" ").set_bg(BG);
                    }
                    (true, false) => {
                        cell.set_symbol("▄").set_fg(bottom).set_bg(BG);
                    }
                    (false, true) => {
                        cell.set_symbol("▀").set_fg(top).set_bg(BG);
                    }
                    (false, false) => {
                        if std::env::var_os("NO_COLOR").is_some() {
                            cell.set_symbol("█");
                        } else {
                            cell.set_symbol("▀");
                        }
                        cell.set_fg(top).set_bg(bottom);
                    }
                }
            }
        }
    }
}

pub fn render(frame: &mut Frame, model: &Model, _installed: &[String]) {
    let area = frame.area();
    model.list_area.set(Rect::default());
    model.nav_area.set(Rect::default());
    model.search_area.set(Rect::default());
    frame.render_widget(
        Block::default().style(Style::default().bg(BG).fg(WHITE)),
        area,
    );
    if area.width < 48 || area.height < 16 {
        frame.render_widget(
            Paragraph::new("Enlarge terminal to at least 48 × 16.\nq quit")
                .style(Style::default().fg(CYAN))
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(if area.height >= 28 { 9 } else { 6 }),
        Constraint::Min(5),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(format!(
            " {} · {}",
            model.scope,
            if model.review {
                "Auto-refresh paused for review"
            } else if model.refresh_error.is_some() {
                "Refresh delayed · ? details"
            } else {
                "Auto-refresh 2s"
            }
        ))
        .style(Style::default().fg(MUTED)),
        rows[0],
    );
    let logo_height = rows[1].height.saturating_sub(1);
    render_logo(
        frame,
        Rect::new(
            rows[1].x + 2,
            rows[1].y,
            if logo_height >= 8 { 20 } else { 14 },
            logo_height,
        ),
    );
    frame.render_widget(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(EDGE)),
        rows[1],
    );
    let cols = Layout::horizontal([
        Constraint::Length(if area.width >= 95 { 23 } else { 0 }),
        Constraint::Min(1),
    ])
    .split(rows[2]);
    model.nav_area.set(cols[0]);
    if cols[0].width > 0 {
        let nav = vec![
            Line::raw(""),
            Line::styled(
                " ◇  Marketplace",
                Style::default()
                    .fg(if model.installed_only { MUTED } else { CYAN })
                    .bg(if model.installed_only {
                        BG
                    } else {
                        Color::Rgb(12, 35, 59)
                    }),
            ),
            Line::raw(""),
            Line::styled(
                " ▤  Installed   [Tab]",
                Style::default().fg(if model.installed_only { CYAN } else { MUTED }),
            ),
            Line::raw(""),
            Line::raw(" /  Search"),
            Line::raw(""),
            Line::raw(" ↻  Refresh now  [r]"),
            Line::raw(" ↑  Update app   [u]"),
            Line::raw(" ×  Remove       [x]"),
            Line::raw(""),
            Line::raw(" ?  Update help  [?]"),
        ];
        frame.render_widget(
            Paragraph::new(nav).style(Style::default().fg(MUTED)).block(
                Block::default()
                    .borders(Borders::RIGHT)
                    .border_style(Style::default().fg(EDGE)),
            ),
            cols[0],
        );
    }
    let content = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(1),
    ])
    .margin(1)
    .split(cols[1]);
    model.search_area.set(content[1]);
    model.list_area.set(content[2]);
    let visible = model.visible();
    frame.render_widget(
        Paragraph::new(vec![Line::styled(
            format!(
                "{}   ·   {} apps · {} updates",
                if model.installed_only {
                    "Installed"
                } else {
                    "Marketplace"
                },
                visible.len(),
                model
                    .installed
                    .iter()
                    .filter(|name| model.update_for(name).is_some())
                    .count()
            ),
            Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
        )]),
        content[0],
    );
    let search = if model.searching {
        format!(" /  {}▌", model.query)
    } else if model.query.is_empty() {
        " /  Search apps, categories, or keywords...".into()
    } else {
        format!(" /  {}", model.query)
    };
    frame.render_widget(
        Paragraph::new(search)
            .style(Style::default().fg(MUTED))
            .block(panel().border_style(Style::default().fg(CYAN))),
        content[1],
    );
    let items: Vec<_> = visible
        .iter()
        .enumerate()
        .map(|(index, p)| {
            let name = p["name"].as_str().unwrap();
            let publisher = catalog::publisher_label(p);
            let app = clean(catalog::display_name(p));
            let action = if model.update_for(name).is_some() {
                "↑ Update app [u]"
            } else if model.installed_package(name).is_some() {
                "✓ Installed"
            } else {
                "Details ↵"
            };
            let mut heading = Line::from(vec![
                Span::styled(
                    format!(" ◇  {app}  "),
                    Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(
                        "v{}{}",
                        model.installed_package(name).unwrap_or(p)["version"]
                            .as_str()
                            .unwrap(),
                        model
                            .update_for(name)
                            .map(|v| format!(" → v{}", v["version"].as_str().unwrap()))
                            .unwrap_or_default()
                    ),
                    Style::default().fg(MUTED),
                ),
            ]);
            if content[2].width >= 65 {
                let button = format!("  {action}  ");
                let gap = (content[2].width as usize)
                    .saturating_sub(heading.width() + button.chars().count() + 4);
                heading.spans.push(Span::raw(" ".repeat(gap)));
                heading.spans.push(Span::styled(
                    button,
                    if index == model.selected {
                        Style::default().fg(CYAN).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(CYAN)
                    },
                ));
            }
            ListItem::new(vec![
                heading,
                Line::styled(
                    format!("    {}", clean(p["description"].as_str().unwrap())),
                    Style::default().fg(MUTED),
                ),
                Line::from(vec![
                    Span::styled(
                        format!(
                            "    {publisher} · {} · {}",
                            p["hosting"]["mode"].as_str().unwrap(),
                            model
                                .star_labels
                                .get(name)
                                .map(String::as_str)
                                .unwrap_or("GitHub stars: unavailable")
                        ),
                        Style::default().fg(MUTED),
                    ),
                    Span::styled(
                        if content[2].width < 65 {
                            format!("   [ {action} ]")
                        } else {
                            String::new()
                        },
                        Style::default().fg(CYAN),
                    ),
                ]),
                Line::raw(""),
            ])
        })
        .collect();
    if items.is_empty() {
        frame.render_widget(
            Paragraph::new("No matching apps. Press / to search or Tab to switch views.")
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(MUTED)),
            content[2],
        );
    } else {
        let mut state = ListState::default()
            .with_offset(model.list_offset.get())
            .with_selected(Some(model.selected));
        frame.render_stateful_widget(
            List::new(items)
                .highlight_symbol("▎")
                .highlight_style(Style::default().bg(Color::Rgb(12, 35, 59)))
                .block(panel().border_style(Style::default().fg(BLUE))),
            content[2],
            &mut state,
        );
        model.list_offset.set(state.offset());
    }
    frame.render_widget(
        Paragraph::new(clean(
            &model
                .refresh_error
                .as_ref()
                .map(|error| {
                    format!("Refresh delayed; showing last view. Retrying automatically. {error}")
                })
                .unwrap_or_else(|| model.message.clone()),
        ))
        .style(Style::default().fg(CYAN))
        .wrap(Wrap { trim: false })
        .block(panel()),
        rows[3],
    );
    frame.render_widget(
        Paragraph::new(format!(
            " v{}  ·  Click / Enter details  ·  i install  ·  Esc close",
            env!("CARGO_PKG_VERSION")
        ))
        .style(Style::default().fg(MUTED)),
        rows[4],
    );
    if model.review || model.pane != 0 {
        let popup = Rect::new(
            area.x + 2,
            area.y + 2,
            area.width.saturating_sub(4),
            area.height.saturating_sub(4),
        );
        frame.render_widget(Clear, popup);
        model.popup_area.set(popup);
        render_details(frame, model, popup);
    }
}

fn render_details(frame: &mut Frame, model: &Model, area: Rect) {
    frame.render_widget(panel(), area);
    if model.pane == 5 && !model.review {
        let error = model
            .refresh_error
            .as_ref()
            .map(|e| format!("\n\nLast refresh problem: {e}"))
            .unwrap_or_default();
        frame.render_widget(
            Paragraph::new(clean(&format!("{}\n\nREFRESH THE VIEW\n{REFRESH_HELP}\n\nUPDATE AN APP\n{UPDATE_HELP}\n\nSelect an installed app and press u to review its installed and available versions, permissions and hosting. Press y to confirm or n / Esc to cancel.\n\nr: refresh now · Tab: Marketplace / Installed · /: search · q: quit{error}", model.scope)))
                .wrap(Wrap { trim: false }).scroll((model.scroll, 0))
                .style(Style::default().fg(MUTED).bg(BG))
                .block(panel().title(" Refresh and app updates ").title_bottom(" Esc close · PgUp/PgDn scroll ").border_style(Style::default().fg(CYAN))),
            area,
        );
        return;
    }
    let (title, body) = if let Some(p) = model
        .pending
        .as_ref()
        .map(|(_, p)| p.clone())
        .or_else(|| model.current())
    {
        if model.review {
            let operation = model.pending.as_ref().unwrap().0;
            if operation == Operation::Remove {
                (" Review removal — y accept / n cancel ", format!("Remove {} v{}?\n\nThe app will disappear from agent discovery. Its records and compatibility contract are retained for reinstallation.\n\nPress y to remove, n to cancel.", p["name"], p["version"]))
            } else {
                let effect = if operation == Operation::Update {
                    format!("{UPDATE_HELP}\n\n{}", model.permission_changes(&p))
                } else {
                    "Install this app into the selected collection so connected agents can discover and use it.".into()
                };
                (if operation == Operation::Update { " Review app update — y accept / n cancel " } else { " Review installation — y accept / n cancel " },
                 format!("{} @ {}\n\n{}\n\n{effect}\n\nRequested permissions: {}\n\nHosting disclosures:\n{}\n\nCertification: Unverified.\n\nSHA-256: {}\n\nPress y to accept these permissions and proceed, n to cancel.", p["name"], p["version"], p["description"], p["permissions"], serde_json::to_string_pretty(&p["hosting"]).unwrap(), agent_market_core::store::hash(&p)))
            }
        } else {
            match model.pane {
            1=>(" App contract ",serde_json::to_string_pretty(&serde_json::json!({"objects":p["objects"],"actions":p["actions"]})).unwrap()),
            2=>(" Agent guide ",p["guide"].as_str().unwrap().into()),
            4=>(" Service status / logs ",model.service_output.clone()),
            _=>(" App details ",format!("{}  v{}\nApp ID: {}\n\n{}\n\nHosting: {}\nPublisher: {}\nCertification: Unverified\n\nPermissions: {}\n\nCapabilities: {} actions · {} record types\n\ni: review install   u: review update   x: remove in Installed\ns: inspect contract   g: agent guide\nEsc: close",catalog::display_name(&p),p["version"].as_str().unwrap(),p["name"].as_str().unwrap(),p["description"].as_str().unwrap(),p["hosting"]["mode"].as_str().unwrap(),catalog::publisher_label(&p),p["permissions"].as_array().unwrap().iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "),p["actions"].as_object().unwrap().len(),p["objects"].as_object().unwrap().len()))
        }
        }
    } else {
        (" App details ", "No matching apps".into())
    };
    frame.render_widget(
        Paragraph::new(clean(&format!(
            "{}\n{}\n{}\n\n{}\n\nExecution: {}",
            model.scope,
            model
                .current()
                .map(|p| model.versions(p["name"].as_str().unwrap()))
                .unwrap_or_default(),
            model
                .current()
                .and_then(|p| model.star_labels.get(p["name"].as_str().unwrap()).cloned())
                .unwrap_or_else(|| "GitHub stars: unavailable".into()),
            body,
            model
                .pending
                .as_ref()
                .map(|(_, p)| p.clone())
                .or_else(|| model.current())
                .map(|p| {
                    let execution = p
                        .get("execution")
                        .cloned()
                        .unwrap_or(serde_json::json!({"driver":"declarative"}));
                    if model.review {
                        execution.to_string()
                    } else if execution["mode"] == "service" {
                        format!("container service · b start · t stop · v status · l logs (Installed) · peers: {}", execution.get("calls").cloned().unwrap_or(serde_json::json!([])))
                    } else {
                        execution["driver"].as_str().unwrap_or("declarative").to_owned()
                    }
                })
                .unwrap_or_default()
        )))
        .wrap(Wrap { trim: false })
        .scroll((model.scroll, 0))
        .style(Style::default().fg(MUTED).bg(BG))
        .block(
            panel()
                .title(title)
                .title_bottom(" Esc close · PgUp/PgDn scroll ")
                .border_style(Style::default().fg(CYAN)),
        ),
        area,
    );
}
pub fn run(runtime: Runtime) -> Result<()> {
    if !std::io::stdout().is_terminal() {
        return Err(agent_market_core::Error::new(
            "terminal",
            "Marketplace requires a terminal; use search for JSON",
        ));
    }
    let mut model = Model::new(&runtime)?;
    let mut terminal = ratatui::try_init()?;
    let result = (|| -> Result<()> {
        crossterm::execute!(std::io::stdout(), EnableMouseCapture)?;
        loop {
            model.tick(&runtime, Instant::now());
            terminal.draw(|frame| render(frame, &model, &model.installed))?;
            if !event::poll(INPUT_POLL_INTERVAL)? {
                continue;
            }
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if model.key(key.code, &runtime)? {
                        break;
                    }
                }
                Event::Mouse(mouse) => model.mouse(mouse, &runtime)?,
                _ => (),
            }
        }
        Ok(())
    })();
    let _ = crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    fn inventory() -> Value {
        catalog::bundled()
            .into_iter()
            .find(|p| p["name"] == "rhyven/inventory")
            .unwrap()
    }
    fn tick(model: &mut Model, runtime: &Runtime) {
        model.tick(runtime, model.last_refresh + REFRESH_INTERVAL);
    }
    fn screen(model: &Model) -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 48)).unwrap();
        terminal
            .draw(|f| render(f, model, &model.installed))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect()
    }
    #[test]
    fn automatic_refresh_tracks_other_clients_and_preserves_browsing() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Runtime::new(dir.path(), "human").unwrap();
        let other = Runtime::new(dir.path(), "other-agent").unwrap();
        let mut model = Model::new(&runtime).unwrap();
        model.query = "inventory".into();
        model.searching = true;
        model.pane = 3;
        model.scroll = 4;
        let package = inventory();
        other.install(&package, true, false).unwrap();
        let mut earlier = package.clone();
        earlier["name"] = serde_json::json!("aaa/inventory");
        earlier["publisher"] = serde_json::json!("aaa");
        catalog::publish(&other.root, &earlier).unwrap();
        let mut update = package.clone();
        update["version"] = serde_json::json!("0.4.0");
        catalog::publish(&other.root, &update).unwrap();
        tick(&mut model, &runtime);
        assert_eq!(model.current().unwrap()["name"], "rhyven/inventory");
        assert_eq!(model.selected, 1); // An earlier sorted app must not steal selection.
        assert_eq!(model.query, "inventory");
        assert!(model.searching);
        assert_eq!((model.pane, model.scroll), (3, 4));
        assert_eq!(
            model.update_for("rhyven/inventory").unwrap()["version"],
            "0.4.0"
        );
        assert_eq!(
            other.describe("rhyven/inventory").unwrap()["version"],
            package["version"]
        ); // Refresh never upgrades.
        model.installed_only = true;
        model.selected = 0;
        other.uninstall("rhyven/inventory").unwrap();
        tick(&mut model, &runtime);
        assert!(model.visible().is_empty());
        assert_eq!(model.pane, 0);
    }
    #[test]
    fn failed_refresh_retains_last_snapshot_and_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Runtime::new(dir.path(), "human").unwrap();
        let mut model = Model::new(&runtime).unwrap();
        let before = model.packages.clone();
        let cache = agent_market_core::collections::registry_dir(&runtime.root)
            .unwrap()
            .join("registry");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("broken.json"), "{broken").unwrap();
        runtime.install(&inventory(), true, false).unwrap();
        tick(&mut model, &runtime);
        assert_eq!(model.packages, before);
        assert!(model.installed.is_empty());
        assert!(model.refresh_error.is_some());
        assert!(screen(&model).contains("Refresh delayed"));
        model.key(KeyCode::Char('r'), &runtime).unwrap();
        std::fs::remove_file(cache.join("broken.json")).unwrap();
        tick(&mut model, &runtime);
        assert!(model.refresh_error.is_none());
        assert_eq!(model.installed, vec!["rhyven/inventory"]);
    }
    #[test]
    fn refresh_does_not_wait_for_another_clients_maintenance() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Runtime::new(dir.path(), "human").unwrap();
        let mut model = Model::new(&runtime).unwrap();
        let other = runtime.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _gate = agent_market_core::maintenance::lock(&other.root).unwrap();
            ready_tx.send(()).unwrap();
            let _ = release_rx.recv_timeout(Duration::from_secs(3));
        });
        ready_rx.recv().unwrap();
        let started = Instant::now();
        tick(&mut model, &runtime);
        let elapsed = started.elapsed();
        let delayed = model.refresh_error.is_some();
        let _ = release_tx.send(());
        worker.join().unwrap();
        assert!(elapsed < Duration::from_millis(500));
        assert!(delayed);
        tick(&mut model, &runtime);
        assert!(model.refresh_error.is_none());
    }
    #[test]
    fn review_stays_fixed_and_rejects_changes_from_another_client() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Runtime::new(dir.path(), "human").unwrap();
        let other = Runtime::new(dir.path(), "agent").unwrap();
        let mut package = inventory();
        runtime.install(&package, true, false).unwrap();
        let mut model = Model::new(&runtime).unwrap();
        model.key(KeyCode::Tab, &runtime).unwrap();
        model.key(KeyCode::Char('x'), &runtime).unwrap();
        let reviewed = model.pending.as_ref().unwrap().1.clone();
        package["version"] = serde_json::json!("0.4.0");
        other.install(&package, true, true).unwrap();
        tick(&mut model, &runtime);
        assert!(model.review);
        assert_eq!(model.pending.as_ref().unwrap().1, reviewed);
        assert_eq!(model.installed_packages[0]["version"], reviewed["version"]);
        model.key(KeyCode::Char('y'), &runtime).unwrap();
        assert!(model.message.contains("changed while you were reviewing"));
        assert_eq!(
            runtime.describe("rhyven/inventory").unwrap()["version"],
            "0.4.0"
        );
        assert!(!model.review);
        assert_eq!(model.installed_packages[0]["version"], "0.4.0");
    }
    #[test]
    fn help_works_without_apps_and_update_review_explains_effects() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Runtime::new(dir.path(), "human").unwrap();
        let mut model = Model::new(&runtime).unwrap();
        model.key(KeyCode::Tab, &runtime).unwrap();
        model.key(KeyCode::Char('?'), &runtime).unwrap();
        let help = screen(&model);
        assert!(help.contains("Refresh and app updates"));
        assert!(help.contains("does not install updates or contact GitHub"));
        assert!(help.contains("does not update the Rhyven program"));
        model.key(KeyCode::Esc, &runtime).unwrap();
        model.key(KeyCode::Tab, &runtime).unwrap();
        model.key(KeyCode::Char('u'), &runtime).unwrap();
        assert!(model.message.contains("Press i"));
        let mut package = inventory();
        runtime.install(&package, true, false).unwrap();
        package["version"] = serde_json::json!("0.4.0");
        catalog::publish(&runtime.root, &package).unwrap();
        tick(&mut model, &runtime);
        model.query = "inventory".into();
        model.selected = 0;
        model.key(KeyCode::Char('u'), &runtime).unwrap();
        let review = screen(&model);
        assert!(review.contains("Review app update"));
        assert!(review.contains("recovery backup"));
        assert!(review.contains("Added permissions: none"));
        assert!(review.contains("Removed permissions: none"));
        model.key(KeyCode::Char('n'), &runtime).unwrap();
        assert_eq!(
            runtime.describe("rhyven/inventory").unwrap()["version"],
            "0.3.0"
        );
    }
    #[test]
    fn click_details_after_scrolling_matches_agent_metadata_without_installing() {
        let dir = tempfile::tempdir().unwrap();
        let r = Runtime::new(dir.path(), "human").unwrap();
        let mut m = Model::new(&r).unwrap();
        m.selected = m.visible().len() - 1;
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 28)).unwrap();
        t.draw(|f| render(f, &m, &[])).unwrap();
        let offset = m.list_offset.get();
        assert!(offset > 0);
        let area = m.list_area.get();
        m.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: area.x + 3,
                row: area.y + 1,
                modifiers: event::KeyModifiers::NONE,
            },
            &r,
        )
        .unwrap();
        assert_eq!(m.selected, offset);
        assert_eq!(m.pane, 3);
        assert!(!m.review);
        assert_eq!(r.apps().unwrap(), serde_json::json!([]));
        let p = m.current().unwrap();
        let listings=r.call("query",serde_json::json!({"app":"rhyven/marketplace","object":"listing","filters":{"name":p["name"]}})).unwrap();
        assert_eq!(
            listings["items"][0]["data"]["description"],
            p["description"]
        );
        t.draw(|f| render(f, &m, &[])).unwrap();
        let text: String = t
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("App details"));
        assert!(!text.contains("Headless apps for agents") && !text.contains("TOMORROW"));
        m.key(KeyCode::Esc, &r).unwrap();
        m.key(KeyCode::Enter, &r).unwrap();
        assert_eq!(m.pane, 3);
        assert!(!m.review);
        m.key(KeyCode::Char('i'), &r).unwrap();
        assert!(m.review);
        m.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: area.x + 3,
                row: area.y + 1,
                modifiers: event::KeyModifiers::NONE,
            },
            &r,
        )
        .unwrap();
        assert!(m.review);
        assert_eq!(r.apps().unwrap(), serde_json::json!([]));
    }

    #[test]
    fn update_remove_and_reinstall_preserve_state() {
        let dir = tempfile::tempdir().unwrap();
        let r = Runtime::new(dir.path(), "human").unwrap();
        let mut p = catalog::bundled()
            .into_iter()
            .find(|p| p["name"] == "rhyven/inventory")
            .unwrap();
        p["name"] = serde_json::json!("acme/assets");
        p["publisher"] = serde_json::json!("acme");
        p["version"] = serde_json::json!("0.1.0");
        r.install(&p, true, false).unwrap();
        let record = r.call("create", serde_json::json!({"app":"acme/assets","object":"asset","data":{"label":"Keep me","serial":"S1"}})).unwrap();
        let mut m = Model::new(&r).unwrap();
        m.key(KeyCode::Tab, &r).unwrap();
        assert_eq!(m.visible().len(), 1); // Installed even without a registry entry.
        p["version"] = serde_json::json!("0.2.0");
        catalog::publish(&r.root, &p).unwrap();
        m.key(KeyCode::Char('r'), &r).unwrap();
        assert!(m.versions("acme/assets").contains("Update available"));
        for installed_view in [false, true] {
            m.installed_only = installed_view;
            m.query = "acme/assets".into();
            assert_eq!(m.visible().len(), 1);
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 40)).unwrap();
            terminal.draw(|f| render(f, &m, &m.installed)).unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(text.contains("v0.1.0 → v0.2.0"));
            assert!(text.contains("Update app [u]"));
        }

        m.key(KeyCode::Char('u'), &r).unwrap();
        m.key(KeyCode::Esc, &r).unwrap();
        assert_eq!(r.describe("acme/assets").unwrap()["version"], "0.1.0");
        m.key(KeyCode::Char('u'), &r).unwrap();
        m.key(KeyCode::Char('y'), &r).unwrap();
        assert_eq!(r.describe("acme/assets").unwrap()["version"], "0.2.0");
        assert!(m.update_for("acme/assets").is_none());
        p["version"] = serde_json::json!("0.3.0");
        // Structurally valid, but incompatible with the installed schema.
        p["objects"]["asset"]["schema"]["properties"]["serial"] =
            serde_json::json!({"type":"integer"});
        catalog::publish(&r.root, &p).unwrap();
        m.key(KeyCode::Char('r'), &r).unwrap();
        m.key(KeyCode::Char('u'), &r).unwrap();
        m.key(KeyCode::Char('y'), &r).unwrap();
        assert_eq!(r.describe("acme/assets").unwrap()["version"], "0.2.0");
        m.key(KeyCode::Char('x'), &r).unwrap();
        m.key(KeyCode::Char('n'), &r).unwrap();
        assert_eq!(m.visible().len(), 1);
        m.key(KeyCode::Char('x'), &r).unwrap();
        m.key(KeyCode::Char('y'), &r).unwrap();
        assert!(m.visible().is_empty());
        assert_eq!(m.selected, 0);
        let compatible = catalog::resolve(&r.root, "acme/assets@0.2.0").unwrap();
        r.install(&compatible, true, false).unwrap();
        assert_eq!(
            r.call(
                "get",
                serde_json::json!({"app":"acme/assets","object":"asset","id":record["id"]})
            )
            .unwrap(),
            record
        );
    }
    #[test]
    fn installed_view_and_responsive_rendering() {
        let dir = tempfile::tempdir().unwrap();
        let r = Runtime::new(dir.path(), "human").unwrap();
        let mut m = Model::new(&r).unwrap();
        m.key(KeyCode::Tab, &r).unwrap();
        assert!(m.visible().is_empty());
        m.key(KeyCode::Tab, &r).unwrap();
        m.key(KeyCode::Char('i'), &r).unwrap();
        m.key(KeyCode::Char('y'), &r).unwrap();
        m.key(KeyCode::Tab, &r).unwrap();
        assert_eq!(m.visible().len(), 1);
        for (width, height) in [(140, 40), (80, 24), (48, 16), (20, 8)] {
            let mut t =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            t.draw(|f| render(f, &m, &m.installed)).unwrap();
            let text = t
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            assert!(!text.contains("Headless apps") && !text.contains("TOMORROW"));
            if width >= 48 {
                assert!(text.contains("Installed"));
            }
        }
        m.key(KeyCode::Char('s'), &r).unwrap();
        m.key(KeyCode::Esc, &r).unwrap();
        assert_eq!(m.pane, 0);
    }
    #[test]
    fn install_requires_review_and_explicit_acceptance() {
        let dir = tempfile::tempdir().unwrap();
        let r = Runtime::new(dir.path(), "human").unwrap();
        let mut m = Model::new(&r).unwrap();
        m.key(KeyCode::Char('i'), &r).unwrap();
        assert!(m.review);
        assert_eq!(r.apps().unwrap(), serde_json::json!([]));
        m.key(KeyCode::Char('n'), &r).unwrap();
        assert_eq!(r.apps().unwrap(), serde_json::json!([]));
        m.key(KeyCode::Char('i'), &r).unwrap();
        m.key(KeyCode::Char('y'), &r).unwrap();
        assert_eq!(r.apps().unwrap().as_array().unwrap().len(), 1);
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut t = ratatui::Terminal::new(backend).unwrap();
        t.draw(|f| render(f, &m, &[])).unwrap();
    }
}
