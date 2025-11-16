use ciborium::value::Value as CborValue;
use serde::{Deserialize, Serialize};

/// Custom serialization helper similar to serde_json::to_value (panics on error)
pub fn to_cbor_value<T: Serialize>(value: &T) -> CborValue {
    to_cbor_value_result(value).unwrap()
}

/// Custom serialization helper similar to serde_json (returns Result)
pub fn to_cbor_value_result<T: Serialize>(
    value: &T,
) -> Result<CborValue, ciborium::ser::Error<std::io::Error>> {
    let mut buffer = Vec::new();
    ciborium::ser::into_writer(value, &mut buffer)?;
    let cbor_value: CborValue = ciborium::de::from_reader(&buffer[..]).map_err(|_| {
        ciborium::ser::Error::Io(std::io::Error::from(std::io::ErrorKind::InvalidData))
    })?;
    Ok(cbor_value)
}

/// Custom deserialization helper similar to serde_json::from_value (panics on error)
pub fn from_cbor_value<T: for<'de> Deserialize<'de>>(value: CborValue) -> T {
    from_cbor_value_result(value).unwrap()
}

/// Custom deserialization helper similar to serde_json (returns Result)
pub fn from_cbor_value_result<T: for<'de> Deserialize<'de>>(
    value: CborValue,
) -> Result<T, ciborium::de::Error<std::io::Error>> {
    let mut buffer = Vec::new();
    ciborium::ser::into_writer(&value, &mut buffer).map_err(|_| {
        ciborium::de::Error::Io(std::io::Error::from(std::io::ErrorKind::InvalidData))
    })?;
    ciborium::de::from_reader(&buffer[..])
}
