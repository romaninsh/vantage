use crate::{prelude::TableAlias, sql::Chunk};

pub trait SqlColumn: Chunk {
    fn name(&self) -> String;
    fn name_with_table(&self) -> String;
    fn get_table_alias(&self) -> &Option<TableAlias>;
    fn set_name(&mut self, name: String);
    fn set_table_alias(&mut self, alias: &TableAlias);
    fn set_alias(&mut self, alias: String);
    fn get_alias(&self) -> Option<String>;
}
