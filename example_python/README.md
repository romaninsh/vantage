# Example Python

Python bindings for Vantage Table framework demonstrating cross-language integration.

## Architecture

This example demonstrates how to expose Vantage table operations to Python while maintaining:

1. **Type Safety in Rust**: Full compile-time guarantees in business logic
2. **Clean Python API**: Pythonic interface with proper async support
3. **Precision Handling**: Rust `Decimal` types mapped to Python `Decimal` for financial data
4. **Zero-Copy Where Possible**: Minimal data conversion overhead

## Key Components

### Rust Side (Pure Business Logic)

```rust
// bakery_model3/src/client.rs - NO Python dependencies
pub trait ClientTable {
    fn get_paying_balance(&self) -> impl Future<Output = Result<Decimal>>;
}

impl ClientTable for Table<SurrealDB, Client> {
    async fn get_paying_balance(&self) -> Result<Decimal> {
        let paying = self.clone().with_condition(self.is_paying_client().eq(true));
        let sum_expr = paying.select().as_sum();
        sum_expr.execute(self.data_source()).await
    }
}
```

### Python Binding Layer

```rust
// example_python/src/lib.rs - Python-specific wrappers
#[pyclass]
pub struct PyClients {
    inner: Table<SurrealDB, Client>,
}

#[pymethods]
impl PyClients {
    pub fn get_paying_balance(&self, py: Python) -> PyResult<PyObject> {
        // Async bridge + Decimal conversion
    }
}
```

## Setup

### Prerequisites

- Rust 1.70+
- Python 3.8+
- SurrealDB (for database operations)

### Build

```bash
# Install maturin (Rust-Python build tool)
pip install maturin

# Build and install in development mode
cd vantage/example_python
maturin develop

# Or build release
maturin build --release
```

### Test

```bash
# Run the integration test
python test_example.py
```

## Example Usage

```python
import asyncio
import example_python
from decimal import Decimal

async def main():
    # 1. Create bakery
    bakery = example_python.PyBakery()

    # 2. Get clients
    clients = bakery.ref_clients()

    # 3. Get paying balance (returns Python Decimal for precision)
    balance_str = await clients.get_paying_balance()
    balance = Decimal(balance_str)

    print(f"Total paying client balance: {balance}")
    # Can handle 9999999999.99 without rounding errors!

if __name__ == "__main__":
    asyncio.run(main())
```

## Design Principles

### 1. **Business Logic Isolation**

The core `bakery_model3` crate has zero Python dependencies. All business logic stays in pure Rust
with full type safety.

### 2. **Wrapper Pattern**

Python bindings are thin wrappers around Rust types:

```rust
#[pyclass]
struct PyClients {
    inner: Table<SurrealDB, Client>,  // Pure Rust type
}
```

### 3. **Async Bridge**

Python's async/await maps cleanly to Rust's async:

```rust
pub fn get_paying_balance(&self, py: Python) -> PyResult<PyObject> {
    pyo3_asyncio::tokio::future_into_py(py, async move {
        self.inner.get_paying_balance().await  // Rust async
    })
}
```

### 4. **Precision Preservation**

Financial data uses `rust_decimal::Decimal` → Python `decimal.Decimal` to avoid floating-point
errors.

## Future Extensions

### Type-Erased Version

```rust
// Future: Use AnyTable for database-agnostic Python API
#[pyclass]
struct PyAnyClients {
    inner: AnyTable,  // Works with any database
}
```

### Multi-Database Support

```python
# Future: Mix databases in same Python session
postgres_clients = vantage.postgres("postgresql://...").table("clients")
surrealdb_orders = vantage.surrealdb("ws://...").table("orders")

# Cross-database operations
total = await postgres_clients.get_paying_balance()
```

## Testing

The test suite verifies:

1. ✅ **Object Creation**: `PyBakery()` succeeds
2. ✅ **Relationship Traversal**: `bakery.ref_clients()` works
3. ✅ **Async Operations**: `await clients.get_paying_balance()` executes
4. ✅ **Precision Handling**: Returns proper `Decimal` type
5. ✅ **Large Numbers**: Handles 9999999999.99 without rounding

## Troubleshooting

### Build Issues

```bash
# Clean and rebuild
rm -rf target/
maturin develop

# Check dependencies
cargo check
```

### Runtime Issues

```bash
# Verify SurrealDB connection
# Check async runtime setup
# Validate Decimal conversion
```

### Import Errors

```python
import sys
sys.path.insert(0, './target/release')  # Adjust path
```
