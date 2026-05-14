pub mod model_cli;
mod table_display;
pub mod vista_cli;

pub use model_cli::{Mode, ModelFactory, Renderer};
pub use table_display::{
    print_table, render_records, render_records_columns, render_records_typed,
};
