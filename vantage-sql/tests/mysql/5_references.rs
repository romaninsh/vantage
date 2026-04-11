//! Test 5: Relationship traversal — with_one, with_many, get_ref_as.

#[allow(unused_imports)]
use vantage_sql::mysql::AnyMysqlType;
use vantage_sql::mysql::MysqlDB;
#[allow(unused_imports)]
use vantage_sql::mysql::MysqlType;
use vantage_sql::mysql_expr;
use vantage_table::table::Table;
use vantage_types::entity;

use vantage_dataset::ReadableDataSet;

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

async fn get_db() -> MysqlDB {
    MysqlDB::connect(MYSQL_URL).await.unwrap()
}

#[entity(MysqlType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Client {
    name: String,
    email: String,
    contact_details: String,
    is_paying_client: bool,
    bakery_id: String,
}

#[entity(MysqlType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ClientOrder {
    bakery_id: String,
    client_id: String,
    is_deleted: bool,
}

fn client_table(db: MysqlDB) -> Table<MysqlDB, Client> {
    Table::new("client", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("email")
        .with_column_of::<String>("contact_details")
        .with_column_of::<bool>("is_paying_client")
        .with_column_of::<String>("bakery_id")
        .with_many("orders", "client_id", order_table)
}

fn order_table(db: MysqlDB) -> Table<MysqlDB, ClientOrder> {
    Table::new("client_order", db)
        .with_id_column("id")
        .with_column_of::<String>("bakery_id")
        .with_column_of::<String>("client_id")
        .with_column_of::<bool>("is_deleted")
        .with_one("client", "client_id", client_table)
}

/// Traverse has_many: paying clients -> their orders
#[tokio::test]
async fn test_has_many_orders_for_paying_clients() {
    let db = get_db().await;
    let mut clients = client_table(db.clone());
    clients.add_condition(mysql_expr!("{} = {}", (clients["is_paying_client"]), true));

    let orders = clients.get_ref_as::<ClientOrder>("orders").unwrap();

    let order_list = orders.list().await.unwrap();
    // Marty has 1 order, Doc has 2 orders -> 3 total
    assert_eq!(order_list.len(), 3);
}

/// Traverse has_many: single client -> orders
#[tokio::test]
async fn test_has_many_orders_for_single_client() {
    let db = get_db().await;
    let mut clients = client_table(db.clone());
    clients.add_condition(mysql_expr!("{} = {}", (clients["name"]), "Doc Brown"));

    let orders = clients.get_ref_as::<ClientOrder>("orders").unwrap();

    let order_list = orders.list().await.unwrap();
    assert_eq!(order_list.len(), 2);
}

/// Traverse has_one: order -> client
#[tokio::test]
async fn test_has_one_client_for_order() {
    let db = get_db().await;
    let mut orders = order_table(db.clone());
    orders.add_condition(mysql_expr!("{} = {}", (orders["id"]), "order1"));

    let client = orders.get_ref_as::<Client>("client").unwrap();

    let client_list = client.list().await.unwrap();
    assert_eq!(client_list.len(), 1);
    assert_eq!(client_list.values().next().unwrap().name, "Marty McFly");
}
