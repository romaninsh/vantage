use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use dataset_ui_adapters::{ratatui_adapter::RatatuiTableAdapter, TableStore, VantageTableAdapter};
use bakery_model3::*;
use ratatui::{
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Style},
    text::Text,
    widgets::{Block, BorderType, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    DefaultTerminal, Frame,
};

const INFO_TEXT: [&str; 2] = [
    "(Esc) quit | (↑) move up | (↓) move down",
    "Bakery Model 3 - Ratatui Client List",
];

struct App<D: dataset_ui_adapters::DataSet + 'static> {
    adapter: RatatuiTableAdapter<D>,
    scroll_state: ScrollbarState,
}

impl<D: dataset_ui_adapters::DataSet + 'static> App<D> {
    async fn new(store: TableStore<D>) -> Self {
        let mut adapter = RatatuiTableAdapter::new(store);
        adapter.refresh_data().await;

        let scroll_state = ScrollbarState::new(adapter.row_count().saturating_sub(1));

        Self {
            adapter,
            scroll_state,
        }
    }

    fn next_row(&mut self) {
        self.adapter.next_row();
        if let Some(selected) = self.adapter.selected_row() {
            self.scroll_state = self.scroll_state.position(selected);
        }
    }

    fn previous_row(&mut self) {
        self.adapter.previous_row();
        if let Some(selected) = self.adapter.selected_row() {
            self.scroll_state = self.scroll_state.position(selected);
        }
    }

    fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        loop {
            terminal.draw(|frame| self.draw(frame))?;

            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                        KeyCode::Char('j') | KeyCode::Down => self.next_row(),
                        KeyCode::Char('k') | KeyCode::Up => self.previous_row(),
                        KeyCode::Char('r') => {
                            // Note: In a real app, you'd want to handle this async properly
                            // For now, we'll skip refresh in the event loop
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let vertical = Layout::vertical([Constraint::Min(5), Constraint::Length(3)]);
        let rects = vertical.split(frame.area());

        self.render_table(frame, rects[0]);
        self.render_scrollbar(frame, rects[0]);
        self.render_footer(frame, rects[1]);
    }

    fn render_table(&mut self, frame: &mut Frame, area: Rect) {
        let table = self
            .adapter
            .create_table()
            .block(
                Block::bordered()
                    .title("Bakery Model 3 - Client List")
                    .border_type(BorderType::Rounded),
            )
            .style(Style::default().bg(Color::Black));

        frame.render_stateful_widget(table, area, self.adapter.state_mut());
    }

    fn render_scrollbar(&mut self, frame: &mut Frame, area: Rect) {
        frame.render_stateful_widget(
            Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            area.inner(Margin {
                vertical: 1,
                horizontal: 1,
            }),
            &mut self.scroll_state,
        );
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let info_footer = Paragraph::new(Text::from_iter(INFO_TEXT))
            .style(Style::new().fg(Color::White).bg(Color::Black))
            .centered()
            .block(
                Block::bordered()
                    .border_type(BorderType::Double)
                    .border_style(Style::new().fg(Color::Blue)),
            );
        frame.render_widget(info_footer, area);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    // Connect to SurrealDB and get client table
    bakery_model3::connect_surrealdb()
        .await
        .expect("Failed to connect to SurrealDB");
    let client_table = Client::table();

    // Create the dataset and table store
    let dataset = VantageTableAdapter::new(client_table).await;
    let store = TableStore::new(dataset);

    println!("Starting Bakery Model 3 - Ratatui Client List...");
    println!("Controls: ↑/↓ navigate, q/Esc to quit, r to refresh");
    println!("Real SurrealDB data using Vantage 0.3 architecture");

    // Create and run the app
    let terminal = ratatui::init();
    let app = App::new(store).await;
    let app_result = app.run(terminal);
    ratatui::restore();

    app_result
}
