// The primitives page's shared-vocabulary rows no other fixture uses:
// count_distinct, nullif, cast, date_format, and the surreal-only median.
surreal::select()
    .from("employee")
    .expression(surreal::count_distinct(surreal::ident("dept")).alias("depts"))
    .expression(surreal::nullif(surreal::ident("qty"), 0).alias("qty_or_none"))
    .expression(surreal::cast(surreal::ident("qty"), "float").alias("qty_f"))
    .expression(surreal::date_format(surreal::ident("created_at"), "%Y-%m").alias("period"))
    .expression(surreal::median(surreal::ident("salary")).alias("mid"))
