//! [`Transaction::wrap`] and [`Transaction::resolver`]: how a Vista joins a
//! transaction.

use std::sync::Arc;

use vantage_core::Result;

use super::{Transaction, cant_take_part, joining::joining, shell::TxShell};
use crate::{resolver::VistaResolver, vista::Vista};

impl Transaction {
    /// `vista`, reading and writing through this transaction. Its shell is
    /// rebound now; a shell that can't rebind errors at the wrapped Vista's
    /// first use, not here. Every Vista the result returns (`get_ref`
    /// targets) is wrapped too.
    ///
    /// The participants the rebind joins are the ones its writes are
    /// credited to, for [`committed_before_failure`](Self::committed_before_failure):
    /// a shell joins in `rebind`, not at its first statement.
    pub fn wrap(&self, vista: &Vista) -> Vista {
        let (rebound, joined) = joining(self, || {
            self.check_open().and_then(|()| vista.shell().rebind(self))
        });
        let rebound = rebound.map_err(|err| {
            if err.is_unsupported() {
                cant_take_part(vista.name())
            } else {
                err
            }
        });
        let name = vista.name().to_string();
        vista
            .clone()
            .map_shell(|original| TxShell::new(original, rebound, joined, name, self.clone()))
    }

    /// `base`, with every Vista it resolves, or adopts, wrapped in this
    /// transaction.
    pub fn resolver(&self, base: Arc<dyn VistaResolver>) -> Arc<dyn VistaResolver> {
        Arc::new(TxResolver {
            tx: self.clone(),
            base,
        })
    }
}

struct TxResolver {
    tx: Transaction,
    base: Arc<dyn VistaResolver>,
}

impl VistaResolver for TxResolver {
    fn vista(&self, name: &str) -> Result<Vista> {
        Ok(self.tx.wrap(&self.base.vista(name)?))
    }

    fn adopt(&self, vista: Vista) -> Vista {
        self.tx.wrap(&self.base.adopt(vista))
    }
}
