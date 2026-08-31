// Interactive scenario picker (ratatui). Lists every scenario discovered under
// scenarios/ grouped by its profile layer (`category` in scenario.toml):
// L1 os / L2 shell / L3 lang / L4 app (L5 service future). L1's version-
// selectable parts (node, python) are `always_on` scenarios shown as locked
// rows `[*]` with a version [label] cyclable via Left/Right; the non-versioned
// L1 infra stays in Dockerfile.base.head and never appears here. Space toggles
// selectable items (no-op on headers and always_on rows); `s` saves the
// selection to .aio/enabled.toml and quits, `q` quits without saving.
// Pre-checks the ids already in the manifest; a `scenarios = ["*"]` entry (the
// full preset) is expanded to every selectable scenario so all boxes pre-check,
// and saving writes back the concrete explicit list.
//
// Rows are a flat list interleaving non-selectable category headers and item
// rows. Navigation (Up/Down) walks all rows. `checked` (Vec<bool>) and
// `version_sel` (Vec<Option<String>>) stay indexed by scenario, so inserting
// header rows never shifts their state.

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
    // Expand a "["*"]" manifest (e.g. the full preset) to all discovered
    // selectable scenarios so the picker pre-checks every box; saving writes
    // back the concrete explicit list, never the wildcard (design §2.2).
    let selected = enabled.expand(&scenarios)?;
    let mut checked: Vec<bool> = scenarios
        .iter()
        .map(|s| selected.iter().any(|e| e == &s.meta.id))
        .collect();

    // Per-scenario selected version label (None for non-versioned). Initialized
    // from the manifest's version selection, else default_version, else the
    // first version. Left/Right cycles this for the selected versioned row.
    let mut version_sel: Vec<Option<String>> = scenarios
        .iter()
        .map(|s| {
            if s.meta.versions.is_empty() {
                None
            } else {
                let label = enabled
                    .versions
                    .iter()
                    .find(|vs| vs.id == s.meta.id)
                    .map(|vs| vs.label.clone())
                    .or_else(|| s.meta.default_version.clone())
                    .unwrap_or_else(|| s.meta.versions[0].label.clone());
                Some(label)
            }
        })
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
                        if s.meta.always_on {
                            // Locked row ([*]): always baked, Space is a no-op.
                            // Versioned => show the current version [label],
                            // cyclable with Left/Right.
                            let mut head = format!("    [*] {}  ", s.meta.name);
                            if let Some(label) = &version_sel[*scenario_idx] {
                                head.push_str(&format!("[{}]  ", label));
                            }
                            ListItem::new(Line::from(vec![
                                Span::raw(head),
                                Span::styled(
                                    s.meta.description.clone(),
                                    Style::default().add_modifier(Modifier::DIM),
                                ),
                            ]))
                        } else {
                            let mark = if checked[*scenario_idx] { "[x]" } else { "[ ]" };
                            ListItem::new(Line::from(vec![
                                Span::raw(format!("    {} {}  ", mark, s.meta.name)),
                                Span::styled(
                                    s.meta.description.clone(),
                                    Style::default().add_modifier(Modifier::DIM),
                                ),
                            ]))
                        }
                    }
                })
                .collect();
            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("AIO 开发场景 · 分层  (空格=切换  ←->=改版本  s=保存退出  q=不存退出  ↑↓=移动)"),
                )
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
            f.render_stateful_widget(list, chunks[0], &mut state);
            let selectable = scenarios.iter().filter(|s| !s.meta.always_on).count();
            let always_on_n = scenarios.len() - selectable;
            let checked_sel = scenarios
                .iter()
                .enumerate()
                .filter(|(i, s)| !s.meta.always_on && checked[*i])
                .count();
            let help = Paragraph::new(format!(
                "已选 {} / 可选 {}（必装 {}）",
                checked_sel, selectable, always_on_n
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
                        // Space toggles selectable items only; no-op on header
                        // rows and always_on rows (L1 node/python can't be
                        // unchecked - only their version is chosen).
                        if let Row::Item { scenario_idx } = rows[i] {
                            if !scenarios[scenario_idx].meta.always_on {
                                checked[scenario_idx] = !checked[scenario_idx];
                            }
                        }
                    }
                }
                KeyCode::Left => cycle_version(
                    &scenarios,
                    &mut version_sel,
                    &rows,
                    state.selected(),
                    -1,
                ),
                KeyCode::Right => cycle_version(
                    &scenarios,
                    &mut version_sel,
                    &rows,
                    state.selected(),
                    1,
                ),
                _ => {}
            }
        }
    }

    // terminal + _guard drop here, restoring the screen / raw mode.
    drop(terminal);
    if saved {
        // Selectable checked scenarios (always_on excluded - always baked).
        let scenarios_out: Vec<String> = scenarios
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                if !s.meta.always_on && checked[i] {
                    Some(s.meta.id.clone())
                } else {
                    None
                }
            })
            .collect();
        // Version selections for every versioned scenario (always_on or not).
        let versions_out: Vec<manifest::VersionSelect> = scenarios
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                if s.meta.versions.is_empty() {
                    return None;
                }
                version_sel[i].clone().map(|label| manifest::VersionSelect {
                    id: s.meta.id.clone(),
                    label,
                })
            })
            .collect();
        let new = manifest::Enabled {
            scenarios: scenarios_out,
            versions: versions_out,
        };
        manifest::save(&manifest_path, &new)?;
        println!(
            "saved {} scenario(s), {} version(s)",
            new.scenarios.len(),
            new.versions.len()
        );
    }
    Ok(())
}

/// Cycle the selected version of the currently-selected versioned row by `dir`
/// (+1 = next, -1 = prev). No-op on headers, non-versioned items, or items with
/// fewer than 2 versions.
fn cycle_version(
    scenarios: &[scenario::Scenario],
    version_sel: &mut [Option<String>],
    rows: &[Row],
    selected: Option<usize>,
    dir: i32,
) {
    let Some(i) = selected else { return };
    let Row::Item { scenario_idx } = rows[i] else { return };
    let s = &scenarios[scenario_idx];
    if s.meta.versions.len() < 2 {
        return;
    }
    let cur = version_sel[scenario_idx]
        .clone()
        .unwrap_or_else(|| s.meta.versions[0].label.clone());
    let pos = s
        .meta
        .versions
        .iter()
        .position(|v| v.label == cur)
        .unwrap_or(0);
    let n = s.meta.versions.len();
    let next = ((pos as i32 + dir + n as i32) % n as i32) as usize;
    version_sel[scenario_idx] = Some(s.meta.versions[next].label.clone());
}

/// Restores the terminal (disable raw mode + leave alternate screen) on drop.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}
