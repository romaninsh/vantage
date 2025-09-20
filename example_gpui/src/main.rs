use bakery_model3::*;
use dataset_ui_adapters::{gpui_adapter::GpuiTableDelegate, TableStore, VantageTableAdapter};
use gpui::*;
use gpui_component::{table::Table, v_flex, ActiveTheme, Root, StyledExt};

actions!(example_gpui, [Quit]);

struct TableApp {
    table: Entity<Table<GpuiTableDelegate<VantageTableAdapter<SurrealDB, Client>>>>,
}

impl TableApp {
    fn new(
        table: Entity<Table<GpuiTableDelegate<VantageTableAdapter<SurrealDB, Client>>>>,
    ) -> Self {
        Self { table }
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
    // Initialize SurrealDB before starting GPUI
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async {
        bakery_model3::connect_surrealdb()
            .await
            .expect("Failed to connect to SurrealDB");
    });

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

        let mut window_size = size(px(1000.), px(700.));
        if let Some(display) = cx.primary_display() {
            let display_size = display.bounds().size;
            window_size.width = window_size.width.min(display_size.width * 0.85);
            window_size.height = window_size.height.min(display_size.height * 0.85);
        }
        let window_bounds = Bounds::centered(None, window_size, cx);

        cx.spawn(async move |cx| {
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

            let client_table = Client::table();
            let dataset = VantageTableAdapter::new(client_table).await;
            let store = TableStore::new(dataset);
            let delegate = GpuiTableDelegate::new(store);

            let window = cx
                .open_window(options, |window, cx| {
                    let table =
                        cx.new(|cx| Table::new(delegate, window, cx).stripe(true).border(true));
                    let view = cx.new(|_| TableApp::new(table));
                    cx.new(|cx| Root::new(view.into(), window, cx))
                })
                .expect("failed to open window");

            window
                .update(cx, |_, window, _| {
                    window.activate_window();
                    window.set_window_title("Bakery Model 3 - Client List");
                })
                .expect("failed to update window");

            Ok::<_, anyhow::Error>(())
        })
        .detach();
    });
}
