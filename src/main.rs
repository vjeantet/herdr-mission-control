mod ansi;
mod herdr;
mod ui;

use std::process::ExitCode;
use std::time::Duration;

use herdr::HerdrClient;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::text::Line;

const REFRESH_INTERVAL: Duration = Duration::from_millis(250);

pub struct Tile {
    pub pane_id: String,
    pub title: String,
    pub agent: Option<String>,
    pub status: String,
    pub focused: bool,
    pub preview: Vec<Line<'static>>,
}

pub struct Section {
    pub tab_id: String,
    pub label: String,
    pub number: usize,
    pub zoomed: bool,
    pub tiles: Vec<Tile>,
}

pub struct App {
    pub sections: Vec<Section>,
    /// (section index, tile index inside section)
    pub selected: (usize, usize),
    /// Persistent scroll offset in virtual rows, adjusted by the renderer.
    pub scroll: u16,
    /// A close confirmation dialog is open for the selected pane.
    pub confirming_close: bool,
}

impl App {
    /// Selected pane id plus its tab's zoom state.
    fn selected_pane(&self) -> Option<(&str, bool)> {
        self.sections
            .get(self.selected.0)
            .and_then(|s| s.tiles.get(self.selected.1).map(|t| (t.pane_id.as_str(), s.zoomed)))
    }

    fn move_horizontal(&mut self, delta: isize) {
        let Some(section) = self.sections.get(self.selected.0) else {
            return;
        };
        let len = section.tiles.len() as isize;
        if len == 0 {
            return;
        }
        let col = (self.selected.1 as isize + delta).rem_euclid(len);
        self.selected.1 = col as usize;
    }

    // Clamped, no wrap-around: jumping from bottom back to top (and the
    // scroll snap that comes with it) is disorienting.
    fn move_vertical(&mut self, delta: isize) {
        let len = self.sections.len() as isize;
        if len == 0 {
            return;
        }
        let row = (self.selected.0 as isize + delta).clamp(0, len - 1);
        self.selected.0 = row as usize;
        let max = self.sections[self.selected.0].tiles.len().saturating_sub(1);
        self.selected.1 = self.selected.1.min(max);
    }

    /// Live refresh: previews, agent statuses and zoom states are re-read
    /// in place. The section/tile structure stays frozen from open time so
    /// the selection and spatial layout remain stable; a pane closed in the
    /// meantime just keeps its last preview.
    fn refresh(&mut self, client: &HerdrClient) {
        for section in &mut self.sections {
            for tile in &mut section.tiles {
                if let Ok(text) = client.pane_read(&tile.pane_id) {
                    tile.preview = ansi::parse(&text);
                }
            }
        }
        let Ok(snapshot) = client.snapshot() else {
            return;
        };
        for section in &mut self.sections {
            if let Some(layout) = snapshot
                .layouts
                .iter()
                .find(|l| l.tab_id == section.tab_id)
            {
                section.zoomed = layout.zoomed;
            }
            for tile in &mut section.tiles {
                if let Some(pane) = snapshot.panes.iter().find(|p| p.pane_id == tile.pane_id) {
                    tile.status = pane.agent_status.clone();
                }
            }
        }
    }

    /// Direct jump: nth tile in flattened order (1-based).
    fn select_nth(&mut self, n: usize) -> bool {
        let mut count = 0;
        for (si, section) in self.sections.iter().enumerate() {
            for ti in 0..section.tiles.len() {
                count += 1;
                if count == n {
                    self.selected = (si, ti);
                    return true;
                }
            }
        }
        false
    }
}

