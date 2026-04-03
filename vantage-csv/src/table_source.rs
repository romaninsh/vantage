use async_trait::async_trait;
use indexmap::IndexMap;
use vantage_core::error;
use vantage_dataset::traits::Result;
use vantage_expressions::Expression;
use vantage_expressions::traits::associated_expressions::AssociatedExpression;
use vantage_expressions::traits::datasource::DataSource;
use vantage_expressions::traits::expressive::{DeferredFn, ExpressiveEnum};
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::table::Table;
use vantage_table::traits::table_like::TableLike;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

use crate::Csv;
use crate::condition::apply_condition;
use crate::type_system::AnyCsvType;

impl DataSource for Csv {}

#[async_trait]
impl TableSource for Csv {
    type Column<Type>
        = Column<Type>
    where
        Type: ColumnType;
    type AnyType = AnyCsvType;
    type Value = AnyCsvType;
    type Id = String;

    fn create_column<Type: ColumnType>(&self, name: &str) -> Self::Column<Type> {
        Column::new(name)
    }

    fn to_any_column<Type: ColumnType>(
        &self,
        column: Self::Column<Type>,
    ) -> Self::Column<Self::AnyType> {
        Column::from_column(column)
    }

    fn convert_any_column<Type: ColumnType>(
        &self,
        any_column: Self::Column<Self::AnyType>,
    ) -> Option<Self::Column<Type>> {
        Some(Column::from_column(any_column))
    }

    fn expr(
        &self,
        template: impl Into<String>,
        parameters: Vec<ExpressiveEnum<Self::Value>>,
    ) -> Expression<Self::Value> {
        Expression::new(template, parameters)
    }

    fn search_table_expr(
        &self,
        _table: &impl TableLike,
        search_value: &str,
    ) -> Expression<Self::Value> {
        Expression::new(format!("SEARCH '{}'", search_value), vec![])
    }

    async fn list_table_values<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let mut records = self.read_csv(table.table_name(), table.columns())?;

        for condition in table.conditions() {
            records = apply_condition(records, condition).await?;
        }

