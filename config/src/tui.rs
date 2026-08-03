// Interactive scenario picker (ratatui). Lists every scenario discovered under
// scenarios/ as a checkbox; Space toggles, `s` saves the selection to
// .aio/enabled.toml and quits, `q` quits without saving. Pre-checks the ids
// already in the manifest. No search/category (MVP: few scenarios; design §7).

use std::io;
use std::path::Path;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Terminal;

use crate::manifest;
use crate::scenario;

pub fn run(repo: &Path) -> Result<()> {
    let scenarios = scenario::scan(&repo.join("scenarios"))?;
    let manifest_path = repo.join(".aio/enabled.toml");
    let enabled = manifest::load(&manifest_path)?;
    let mut checked: Vec<bool> = scenarios
        .iter()
        .map(|s| enabled.scenarios.iter().any(|e| e == &s.meta.id))
        .collect();

    let mut state = ListState::default();
    if !scenarios.is_empty() {
        state.select(Some(0));
    }

    // Raw mode + alternate screen. The TerminalGuard restores the terminal on
    // drop (RAII) so the user's terminal is never left in raw mode, even if the
    // TUI errors or panics.
    enable_raw_mode().context("enable raw mode")?;
    let _guard = TerminalGuard;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    let mut saved = false;
    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(3)])
                .split(f.area());
            let items: Vec<ListItem> = scenarios
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let mark = if checked[i] { "[x]" } else { "[ ]" };
                    ListItem::new(Line::from(vec![
                        Span::raw(format!("{} {}  ", mark, s.meta.name)),
                        Span::styled(
                            s.meta.description.clone(),
                            Style::default().add_modifier(Modifier::DIM),
                        ),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("AIO 开发场景  (空格=切换  s=保存退出  q=不存退出  ↑↓=移动)"),
                )
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
            f.render_stateful_widget(list, chunks[0], &mut state);
            let help = Paragraph::new(format!(
                "已选 {} / 共 {}",
                checked.iter().filter(|&&c| c).count(),
                scenarios.len()
            ));
            f.render_widget(help, chunks[1]);
        })?;

        if !event::poll(std::time::Duration::from_millis(250))? {
            continue;
        }
        if let Event::Key(k) = event::read()? {
            // React only to Press to avoid double-fire on some terminals.
            if k.kind != KeyEventKind::Press {
                continue;
            }
            let len = scenarios.len();
            match k.code {
                KeyCode::Char('q') => break,
                KeyCode::Char('s') => {
                    saved = true;
                    break;
                }
                KeyCode::Down if len > 0 => {
                    let i = state.selected().unwrap_or(0);
                    state.select(Some((i + 1) % len));
                }
                KeyCode::Up if len > 0 => {
                    let i = state.selected().unwrap_or(0);
                    state.select(Some((i + len - 1) % len));
                }
                KeyCode::Char(' ') => {
                    if let Some(i) = state.selected() {
                        checked[i] = !checked[i];
                    }
                }
                _ => {}
            }
        }
    }

    // terminal + _guard drop here, restoring the screen / raw mode.
    drop(terminal);
    if saved {
        let new = manifest::Enabled {
            scenarios: scenarios
                .iter()
                .enumerate()
                .filter_map(|(i, s)| if checked[i] { Some(s.meta.id.clone()) } else { None })
                .collect(),
        };
        manifest::save(&manifest_path, &new)?;
        println!("saved {} scenario(s)", new.scenarios.len());
    }
    Ok(())
}

/// Restores the terminal (disable raw mode + leave alternate screen) on drop.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}
