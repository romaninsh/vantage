//! The shell behind [`Vista::with_id_generator`](crate::Vista::with_id_generator):
//! the inner shell, its metadata with the generator added.

use vantage_core::Result;

use crate::{TableShell, Transaction, VistaMetadata, forward_table_shell};

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

    fn rebind(&self, tx: &Transaction) -> Result<Box<dyn TableShell>> {
        Ok(Box::new(Self {
            inner: self.inner.rebind(tx)?,
            metadata: self.metadata.clone(),
        }))
    }
});
