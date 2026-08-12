// Interactive scenario picker (ratatui). Lists every scenario discovered under
// scenarios/ grouped by its profile layer (`category` in scenario.toml):
// L2 shell / L3 lang / L4 app (L5 service future). L1 ("os") lives in
// Dockerfile.base.head and is NOT a selectable scenario, so it never appears
// here. Space toggles, `s` saves the selection to .aio/enabled.toml and quits,
// `q` quits without saving. Pre-checks the ids already in the manifest.
//
// Rows are a flat list interleaving non-selectable category headers and
// selectable item checkboxes. Navigation (Up/Down) walks all rows; Space is a
// no-op on header rows. `checked` (Vec<bool>) stays indexed by scenario, so
// inserting header rows never shifts checkbox state.

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

/// A renderable list row: either a category header (non-selectable) or a
/// scenario item (checkbox). `scenario_idx` indexes the `scenarios` Vec and the
/// parallel `checked` Vec, independent of where headers fall in the row list.
enum Row {
    Header { category: String },
    Item { scenario_idx: usize },
}

pub fn run(repo: &Path) -> Result<()> {
    let scenarios = scenario::scan(&repo.join("scenarios"))?;
    let manifest_path = repo.join(".aio/enabled.toml");
    let enabled = manifest::load(&manifest_path)?;
    let mut checked: Vec<bool> = scenarios
        .iter()
        .map(|s| enabled.scenarios.iter().any(|e| e == &s.meta.id))
        .collect();

    // Order scenarios by (category_rank, id), then build the interleaved row
    // list: a header row before each new category, followed by its items.
    let mut order: Vec<usize> = (0..scenarios.len()).collect();
    order.sort_by_key(|&i| {
        (
            scenario::category_rank(&scenarios[i].meta.category),
            scenarios[i].meta.id.clone(),
        )
    });
    let mut rows: Vec<Row> = Vec::new();
    let mut last_cat: Option<String> = None;
    for &i in &order {
        let cat = scenarios[i].meta.category.clone();
        if last_cat.as_deref() != Some(cat.as_str()) {
            rows.push(Row::Header { category: cat.clone() });
            last_cat = Some(cat);
        }
        rows.push(Row::Item { scenario_idx: i });
    }

    let mut state = ListState::default();
    if !rows.is_empty() {
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
            let items: Vec<ListItem> = rows
                .iter()
                .map(|r| match r {
                    Row::Header { category } => ListItem::new(Line::from(vec![Span::styled(
                        format!("  {}", scenario::category_title(category)),
                        Style::default().add_modifier(Modifier::BOLD),
                    )])),
                    Row::Item { scenario_idx } => {
                        let s = &scenarios[*scenario_idx];
                        let mark = if checked[*scenario_idx] { "[x]" } else { "[ ]" };
                        ListItem::new(Line::from(vec![
                            Span::raw(format!("    {} {}  ", mark, s.meta.name)),
                            Span::styled(
                                s.meta.description.clone(),
                                Style::default().add_modifier(Modifier::DIM),
                            ),
                        ]))
                    }
                })
                .collect();
            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("AIO 开发场景 · 分层  (空格=切换  s=保存退出  q=不存退出  ↑↓=移动)"),
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
            let nrows = rows.len();
            match k.code {
                KeyCode::Char('q') => break,
                KeyCode::Char('s') => {
                    saved = true;
                    break;
                }
                KeyCode::Down if nrows > 0 => {
                    let i = state.selected().unwrap_or(0);
                    state.select(Some((i + 1) % nrows));
                }
                KeyCode::Up if nrows > 0 => {
                    let i = state.selected().unwrap_or(0);
                    state.select(Some((i + nrows - 1) % nrows));
                }
                KeyCode::Char(' ') => {
                    if let Some(i) = state.selected() {
                        // Space is a no-op on header rows (only items toggle).
                        if let Row::Item { scenario_idx } = rows[i] {
                            checked[scenario_idx] = !checked[scenario_idx];
                        }
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
