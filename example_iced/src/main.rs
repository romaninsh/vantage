use dataset_ui_adapters::{
    iced_adapter::{IcedTable, TableMessage},
    MockProductDataSet, TableStore,
};
use iced::widget::{Column, Container, Text};
use iced::{Application, Command, Element, Settings, Theme};

#[derive(Debug)]
struct TableApp {
    table: IcedTable<MockProductDataSet>,
}

#[derive(Debug, Clone)]
enum Message {
    Table(TableMessage),
}

impl Application for TableApp {
    type Message = Message;
    type Theme = Theme;
    type Executor = iced::executor::Default;
    type Flags = ();

    fn new(_flags: Self::Flags) -> (Self, Command<Self::Message>) {
        // Create mock dataset
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let table = IcedTable::new(store);

        let app = Self { table };

        // Load data on startup
        (
            app,
            Command::perform(async {}, |_| Message::Table(TableMessage::LoadData)),
        )
    }

    fn title(&self) -> String {
        String::from("Dataset UI Adapters - Iced Example")
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match message {
            Message::Table(table_msg) => {
                let command = self.table.update(table_msg);
                command.map(Message::Table)
            }
        }
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let header = Text::new("Dataset UI Adapters - Iced Table Example").size(24);

        let table_view = self.table.view().map(Message::Table);

        Container::new(Column::new().spacing(20).push(header).push(table_view))
            .padding(20)
            .into()
    }
}

#[tokio::main]
async fn main() -> iced::Result {
    TableApp::run(Settings::default())
}
