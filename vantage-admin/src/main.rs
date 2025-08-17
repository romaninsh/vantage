use gpui::*;

mod app;
mod assets;
mod page;
mod vantage;

use assets::Assets;
use gpui_component::Root;
use page::*;

use crate::app::AdminApp;

actions!(rust_admin, [Quit]);

fn main() {
    let app = Application::new().with_assets(Assets);

    app.run(move |cx| {
        gpui_component::init(cx);

        // Set up Mac menus
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);

        cx.on_action(|_: &Quit, cx: &mut App| {
            cx.quit();
        });

        use gpui_component::input::{Copy, Cut, Paste, Redo, Undo};
        cx.set_menus(vec![
            Menu {
                name: "Golf Admin System".into(),
                items: vec![MenuItem::action("Quit", Quit)],
            },
            Menu {
                name: "Edit".into(),
                items: vec![
                    MenuItem::os_action("Undo", Undo, gpui::OsAction::Undo),
                    MenuItem::os_action("Redo", Redo, gpui::OsAction::Redo),
                    MenuItem::separator(),
                    MenuItem::os_action("Cut", Cut, gpui::OsAction::Cut),
                    MenuItem::os_action("Copy", Copy, gpui::OsAction::Copy),
                    MenuItem::os_action("Paste", Paste, gpui::OsAction::Paste),
                ],
            },
            Menu {
                name: "Window".into(),
                items: vec![],
            },
        ]);

        cx.activate(true);

        let window_size = size(px(1200.), px(800.));
        let window_bounds = Bounds::centered(None, window_size, cx);

        cx.spawn(async move |cx| {
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(window_bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Golf Admin System".into()),
                    appears_transparent: false,
                    traffic_light_position: None,
                }),
                window_min_size: Some(size(px(800.), px(600.))),
                kind: WindowKind::Normal,
                ..Default::default()
            };

            cx.open_window(options, |window, cx| {
                let admin_app = AdminApp::view(window, cx);

                // Handle batch details action
                cx.on_action({
                    let admin_app = admin_app.clone();
                    move |action: &ShowBatchDetails, cx| {
                        admin_app.update(cx, |admin_app, cx| {
                            admin_app.pending_detail_request = Some(action.0.clone());
                            cx.notify();
                        });
                    }
                });

                cx.new(|cx| Root::new(admin_app.into(), window, cx))
            })
            .expect("failed to open window");

            Ok::<_, anyhow::Error>(())
        })
        .detach();
    });
}
