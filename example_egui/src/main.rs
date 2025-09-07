use dataset_ui_adapters::{egui_adapter::EguiTable, MockProductDataSet, TableStore};
use eframe::egui;

struct TableApp {
    table: EguiTable<MockProductDataSet>,
}

impl Default for TableApp {
    fn default() -> Self {
        // Create mock dataset
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let table = EguiTable::new(store);

        Self { table }
    }
}

impl eframe::App for TableApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Dataset UI Adapters - egui Table Example");
            ui.separator();

            ui.add_space(10.0);

            // Display the table
            self.table.show(ui);
        });
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_title("Dataset UI Adapters - egui Example"),
        ..Default::default()
    };

    eframe::run_native(
        "Dataset UI Adapters - egui Example",
        options,
        Box::new(|_cc| Ok(Box::new(TableApp::default()))),
    )?;

    Ok(())
}
