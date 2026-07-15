//! Ratatui binding for a Dio-backed [`TableScenery`] — a scrollable,
//! self-refreshing terminal table.
//!
//! [`SceneryTable`] owns the whole terminal loop. Rendering is virtualized —
//! only the visible band becomes widgets, so six-figure row counts stay
//! cheap — and the visible range is forwarded to the scenery as its
//! viewport, which is what drives detail-pass hydration: rows fetch their
//! augment columns as they come into view. The table repaints whenever the
//! scenery's generation moves, and likewise for every [`ValueScenery`]
//! pinned to the status bar.
//!
//! ```ignore
//! ratatui_dio::SceneryTable::new(scenery)
//!     .with_column("FILENAME", "Key", 0) // width 0 = flexible fill
//!     .with_column("SIZE", "Size", 10)
//!     .with_status_value("total rows", totals)
//!     .run()
//!     .await?;
//! ```

use std::sync::Arc;

use ciborium::Value as CborValue;

/// How many rows around the cursor are declared as the scenery's viewport —
/// i.e. sent through the detail pass. Detail fetches cost a network
/// round-trip each, so the band tracks what the user is actually looking
/// at rather than the whole screen.
const HYDRATE_BAND: usize = 10;
use crossterm::event::{Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Row, Table, TableState};
use ratatui::Frame;
use tokio::sync::{mpsc, watch};
use vantage_diorama::{Generation, TableScenery, ValueScenery};

/// One displayed column: header, the record field it reads, and a fixed
/// width (`0` = flexible fill).
struct SceneryColumn {
    header: String,
    field: String,
    width: u16,
}

/// A scrollable terminal table over a [`TableScenery`], with reactive
/// [`ValueScenery`] readouts in its status bar.
pub struct SceneryTable {
    scenery: Arc<dyn TableScenery>,
    columns: Vec<SceneryColumn>,
    status_values: Vec<(String, Arc<dyn ValueScenery>)>,
}

impl SceneryTable {
    pub fn new(scenery: Arc<dyn TableScenery>) -> Self {
        Self {
            scenery,
            columns: Vec::new(),
            status_values: Vec::new(),
        }
    }

    /// Add a column reading `field` off each record. `width` is a fixed cell
    /// width; `0` makes the column flexible. Cells whose field is absent
    /// render as `…` — the un-hydrated look.
    pub fn with_column(
        mut self,
        header: impl Into<String>,
        field: impl Into<String>,
        width: u16,
    ) -> Self {
        self.columns.push(SceneryColumn {
            header: header.into(),
            field: field.into(),
            width,
        });
        self
    }

    /// Pin a reactive value to the status bar, shown as `label value`.
    pub fn with_status_value(
        mut self,
        label: impl Into<String>,
        value: Arc<dyn ValueScenery>,
    ) -> Self {
        self.status_values.push((label.into(), value));
        self
    }

