use ciborium::Value as CborValue;
use vantage_dataset::traits::ValueSet;

use crate::live_table::LiveTable;

impl ValueSet for LiveTable {
    type Id = String;
    type Value = CborValue;
}
