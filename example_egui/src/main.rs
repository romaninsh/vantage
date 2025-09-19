use bakery_model3::*;
use dataset_ui_adapters::{egui_adapter::EguiTable, TableStore, VantageTableAdapter};
use eframe::egui;

struct TableApp {
    table: EguiTable<VantageTableAdapter<Client>>,
}

impl TableApp {
    async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        bakery_model3::connect_surrealdb().await?;
        let client_table = Client::table();
        let dataset = VantageTableAdapter::new(client_table).await;
        let store = TableStore::new(dataset);
        let table = EguiTable::new(store).await;
        Ok(Self { table })
    }
}

impl eframe::App for TableApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Bakery Model 3 - egui Client List");
            ui.separator();

            ui.add_space(10.0);

            self.table.show(ui);
        });
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_title("Bakery Model 3 - egui Client List"),
        ..Default::default()
    };

    let app = TableApp::new().await?;

    eframe::run_native(
        "Bakery Model 3 - egui Client List",
        options,
        Box::new(move |_cc| Ok(Box::new(app))),
    )?;

    Ok(())
}
