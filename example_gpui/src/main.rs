use bakery_model3::*;
use dataset_ui_adapters::{gpui_adapter::GpuiTableDelegate, TableStore, VantageTableAdapter};
use gpui::*;
use gpui_component::{table::Table, v_flex, ActiveTheme, Root, StyledExt};
use tokio::runtime::Runtime;

actions!(example_gpui, [Quit]);

struct TableApp {
    table: Entity<Table<GpuiTableDelegate<VantageTableAdapter<Client>>>>,
}

impl TableApp {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Connect to SurrealDB and get client table
        let rt = Runtime::new().expect("Failed to create tokio runtime");

        let client_table = rt.block_on(async {
            bakery_model3::connect_surrealdb()
                .await
                .expect("Failed to connect to SurrealDB");
            Client::table()
        });

        let dataset = rt.block_on(async { VantageTableAdapter::new(client_table).await });
        let store = TableStore::new(dataset);
        let delegate = GpuiTableDelegate::new(store);
        let table = cx.new(|cx| Table::new(delegate, window, cx).stripe(true).border(true));

        Self { table }
    }

    fn view(window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self::new(window, cx))
    }
}

impl Render for TableApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .p_4()
            .gap_4()
            .bg(cx.theme().background)
            .child(
                div()
                    .text_2xl()
                    .font_bold()
                    .text_color(cx.theme().foreground)
                    .child("Bakery Model 3 - GPUI Client List"),
            )
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("Real SurrealDB data using Vantage 0.3 architecture"),
            )
            .child(self.table.clone())
    }
}

fn main() {
    let app = Application::new();

    app.run(move |cx| {
        gpui_component::init(cx);

        // Set up quit action and key binding
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.on_action(|_: &Quit, cx: &mut App| {
            cx.quit();
        });

        // Set up simple menu with just Quit
        cx.set_menus(vec![Menu {
            name: "Bakery Model 3".into(),
            items: vec![MenuItem::action("Quit", Quit)],
        }]);

        cx.activate(true);

        let window_size = size(px(1000.), px(700.));
        let window_bounds = Bounds::centered(None, window_size, cx);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(window_bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Bakery Model 3 - Client List".into()),
                appears_transparent: false,
                traffic_light_position: None,
            }),
            window_min_size: Some(size(px(600.), px(400.))),
            kind: WindowKind::Normal,
            ..Default::default()
        };

        cx.open_window(options, |window, cx| {
            let table_app = TableApp::view(window, cx);
            cx.new(|cx| Root::new(table_app.into(), window, cx))
        })
        .unwrap();
    });
}
