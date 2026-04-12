use vantage_core::Result;
use vantage_expressions::traits::datasource::SelectableDataSource;
use vantage_expressions::{Expression, Expressive};

use crate::select::SurrealSelect;
use crate::select::select_field::SelectField;
use crate::surrealdb::SurrealDB;
use crate::types::AnySurrealType;

impl SelectableDataSource<AnySurrealType> for SurrealDB {
    type Select = SurrealSelect;

    fn select(&self) -> Self::Select {
        SurrealSelect::new()
    }

    fn add_select_column(
        &self,
        select: &mut Self::Select,
        expression: Expression<AnySurrealType>,
        alias: Option<&str>,
    ) {
        let mut field = SelectField::new(expression);
        if let Some(a) = alias {
            field = field.with_alias(a.to_string());
        }
        select.fields.push(field);
    }

    async fn execute_select(&self, select: &Self::Select) -> Result<Vec<AnySurrealType>> {
        use vantage_expressions::ExprDataSource;

        let result = self.execute(&select.expr()).await?;

        // Result should be an array of rows
        let arr = result
            .into_value()
            .into_array()
            .map_err(|_| vantage_core::error!("execute_select: expected array result"))?;

        arr.into_iter()
            .map(|item| {
                AnySurrealType::from_cbor(&item)
                    .ok_or_else(|| vantage_core::error!("execute_select: failed to convert row"))
            })
            .collect()
    }
}
