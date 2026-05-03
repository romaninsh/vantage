use ciborium::Value as CborValue;
use vantage_dataset::ValueSet;

use crate::vista::Vista;

impl ValueSet for Vista {
    type Id = String;
    type Value = CborValue;
}
