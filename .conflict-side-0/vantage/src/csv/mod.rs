// csv mod implements necessary extensions for Vantage to operate with CSV tables.
//
// Example:
// let csv_persistence = csv::Persistence("./csv_files/");
// let users = csv::Table::init("users.csv", csv_persistence.clone());
//
// A shared persistence allow use of set mapping. Users is representing an
// entire collection of records in "users.csv", however it can be tightened
// up with filter. CSV does not have indexes, so conditions are implemented
// using a regular filter:
//
// let users = users.with_condition(|r|=>r.is_vip == true);
//
// Aggregation is also possible by a CSV persistence. For instance we can now
// refer to "city_id" field like this:
//
// let city_ids = users.field_query("city_id");
//
// A resulting object will refer to a set of city_id's and it can be used
// elsewhere like this:
//
// let city = csv::Table::init("cities.csv", csv_peristence.clone());
// let addresses = addresses.with_condition(addresses.city_id().in(city_ids));
//
// What happens here is - address.city_id() is declared as a CSV table field, that
// implements operation in() and implements into() for csv::Condition. The syntax
// is deliberatly similar to SQL counterpart, giving us syntactical compatibility,
// howevel is implemented using an entirely different set of types
//
// The above operation can also be wrapped into dependency traversal, given that
// both objects share the same persistence, therefore:
//
// users.hasOne("city", ||csv::Table::init(""))
