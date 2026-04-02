/// Create a type-union enum with automatic From implementations
///
/// # Basic Usage
/// ```rust
/// fn_args!(MyArgs: S(String), I(i32));
///
/// fn process(arg: impl Into<MyArgs>) {
///     match arg.into() {
///         MyArgs::S(s) => println!("String: {}", s),
///         MyArgs::I(i) => println!("Int: {}", i),
///     }
/// }
/// ```
///
/// # Automatic Blanket Implementations
///
/// The macro automatically generates special blanket implementations:
///
/// - **T(AnySurrealType)**: Adds `impl<T: SurrealType> From<T> for EnumName`
///   allowing any SurrealType to be converted to the T variant
///
/// - **E(Expression<AnySurrealType>)**: Adds `impl<T: Expressive<AnySurrealType>> From<T> for EnumName`
///   allowing any expressive type (Identifier, Thing, etc.) to be converted to the E variant
///
/// # Predefined Enums
///
/// This crate provides several predefined enums:
///
/// - `AllSurrealTypes`: Only accepts SurrealTypes → T(AnySurrealType)
/// - `AllSurrealTypesOrExpr`: Accepts SurrealTypes or Expressions → T(AnySurrealType), E(Expression<AnySurrealType>)
/// - `ExprArguments`: Full flexibility → T(AnySurrealType), E(Expression<AnySurrealType>), D(DeferredFn<AnySurrealType>)
#[macro_export]
macro_rules! fn_args {
    // Main entry point - all patterns delegate to this
    ($enum_name:ident $(<$($generic:ident),+ $(,)?>)?: $($variant:ident($type:ty)),+ $(,)?) => {
        // Generate the enum
        $crate::fn_args_generate_enum!($enum_name $(<$($generic),+>)?: $($variant($type)),+);

        // Generate basic From implementations
        $crate::fn_args_generate_from!($enum_name $(<$($generic),+>)?: $($variant($type)),+);

        // Generate conditional blanket implementations
        $crate::fn_args_conditional_blanket_any!($enum_name: $($variant($type)),+);
        $crate::fn_args_conditional_blanket_expressive!($enum_name: $($variant($type)),+);
    };
}

/// used by `fn_args!` to generate the enum definition
#[macro_export]
macro_rules! fn_args_generate_enum {
    ($enum_name:ident: $($variant:ident($type:ty)),+) => {
        pub enum $enum_name {
            $($variant($type)),+
        }
    };
    ($enum_name:ident <$($generic:ident),+>: $($variant:ident($type:ty)),+) => {
        pub enum $enum_name<$($generic),+> {
            $($variant($type)),+
        }
    };
}

/// used by `fn_args!` to generate From implementations for each variant
#[macro_export]
macro_rules! fn_args_generate_from {
    ($enum_name:ident: $($variant:ident($type:ty)),+) => {
        $(
            impl From<$type> for $enum_name {
                fn from(val: $type) -> Self {
                    Self::$variant(val)
                }
            }
        )+
    };
    ($enum_name:ident <$($generic:ident),+>: $($variant:ident($type:ty)),+) => {
        $(
            impl<$($generic),+> From<$type> for $enum_name<$($generic),+> {
                fn from(val: $type) -> Self {
                    Self::$variant(val)
                }
            }
        )+
    };
}

/// Used by `fn_args!` to generate conditional SurrealType blanket implementation
#[macro_export]
macro_rules! fn_args_conditional_blanket_any {
    ($enum_name:ident: T(AnySurrealType) $(, $($rest:tt)*)?) => {
        // Add SurrealType blanket implementation
        impl<ST> From<ST> for $enum_name
        where
            ST: crate::SurrealType,
        {
            fn from(val: ST) -> Self {
                $enum_name::T(AnySurrealType::new(val))
            }
        }
    };
    ($enum_name:ident: $variant:ident($type:ty) $(, $($rest:tt)*)?) => {
        // Continue checking remaining variants
        $crate::fn_args_conditional_blanket_any!($enum_name: $($($rest)*)?);
    };
    ($enum_name:ident:) => {
        // No T(AnySurrealType) variant found
    };
}

/// Used by `fn_args!` to generate conditional Expressive blanket implementation
#[macro_export]
macro_rules! fn_args_conditional_blanket_expressive {
    ($enum_name:ident: E(Expression<AnySurrealType>) $(, $($rest:tt)*)?) => {
        // Add Expressive blanket implementation
        impl<EX> From<EX> for $enum_name
        where
            EX: vantage_expressions::Expressive<AnySurrealType>,
        {
            fn from(val: EX) -> Self {
                $enum_name::E(val.expr())
            }
        }
    };
    ($enum_name:ident: $variant:ident($type:ty) $(, $($rest:tt)*)?) => {
        // Continue checking remaining variants
        $crate::fn_args_conditional_blanket_expressive!($enum_name: $($($rest)*)?);
    };
    ($enum_name:ident:) => {
        // No E(Expression<AnySurrealType>) variant found
    };
}

/// Create a SurrealDB expression with automatic type conversion
///
/// Usage:
/// - `surreal_expr!("template")` - no parameters
/// - `surreal_expr!("template", arg1, arg2)` - converts args via Into<ExprArguments>
/// - Supports any type that implements SurrealType, expressions, expressive, and deferred functions
#[macro_export]
macro_rules! surreal_expr {
    // Simple template without parameters
    ($template:expr) => {
        vantage_expressions::Expression::<$crate::AnySurrealType>::new($template, vec![])
    };

    // Template with parameters
    ($template:expr, $($param:expr),*) => {
        vantage_expressions::Expression::<$crate::AnySurrealType>::new(
            $template,
            vec![
                $(
                    $crate::surreal_param!($param)
                ),*
            ]
        )
    };
}

/// Helper function to convert ExprArguments to ExpressiveEnum
pub fn expr_arg_to_expressive_enum(
    arg: crate::SurrealExprArgs,
) -> vantage_expressions::ExpressiveEnum<crate::AnySurrealType> {
    match arg {
        crate::SurrealExprArgs::T(val) => vantage_expressions::ExpressiveEnum::Scalar(val),
        crate::SurrealExprArgs::E(expr) => {
            use vantage_expressions::Expressive;
            vantage_expressions::ExpressiveEnum::Nested(expr.expr())
        }
        crate::SurrealExprArgs::D(deferred) => {
            vantage_expressions::ExpressiveEnum::Deferred(deferred)
        }
    }
}

/// Helper macro to handle parameter conversion via ExprArguments
#[macro_export]
macro_rules! surreal_param {
    // Convert parameter to ExprArguments and then to ExpressiveEnum
    ($param:expr) => {
        $crate::macros::expr_arg_to_expressive_enum(($param).into())
    };
}
