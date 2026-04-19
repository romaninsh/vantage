use bakery_model3::{Client, connect_surrealdb, surrealdb};
use dataset_ui_adapters::{TableStore, VantageTableAdapter, gpui_adapter::GpuiTableDelegate};
use gpui::*;
use gpui_component::{
    ActiveTheme, ContextModal as _, Root, StyledExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    form::{form_field, v_form},
    input::{InputState, TextInput},
    modal::Modal,
    table::Table,
    v_flex,
};
use vantage_table::any::AnyTable;

actions!(example_gpui, [Quit, AddClient]);

struct TableApp {
    table: Entity<Table<GpuiTableDelegate<VantageTableAdapter>>>,
    name_input: Entity<InputState>,
    email_input: Entity<InputState>,
    contact_input: Entity<InputState>,
    bakery_input: Entity<InputState>,
    is_paying_client: bool,
    show_dialog: bool,
}

impl TableApp {
    fn new(
        table: Entity<Table<GpuiTableDelegate<VantageTableAdapter>>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name_input = cx.new(|cx| InputState::new(window, cx));
        let email_input = cx.new(|cx| InputState::new(window, cx));
        let contact_input = cx.new(|cx| InputState::new(window, cx));
        let bakery_input = cx.new(|cx| InputState::new(window, cx));

        Self {
            table,
            name_input,
            email_input,
            contact_input,
            bakery_input,
            is_paying_client: false,
            show_dialog: false,
        }
    }
}

impl Render for TableApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut layout =
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
                        .child("Real SurrealDB data"),
                )
                .child(
                    div()
                        .mb_4()
                        .child(Button::new("add_client").label("Add Client").on_click(
                            cx.listener(|this, _, _, cx| {
                                println!("Button clicked!");
                                this.show_add_client_modal(cx);
                            }),
                        )),
                )
                .child(self.table.clone());

        layout
    }
}

impl TableApp {
    fn show_add_client_modal(&mut self, _cx: &mut Context<Self>) {
        println!("Modal function called - this will be implemented with a simple overlay");

        let new_client = Client {
            name: "Test Client".to_string(),
            email: "test@example.com".to_string(),
            contact_details: "555-1234".to_string(),
            is_paying_client: true,
            bakery_id: Some("test_bakery".to_string()),
        };

        dbg!(&new_client);
    }
}

fn main() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async {
        connect_surrealdb()
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

            let client_table = AnyTable::from_table(Client::surreal_table(surrealdb()));
            let dataset = VantageTableAdapter::new(client_table).await;
            let store = TableStore::new(dataset);
            let delegate = GpuiTableDelegate::new(store);

            let window = cx
                .open_window(options, |window, cx| {
                    let table =
                        cx.new(|cx| Table::new(delegate, window, cx).stripe(true).border(true));
                    let view = cx.new(|cx| TableApp::new(table, window, cx));
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
