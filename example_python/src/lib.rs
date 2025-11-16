use pyo3::prelude::*;
use rust_decimal::Decimal;

use bakery_model3::{
    Bakery, Client, Order, Product,
    ClientTable as ClientTableTrait,
    connect_surrealdb, surrealdb
};
use vantage_core::Entity;
use vantage_table::Table;
use vantage_surrealdb::SurrealDB;

// ===== Helper Functions =====

/// Helper to get count of records for any table
fn py_count_helper<E: Entity>(table: &Table<SurrealDB, E>, py: Python) -> PyResult<PyObject> {
    let t = table.clone();
    pyo3_asyncio::tokio::future_into_py(py, async move {
        use vantage_table::traits::table_like::TableLike;
        t.get_count().await
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
    })
}

/// Helper to list all records as JSON for any table
fn py_list_helper<E: Entity>(table: &Table<SurrealDB, E>, py: Python) -> PyResult<PyObject> {
    let t = table.clone();
    pyo3_asyncio::tokio::future_into_py(py, async move {
        use vantage_dataset::dataset::ReadableValueSet;
        let data = t.list_values().await
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

        let json_strings: Vec<String> = data.into_iter()
            .map(|(id, value)| format!(r#"{{"id": "{}", "data": {}}}"#, id, value))
            .collect();
        Ok(json_strings)
    })
}

/// Helper to add conditions (placeholder for now)
fn py_add_condition_helper<E: Entity>(_table: &mut Table<SurrealDB, E>, _condition_expr: String) -> PyResult<()> {
    // TODO: Parse condition_expr and add to table
    Ok(())
}

/// Macro to generate common table method invocations
macro_rules! common_table_methods {
    () => {
        pub fn add_condition(&mut self, condition_expr: String) -> PyResult<()> {
            py_add_condition_helper(&mut self.inner, condition_expr)
        }
        pub fn count(&self, py: Python) -> PyResult<PyObject> {
            py_count_helper(&self.inner, py)
        }
        pub fn list_all(&self, py: Python) -> PyResult<PyObject> {
            py_list_helper(&self.inner, py)
        }
    };
}

// ===== Python Classes =====

/// Python wrapper for Client table
#[pyclass]
pub struct PyClient {
    inner: Table<SurrealDB, Client>,
}

#[pymethods]
impl PyClient {
    #[new]
    pub fn new() -> PyResult<Self> {
        Ok(Self { inner: Client::table(surrealdb()) })
    }

    // Common table methods
    common_table_methods!();

    // Client-specific methods
    pub fn get_paying_balance(&self, py: Python) -> PyResult<PyObject> {
        let table = self.inner.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let balance = table.get_paying_balance().await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            Ok(balance.to_string())
        })
    }

    pub fn ref_bakery(&self) -> PyResult<PyBakery> {
        let bakery_table = self.inner.ref_bakery();
        Ok(PyBakery { inner: bakery_table })
    }

    pub fn ref_orders(&self) -> PyResult<PyOrder> {
        let orders_table = self.inner.ref_orders();
        Ok(PyOrder { inner: orders_table })
    }
}

/// Python wrapper for Bakery table
#[pyclass]
pub struct PyBakery {
    inner: Table<SurrealDB, Bakery>,
}

#[pymethods]
impl PyBakery {
    #[new]
    pub fn new() -> PyResult<Self> {
        Ok(Self { inner: Bakery::table(surrealdb()) })
    }

    // Common table methods
    common_table_methods!();

    // Bakery-specific methods
    pub fn ref_clients(&self) -> PyResult<PyClient> {
        // TODO: Implement bakery -> clients relationship
        // For now, return a new client table (should be filtered by bakery_id)
        PyClient::new()
    }

    pub fn ref_products(&self) -> PyResult<PyProduct> {
        // TODO: Implement bakery -> products relationship
        PyProduct::new()
    }
}

/// Python wrapper for Order table
#[pyclass]
pub struct PyOrder {
    inner: Table<SurrealDB, Order>,
}

#[pymethods]
impl PyOrder {
    #[new]
    pub fn new() -> PyResult<Self> {
        Ok(Self { inner: Order::table(surrealdb()) })
    }

    // Common table methods
    common_table_methods!();

    // Order-specific methods (to be added)
}

/// Python wrapper for Product table
#[pyclass]
pub struct PyProduct {
    inner: Table<SurrealDB, Product>,
}

#[pymethods]
impl PyProduct {
    #[new]
    pub fn new() -> PyResult<Self> {
        Ok(Self { inner: Product::table(surrealdb()) })
    }

    // Common table methods
    common_table_methods!();

    // Product-specific methods (to be added)
}

// ===== Database Connection Functions =====

/// Initialize database connection
#[pyfunction]
pub fn init_database(py: Python) -> PyResult<PyObject> {
    pyo3_asyncio::tokio::future_into_py(py, async {
        connect_surrealdb().await
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyConnectionError, _>(e.to_string()))?;
        Ok(())
    })
}

/// Initialize database with debug logging
#[pyfunction]
pub fn init_database_debug(py: Python) -> PyResult<PyObject> {
    pyo3_asyncio::tokio::future_into_py(py, async {
        bakery_model3::connect_surrealdb_with_debug(true).await
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyConnectionError, _>(e.to_string()))?;
        Ok(())
    })
}

/// Python module definition
#[pymodule]
fn example_python(py: Python, m: &PyModule) -> PyResult<()> {
    // Setup async runtime
    pyo3_asyncio::tokio::init(py);

    // Individual model classes
    m.add_class::<PyClient>()?;
    m.add_class::<PyBakery>()?;
    m.add_class::<PyOrder>()?;
    m.add_class::<PyProduct>()?;

    // Utility functions
    m.add_function(wrap_pyfunction!(init_database, m)?)?;
    m.add_function(wrap_pyfunction!(init_database_debug, m)?)?;

    Ok(())
}
