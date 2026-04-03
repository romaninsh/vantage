use std::marker::PhantomData;

use crate::AnySurrealType;
use crate::identifier::Identifier;
use crate::sum::{Fx, Sum};
use crate::surreal_return::SurrealReturn;
use vantage_expressions::{Expressive, ExpressiveOr, result};

use super::SurrealSelect;

impl SurrealSelect<result::Rows> {
    pub fn as_sum(
        self,
        field_or_expr: impl ExpressiveOr<AnySurrealType, Identifier>,
    ) -> SurrealReturn {
        let query = self.only(field_or_expr);
        SurrealReturn::new(Sum::new(query.expr()).into())
    }
    pub fn as_max(
        self,
        field_or_expr: impl ExpressiveOr<AnySurrealType, Identifier>,
    ) -> SurrealReturn {
        let query = self.only(field_or_expr);
        SurrealReturn::new(Fx::new("math::max", vec![query.expr()]).into())
    }
    pub fn as_min(
        self,
        field_or_expr: impl ExpressiveOr<AnySurrealType, Identifier>,
    ) -> SurrealReturn {
        let query = self.only(field_or_expr);
        SurrealReturn::new(Fx::new("math::min", vec![query.expr()]).into())
    }
    pub fn as_count(self) -> SurrealReturn {
        let result = self.only("id");
        SurrealReturn::new(Fx::new("count", vec![result.expr()]).into())
    }
    pub fn only(
        self,
        field_or_expr: impl ExpressiveOr<AnySurrealType, Identifier>,
    ) -> SurrealSelect<result::List> {
        self.without_fields()
            .with_expression(field_or_expr.field_expr(), None)
            .into_list()
    }
    fn into_list(self) -> SurrealSelect<result::List> {
        if self.from_only {
            panic!("SelectQuery<Rows>::as_list() must not have from_only=true");
        }
        if self.single_value {
            panic!("SelectQuery<Rows>::as_list() must not have single_value=true");
        }
        SurrealSelect {
            fields: self.fields,
            fields_omit: self.fields_omit,
            from: self.from,
            from_omit: self.from_omit,
            from_only: self.from_only,
            where_conditions: self.where_conditions,
            order_by: self.order_by,
            group_by: self.group_by,
            distinct: self.distinct,
            limit: self.limit,
            skip: self.skip,
            _phantom: PhantomData,
            single_value: true,
        }
    }
    pub fn only_first_row(self) -> SurrealSelect<result::SingleRow> {
        if self.from_only {
            panic!("SelectQuery<Rows>::as_one_row() must not have from_only=true");
        }
        if self.single_value {
            panic!("SelectQuery<Rows>::as_one_row() must not have single_value=true");
        }
        SurrealSelect {
            fields: self.fields,
            fields_omit: self.fields_omit,
            from: self.from,
            from_omit: self.from_omit,
            from_only: true,
            where_conditions: self.where_conditions,
            order_by: self.order_by,
            group_by: self.group_by,
            distinct: self.distinct,
            limit: self.limit,
            skip: self.skip,
            _phantom: PhantomData,
            single_value: self.single_value,
        }
    }
}

impl SurrealSelect<result::List> {
    pub fn only_first_row(self) -> SurrealSelect<result::Single> {
        if self.from_only {
            panic!("SelectQuery<List>::only_first_row() must not have from_only=true");
        }
        if !self.single_value {
            panic!("SelectQuery<List>::only_first_row() must have single_value=true");
        }
        SurrealSelect {
            fields: self.fields,
            fields_omit: self.fields_omit,
            from: self.from,
            from_omit: self.from_omit,
            from_only: true,
            where_conditions: self.where_conditions,
            order_by: self.order_by,
            group_by: self.group_by,
            distinct: self.distinct,
            limit: self.limit,
            skip: self.skip,
            _phantom: PhantomData,
            single_value: self.single_value,
        }
    }
}

impl SurrealSelect<result::SingleRow> {
    pub fn only(
        self,
        field_or_expr: impl ExpressiveOr<AnySurrealType, Identifier>,
    ) -> SurrealSelect<result::Single> {
        self.without_fields()
            .with_expression(field_or_expr.field_expr(), None)
            .as_single_value()
    }
    pub fn as_single_value(self) -> SurrealSelect<result::Single> {
        if !self.from_only {
            panic!("SelectQuery<SingleRow>::as_single_value() must have from_only=true");
        }
        if self.single_value {
            panic!("SelectQuery<SingleRow>::as_single_value() must not have single_value=true");
        }
        SurrealSelect {
            fields: self.fields,
            fields_omit: self.fields_omit,
            from: self.from,
            from_omit: self.from_omit,
            from_only: true,
            where_conditions: self.where_conditions,
            order_by: self.order_by,
            group_by: self.group_by,
            distinct: self.distinct,
            limit: self.limit,
            skip: self.skip,
            _phantom: PhantomData,
            single_value: true,
        }
    }
}
