use anyhow::Result;

mod csv {
    use anyhow::Result;
    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use serde_json::json;
    use serde_json::Map;

    use serde_json::Value;
    use vantage::prelude::{Entity, ReadableDataSet};

    pub struct Persistence {
        files: Map<String, Vec<String>>,
    }

    impl Persistence {
        pub fn from_files(files: &[&str]) -> Persistence {
            // read from files, store
            Persistence {}
        }

        pub async fn read_records(&self) -> Vec<Value> {
            json!([
                {"id": 1, "name": "London"},
                {"id": 2, "name": "Paris"}
            ])
            .as_array()
            .unwrap()
            .clone()
        }
    }

    enum Operation {
        Eq,
        In,
    }

    pub struct Condition {
        field: String,
        operation: Operation,
        value: Value,
    }

    impl Condition {
        fn apply_condition(&self, v: Vec<Value>) -> Vec<Value> {
            v.into_iter()
                .filter(|v| {
                    let o = v.as_object().unwrap();
                    let f = o.get(&self.field).unwrap();

                    match self.operation {
                        Operation::Eq => f == v,
                        Operation::In => todo!(),
                    }
                })
                .collect()
        }
    }

    pub struct Table<E: Entity> {
        data_source: Persistence,
        _phantom: std::marker::PhantomData<E>,
        conditions: Vec<Condition>,
    }

    impl<E: Entity> ReadableDataSet<E> for Table<E> {
        async fn get(&self) -> Result<Vec<E>> {
            let mut p = self.data_source.read_records().await;

            for x in &self.conditions {
                p = x.apply_condition(p);
            }

            Ok(p.into_iter()
                .map(|v| serde_json::from_value(v).unwrap())
                .collect())
        }

        async fn get_all_untyped(&self) -> Result<Vec<Map<String, Value>>> {
            todo!()
        }

        async fn get_row_untyped(&self) -> Result<Map<String, Value>> {
            todo!()
        }

        async fn get_col_untyped(&self) -> Result<Vec<Value>> {
            todo!()
        }

        async fn get_one_untyped(&self) -> Result<Value> {
            todo!()
        }

        async fn get_some(&self) -> Result<Option<E>> {
            todo!()
        }

        async fn get_as<T2: DeserializeOwned>(&self) -> Result<Vec<T2>> {
            todo!()
        }

        async fn get_some_as<T2>(&self) -> Result<Option<T2>>
        where
            T2: DeserializeOwned + Default + Serialize,
        {
            todo!()
        }

        fn select_query(&self) -> vantage::prelude::Query {
            todo!()
        }
    }
}

// async fn init_csv_persistence() -> csv::Persistence {}

#[tokio::main]
async fn main() -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_one() {
        let p = csv::Persistence::

    }
}
