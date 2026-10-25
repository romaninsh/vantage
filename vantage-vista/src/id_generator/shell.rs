//! The shell behind [`Vista::with_id_generator`](crate::Vista::with_id_generator):
//! the inner shell, its metadata with the generator added.

use crate::{TableShell, VistaMetadata, forward_table_shell};

#[derive(Clone)]
pub(crate) struct IdGeneratorShell {
    inner: Box<dyn TableShell>,
    metadata: VistaMetadata,
}

impl IdGeneratorShell {
    pub(crate) fn new(inner: Box<dyn TableShell>, generator: super::IdGenerator) -> Self {
        let metadata = inner.metadata().clone().with_id_generator(generator);
        Self { inner, metadata }
    }
}

forward_table_shell!(IdGeneratorShell, inner, {
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }
});
