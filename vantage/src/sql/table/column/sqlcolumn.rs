use crate::sql::Chunk;

pub trait SqlColumn: Chunk {
    fn name(&self) -> String;
    fn name_with_table(&self) -> String;
    fn set_name(&mut self, name: String);
    fn set_table_alias(&mut self, alias: String);
    fn set_column_alias(&mut self, alias: String);
    fn get_column_alias(&self) -> Option<String>;
}
