use std::ops::Deref;

use crate::sql::Expression;

use super::super::sql::chunk::Chunk;

pub trait SqlField: Chunk {
    fn render_column(&self, alias: Option<&str>) -> Expression;
    fn calculated(&self) -> bool;
}

impl Chunk for Box<dyn SqlField> {
    fn render_chunk(&self) -> Expression {
        self.deref().render_column(None)
    }
}
