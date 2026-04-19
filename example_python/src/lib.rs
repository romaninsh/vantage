//! Python bindings for the bakery_model3 tables over SurrealDB.
//!
//! Exposes one Python class per entity (Bakery, Client, Order, Product), each
//! supporting `count()` and `list_all()`. Tables are wrapped via `AnyTable` so
//! the binding is decoupled from the SurrealDB backend type.

use bakery_model3::{connect_surrealdb, surrealdb, Bakery, Client, Order, Product};
use pyo3::exceptions::{PyConnectionError, PyRuntimeError};
use pyo3::prelude::*;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_table::any::AnyTable;

fn any_client() -> AnyTable {
    AnyTable::from_table(Client::surreal_table(surrealdb()))
}

fn any_bakery() -> AnyTable {
    AnyTable::from_table(Bakery::surreal_table(surrealdb()))
}

fn any_order() -> AnyTable {
    AnyTable::from_table(Order::surreal_table(surrealdb()))
}

fn any_product() -> AnyTable {
    AnyTable::from_table(Product::surreal_table(surrealdb()))
}

fn to_py_err<E: std::fmt::Display>(e: E) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

async fn count_any(table: AnyTable) -> PyResult<i64> {
    use vantage_table::traits::table_like::TableLike;
    table.get_count().await.map_err(to_py_err)
}

async fn list_any(table: AnyTable) -> PyResult<Vec<String>> {
    let records = table.list_values().await.map_err(to_py_err)?;
    Ok(records
        .into_iter()
        .map(|(id, record)| {
            let mut obj = serde_json::Map::new();
            obj.insert("id".to_string(), serde_json::Value::String(id));
            let mut data = serde_json::Map::new();
            for (k, v) in record {
                data.insert(k, v);
            }
            obj.insert("data".to_string(), serde_json::Value::Object(data));
            serde_json::Value::Object(obj).to_string()
        })
        .collect())
}

macro_rules! py_table_class {
    ($Name:ident, $factory:ident) => {
        #[pyclass]
        pub struct $Name;

        #[pymethods]
        impl $Name {
            #[new]
            fn new() -> Self {
                Self
            }

            fn count<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
                pyo3_async_runtimes::tokio::future_into_py(py, async move {
                    count_any($factory()).await
                })
            }

            fn list_all<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
                pyo3_async_runtimes::tokio::future_into_py(
                    py,
                    async move { list_any($factory()).await },
                )
            }
        }
    };
}

py_table_class!(PyClient, any_client);
py_table_class!(PyBakery, any_bakery);
py_table_class!(PyOrder, any_order);
py_table_class!(PyProduct, any_product);

#[pyfunction]
fn init_database<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async {
        connect_surrealdb()
            .await
            .map_err(|e| PyConnectionError::new_err(e.to_string()))?;
        Ok(())
    })
}

#[pymodule]
fn example_python(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyClient>()?;
    m.add_class::<PyBakery>()?;
    m.add_class::<PyOrder>()?;
    m.add_class::<PyProduct>()?;
    m.add_function(wrap_pyfunction!(init_database, m)?)?;
    Ok(())
}
