// surreal::fx binds number and string arguments, like surreal::expr.
surreal::select().expression(surreal::fx("array::range", [1, 5]).alias("r")).expression(surreal::fx("string::uppercase", ["x"]).alias("u")).from("order")