fn build_app(client: &HerdrClient) -> Result<App, String> {
    let snapshot = client.snapshot()?;
    let workspace_id = std::env::var("HERDR_WORKSPACE_ID")
        .ok()
        .or(snapshot.focused_workspace_id)
        .ok_or("no current workspace")?;

    let mut sections = Vec::new();
    let mut selected = (0usize, 0usize);
    for tab in snapshot
        .tabs
        .iter()
        .filter(|t| t.workspace_id == workspace_id)
    {
        let mut tiles = Vec::new();
        for pane in snapshot
            .panes
            .iter()
            .filter(|p| p.tab_id == tab.tab_id)
        {
            let preview = ansi::parse(
                &client
                    .pane_read(&pane.pane_id)
                    .unwrap_or_else(|err| format!("(read error: {err})")),
            );
            let title = pane
                .title
                .clone()
                .or_else(|| pane.terminal_title_stripped.clone())
                .unwrap_or_else(|| pane.pane_id.clone());
            if pane.focused && tab.focused {
                selected = (sections.len(), tiles.len());
            }
            tiles.push(Tile {
                pane_id: pane.pane_id.clone(),
                title,
                agent: pane.display_agent.clone().or_else(|| pane.agent.clone()),
                status: pane.agent_status.clone(),
                focused: pane.focused,
                preview,
            });
        }
        let zoomed = snapshot
            .layouts
            .iter()
            .find(|l| l.tab_id == tab.tab_id)
            .is_some_and(|l| l.zoomed);
        sections.push(Section {
            tab_id: tab.tab_id.clone(),
            label: tab.label.clone(),
            number: tab.number,
            zoomed,
            tiles,
        });
    }

    if sections.iter().all(|s| s.tiles.is_empty()) {
        return Err(format!("no panes in workspace {workspace_id}"));
    }
    Ok(App {
        sections,
        selected,
        scroll: 0,
        confirming_close: false,
    })
}

enum Outcome {
    Cancel,
    Switch { pane_id: String, tab_zoomed: bool },
}

fn run(app: &mut App, client: &HerdrClient) -> std::io::Result<Outcome> {
    let mut terminal = ratatui::try_init()?;
    let outcome = loop {
        terminal.draw(|frame| ui::draw(frame, app))?;
        if !event::poll(REFRESH_INTERVAL)? {
            app.refresh(client);
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if app.confirming_close {
            match key.code {
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('o') => {
                    app.confirming_close = false;
                    if let Some((id, _)) = app.selected_pane() {
                        let id = id.to_string();
                        if client.pane_close(&id).is_ok() {
                            // Stay in Mission Control for chained closes: rebuild
                            // the grid (herdr may have cascaded tab/workspace
                            // closes) and follow its new focused pane. Exit
                            // only when nothing is left to show.
                            match build_app(client) {
                                Ok(rebuilt) => *app = rebuilt,
                                Err(_) => break Outcome::Cancel,
                            }
                        }
                    }
                }
                KeyCode::Esc | KeyCode::Char('n') => app.confirming_close = false,
                _ => {}
            }
            continue;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => break Outcome::Cancel,
            KeyCode::Backspace => {
                if app.selected_pane().is_some() {
                    app.confirming_close = true;
                }
            }
            KeyCode::Left | KeyCode::Char('h') => app.move_horizontal(-1),
            KeyCode::Right | KeyCode::Char('l') => app.move_horizontal(1),
            KeyCode::Up | KeyCode::Char('k') => app.move_vertical(-1),
            KeyCode::Down | KeyCode::Char('j') => app.move_vertical(1),
            KeyCode::Enter => {
                if let Some((id, tab_zoomed)) = app.selected_pane() {
                    break Outcome::Switch {
                        pane_id: id.to_string(),
                        tab_zoomed,
                    };
                }
            }
            KeyCode::Char(c @ '1'..='9') => {
                let n = c as usize - '0' as usize;
                if app.select_nth(n) {
                    if let Some((id, tab_zoomed)) = app.selected_pane() {
                        break Outcome::Switch {
                            pane_id: id.to_string(),
                            tab_zoomed,
                        };
                    }
                }
            }
            _ => {}
        }
    };
    ratatui::restore();
    Ok(outcome)
}

fn main() -> ExitCode {
    let client = match HerdrClient::from_env() {
        Ok(client) => client,
        Err(err) => {
            eprintln!("herdr-mission-control: {err}");
            return ExitCode::FAILURE;
        }
    };

    let mut app = match build_app(&client) {
        Ok(app) => app,
        Err(err) => {
            eprintln!("herdr-mission-control: {err}");
            return ExitCode::FAILURE;
        }
    };

    let outcome = match run(&mut app, &client) {
        Ok(outcome) => outcome,
        Err(err) => {
            ratatui::restore();
            eprintln!("herdr-mission-control: terminal error: {err}");
            return ExitCode::FAILURE;
        }
    };

    // Focus is applied after the TUI is torn down but before the process
    // exits (which closes the popup). Risk under validation: the popup close
    // must not restore the previous focus over this one.
    match outcome {
        Outcome::Cancel => {}
        Outcome::Switch {
            pane_id,
            tab_zoomed,
        } => {
            if let Err(err) = client.focus_pane(&pane_id, tab_zoomed) {
                eprintln!("herdr-mission-control: focus {pane_id} failed: {err}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}
