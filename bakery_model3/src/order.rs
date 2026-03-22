use vantage_csv::{AnyCsvType, Csv};
use vantage_table::table::Table;
use vantage_types::entity;

#[entity(CsvType)]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Order {
    pub is_deleted: bool,
    pub created_at: String,
    pub lines: String, // JSON stored as string in CSV
}

impl Order {
    pub fn csv_table(csv: Csv) -> Table<Csv, Order> {
        Table::<Csv, Order>::new("order", csv)
            .with_column_of::<bool>("is_deleted")
            .with_column_of::<String>("created_at")
            .with_column_of::<String>("lines")
            .into_entity()
    }
}