    /// Take over the terminal until `q`/`Esc`. `↑`/`↓`/`PgUp`/`PgDn`/
    /// `Home`/`End` scroll (moving the scenery's viewport with the screen),
    /// `r` asks the Dio to refresh.
    pub async fn run(self) -> std::io::Result<()> {
        let SceneryTable {
            scenery,
            columns,
            status_values,
        } = self;
        let mut terminal = ratatui::init();

        // Keyboard events, forwarded from a blocking reader thread.
        let (key_tx, mut keys) = mpsc::unbounded_channel();
        std::thread::spawn(move || {
            while let Ok(event) = crossterm::event::read() {
                if key_tx.send(event).is_err() {
                    break;
                }
            }
        });

        // Any generation movement — the table's or a status value's — is one
        // repaint tick.
        let (tick_tx, mut ticks) = mpsc::unbounded_channel();
        forward_generation(scenery.subscribe(), tick_tx.clone());
        for (_, value) in &status_values {
            forward_generation(value.subscribe(), tick_tx.clone());
        }

        let mut selected: usize = 0;
        let mut offset: usize = 0;

        loop {
            let height = (terminal.size()?.height.saturating_sub(2) as usize).max(1);
            let total = scenery.row_count();
            selected = selected.min(total.saturating_sub(1));
            // Keep the selection inside the visible band.
            if selected < offset {
                offset = selected;
            }
            if selected >= offset + height {
                offset = selected + 1 - height;
            }

            terminal
                .draw(|frame| draw(frame, &scenery, &columns, &status_values, selected, offset))?;

            // Hydrate a small band around the cursor — the rows the user is
            // actually looking at, not the whole screen.
            let band_start = selected.saturating_sub(HYDRATE_BAND / 2);
            scenery.set_viewport(band_start..(band_start + HYDRATE_BAND).min(total));

            tokio::select! {
                // recv() returning None means the channel closed (generation
                // forwarder or terminal input thread died) — exit cleanly
                // instead of spinning on a closed channel or going deaf to
                // input.
                res = ticks.recv() => {
                    if res.is_none() { break; }
                }
                res = keys.recv() => {
                    let Some(event) = res else { break };
                    let Event::Key(key) = event else { continue };
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    selected = match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Up => selected.saturating_sub(1),
                        KeyCode::Down => selected.saturating_add(1),
                        KeyCode::PageUp => selected.saturating_sub(height),
                        KeyCode::PageDown => selected.saturating_add(height),
                        KeyCode::Home => 0,
                        KeyCode::End => usize::MAX,
                        KeyCode::Char('r') => {
                            scenery.request_refresh();
                            continue;
                        }
                        _ => continue,
                    };
                }
            }
        }

        ratatui::restore();
        Ok(())
    }
}

/// Repaint-tick forwarder: one per subscribed generation channel.
fn forward_generation(mut rx: watch::Receiver<Generation>, tx: mpsc::UnboundedSender<()>) {
    tokio::spawn(async move {
        while rx.changed().await.is_ok() {
            if tx.send(()).is_err() {
                break;
            }
        }
    });
}

fn draw(
    frame: &mut Frame,
    scenery: &Arc<dyn TableScenery>,
    columns: &[SceneryColumn],
    status_values: &[(String, Arc<dyn ValueScenery>)],
    selected: usize,
    offset: usize,
) {
    let [main, footer] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());

    let height = (main.height.saturating_sub(1)) as usize; // minus header row
    let total = scenery.row_count();
    let end = (offset + height).min(total);

    let rows: Vec<Row> = (offset..end)
        .map(|idx| match scenery.row(idx) {
            Some(row) => Row::new(
                columns
                    .iter()
                    .map(|c| cell_text(row.record.get(&c.field)))
                    .collect::<Vec<_>>(),
            ),
            None => Row::new(vec!["…".to_string()]),
        })
        .collect();

    let constraints: Vec<Constraint> = columns
        .iter()
        .map(|c| {
            if c.width == 0 {
                Constraint::Min(10)
            } else {
                Constraint::Length(c.width)
            }
        })
        .collect();

    let mut state = TableState::default();
    state.select(selected.checked_sub(offset));
    let table = Table::new(rows, constraints)
        .header(
            Row::new(columns.iter().map(|c| c.header.clone()).collect::<Vec<_>>())
                .style(Style::new().add_modifier(Modifier::BOLD)),
        )
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(table, main, &mut state);

    let summary = scenery.status_summary();
    let mut status = format!(" {total} rows · {} augmented", summary.fresh);
    for (label, value) in status_values {
        status.push_str(&format!(" · {label} {}", cell_text(value.value().as_ref())));
    }
    status.push_str(" · ↑/↓ PgUp/PgDn scroll · r refresh · q quit");
    frame.render_widget(ratatui::text::Line::from(status), footer);
}

/// A cell's display text. Absent field → `…` (not hydrated yet).
fn cell_text(value: Option<&CborValue>) -> String {
    match value {
        None => "…".to_string(),
        Some(CborValue::Text(s)) => s.clone(),
        Some(CborValue::Integer(i)) => i128::from(*i).to_string(),
        Some(CborValue::Float(f)) => f.to_string(),
        Some(CborValue::Bool(b)) => b.to_string(),
        Some(_) => String::new(),
    }
}
