//! [`Servo::delete`]: removing the bound record.

use vantage_core::{Result, error};

use super::Servo;
use crate::ops::ChangeFlash;

impl Servo {
    /// Delete the bound record through [`Dio::flash`](crate::Dio::flash),
    /// carrying the baseline as its pre-image, and clear the draft once the
    /// delete settles. A record outside the Dio's set is refused as
    /// `Conflict`.
    pub async fn delete(&self) -> Result<ChangeFlash> {
        let (id, before) = {
            let id = self
                .state
                .id
                .read()
                .unwrap()
                .clone()
                .ok_or_else(|| error!("an unsaved servo has no record to delete"))?;
            (id, self.state.baseline.read().unwrap().clone())
        };
        let mut flash = ChangeFlash::delete(id);
        if let Some(b) = before {
            flash = flash.with_before(b);
        }
        self.dio.flash(flash.clone()).await?;
        {
            *self.state.baseline.write().unwrap() = None;
            self.state.data.write().unwrap().clear();
        }
        self.state.bump_generation();
        Ok(flash)
    }
}