        Ok(records)
    }

    async fn get_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let records = self.read_csv(table.table_name(), table.columns())?;
        records
            .get(id)
            .cloned()
            .ok_or_else(|| error!("Record not found", id = id))
    }

    async fn get_table_some_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let records = self.read_csv(table.table_name(), table.columns())?;
        Ok(records.into_iter().next())
    }

    async fn get_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let records = self.read_csv(table.table_name(), table.columns())?;
        Ok(records.len() as i64)
    }

    async fn get_sum<E, Type: ColumnType>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Type>,
    ) -> Result<Type>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("Sum not implemented for CSV backend"))
    }

    async fn get_max<E, Type: ColumnType>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Type>,
    ) -> Result<Type>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("Max not implemented for CSV backend"))
    }

    async fn get_min<E, Type: ColumnType>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Type>,
    ) -> Result<Type>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("Min not implemented for CSV backend"))
    }

    async fn insert_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        _record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("CSV is a read-only data source"))
    }

    async fn replace_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        _record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("CSV is a read-only data source"))
    }

    async fn patch_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        _partial: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("CSV is a read-only data source"))
    }

    async fn delete_table_value<E>(&self, _table: &Table<Self, E>, _id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("CSV is a read-only data source"))
    }

    async fn delete_table_all_values<E>(&self, _table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("CSV is a read-only data source"))
    }

    async fn insert_table_return_id_value<E>(
        &self,
        _table: &Table<Self, E>,
        _record: &Record<Self::Value>,
    ) -> Result<Self::Id>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("CSV is a read-only data source"))
    }

    fn column_table_values_expr<'a, E, Type: ColumnType>(
        &'a self,
        table: &Table<Self, E>,
        column: &Self::Column<Type>,
    ) -> AssociatedExpression<'a, Self, Self::Value, Vec<Type>>
    where
        E: Entity<Self::Value> + 'static,
        Self: Sized,
    {
        use vantage_expressions::{
            expr_any,
            traits::{associated_expressions::AssociatedExpression, datasource::ExprDataSource},
        };

        let table_clone = table.clone();
        let col = column.name().to_string();
        let csv = self.clone();

        let inner = expr_any!("{}", {
            DeferredFn::new(move || {
                let csv = csv.clone();
                let table = table_clone.clone();
                let col = col.clone();
                Box::pin(async move {
                    let records = csv.list_table_values(&table).await?;
                    let values: Vec<AnyCsvType> = records
                        .values()
                        .filter_map(|r| r.get(&col).cloned())
                        .collect();
                    Ok(ExpressiveEnum::Scalar(AnyCsvType::new(values)))
                })
            })
        });

        let expr = expr_any!("{}", { self.defer(inner) });
        AssociatedExpression::new(expr, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::type_system::CsvTypeVariants;
    use vantage_dataset::prelude::{ReadableValueSet, WritableValueSet};
    use vantage_types::EmptyEntity;

    fn test_csv() -> Csv {
        Csv::new(format!("{}/data", env!("CARGO_MANIFEST_DIR")))
    }

    #[tokio::test]
    async fn test_list_bakery() {
        let csv = test_csv();
        let table = Table::<Csv, EmptyEntity>::new("bakery", csv)
            .with_column_of::<String>("name")
            .with_column_of::<i64>("profit_margin");

        let values = table.list_values().await.unwrap();
        assert_eq!(values.len(), 1);
        assert!(values.contains_key("hill_valley"));

        let bakery = &values["hill_valley"];
        let name = bakery["name"].try_get::<String>().unwrap();
        assert_eq!(name, "Hill Valley Bakery");

        let profit = bakery["profit_margin"].try_get::<i64>().unwrap();
        assert_eq!(profit, 15);
    }

    #[tokio::test]
    async fn test_list_clients() {
        let csv = test_csv();
        let table = Table::<Csv, EmptyEntity>::new("client", csv)
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<bool>("is_paying_client")
            .with_column_of::<serde_json::Value>("metadata");

        let values = table.list_values().await.unwrap();
        assert_eq!(values.len(), 3);

        let marty = &values["marty"];
        assert_eq!(marty["name"].try_get::<String>().unwrap(), "Marty McFly");
        assert!(marty["is_paying_client"].try_get::<bool>().unwrap());

        let biff = &values["biff"];
        assert!(!biff["is_paying_client"].try_get::<bool>().unwrap());
        assert_eq!(biff["metadata"].type_variant(), Some(CsvTypeVariants::Json));
    }

    #[tokio::test]
    async fn test_list_products_typed() {
        let csv = test_csv();
        let table = Table::<Csv, EmptyEntity>::new("product", csv)
            .with_column_of::<String>("name")
            .with_column_of::<i64>("calories")
            .with_column_of::<i64>("price")
            .with_column_of::<bool>("is_deleted")
            .with_column_of::<serde_json::Value>("inventory");

        let values = table.list_values().await.unwrap();
        assert_eq!(values.len(), 5);

        let cupcake = &values["flux_cupcake"];
        assert_eq!(
            cupcake["name"].try_get::<String>().unwrap(),
            "Flux Capacitor Cupcake"
        );
        assert_eq!(cupcake["calories"].try_get::<i64>().unwrap(), 300);
        assert_eq!(cupcake["price"].try_get::<i64>().unwrap(), 120);
        assert!(!cupcake["is_deleted"].try_get::<bool>().unwrap());

        let inv = cupcake["inventory"].try_get::<serde_json::Value>().unwrap();
        assert_eq!(inv["stock"], serde_json::json!(50));
    }

    #[tokio::test]
    async fn test_untyped_columns_stay_string() {
        let csv = test_csv();
        let table = Table::<Csv, EmptyEntity>::new("product", csv);

        let values = table.list_values().await.unwrap();
        let cupcake = &values["flux_cupcake"];
        assert_eq!(
            cupcake["calories"].type_variant(),
            Some(CsvTypeVariants::String)
        );
        assert_eq!(cupcake["calories"].try_get::<String>().unwrap(), "300");
    }

    #[tokio::test]
    async fn test_get_value_by_id() {
        let csv = test_csv();
        let table = Table::<Csv, EmptyEntity>::new("client", csv)
            .with_column_of::<String>("name")
            .with_column_of::<String>("email");

        let record = table.get_value(&"doc".to_string()).await.unwrap();
        assert_eq!(record["name"].try_get::<String>().unwrap(), "Doc Brown");
        assert_eq!(
            record["email"].try_get::<String>().unwrap(),
            "doc@brown.com"
        );
    }

    #[tokio::test]
    async fn test_get_value_not_found() {
        let csv = test_csv();
        let table = Table::<Csv, EmptyEntity>::new("client", csv);

        let result = table.get_value(&"nonexistent".to_string()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_some_value() {
        let csv = test_csv();
        let table = Table::<Csv, EmptyEntity>::new("bakery", csv).with_column_of::<String>("name");

        let result = table.get_some_value().await.unwrap();
        assert!(result.is_some());
        let (id, record) = result.unwrap();
        assert_eq!(id, "hill_valley");
        assert_eq!(
            record["name"].try_get::<String>().unwrap(),
            "Hill Valley Bakery"
        );
    }

    #[tokio::test]
    async fn test_get_count() {
        let csv = test_csv();
        let table = Table::<Csv, EmptyEntity>::new("product", csv);

        let count = table.data_source().get_count(&table).await.unwrap();
        assert_eq!(count, 5);
    }

    #[tokio::test]
    async fn test_write_operations_fail() {
        let csv = test_csv();
        let table = Table::<Csv, EmptyEntity>::new("bakery", csv);

        let record = Record::new();
        assert!(
            WritableValueSet::insert_value(&table, &"test".to_string(), &record)
                .await
                .is_err()
        );
        assert!(
            WritableValueSet::delete(&table, &"test".to_string())
                .await
                .is_err()
        );
        assert!(WritableValueSet::delete_all(&table).await.is_err());
    }

    #[tokio::test]
    async fn test_missing_file() {
        let csv = test_csv();
        let table = Table::<Csv, EmptyEntity>::new("nonexistent", csv);

        let result = table.list_values().await;
        assert!(result.is_err());
    }
}
