use vantage_csv::{AnyCsvType, Csv};
use vantage_table::table::Table;
use vantage_types::entity;

#[entity(CsvType)]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Bakery {
    pub name: String,
    pub profit_margin: i64,
}

impl Bakery {
    pub fn csv_table(csv: Csv) -> Table<Csv, Bakery> {
        Table::<Csv, Bakery>::new("bakery", csv)
            .with_column_of::<String>("name")
            .with_column_of::<i64>("profit_margin")
            .into_entity()
    }
}

// SurrealDB support commented out during CSV-first development
// impl Bakery {
//     pub fn table(db: SurrealDB) -> Table<SurrealDB, Bakery> {
//         Table::new("bakery", db)
//             .with_column_of::<String>("name")
//             .with_column_of::<i64>("profit_margin")
//             .into_entity()
//     }
// }
