use crate::{traits::datasource::DataSource, Renderable};
use rusqlite::Connection;

impl DataSource for Connection {
    fn query_fetch(
        &self,
        query: &crate::Query,
    ) -> Result<Vec<Vec<String>>, Box<dyn std::error::Error>> {
        let query = query.render();
        let mut stmt = self.prepare(&query).unwrap();
        let cnt = stmt.column_count();

        let row_iter = stmt
            .query_map([], |row| {
                let mut result_row = Vec::new();
                // Loop through each column in the row.
                for i in 0..cnt {
                    // Use get_raw to access each column as a generic ValueRef.
                    let value: String = row.get(i).unwrap();
                    result_row.push(value);
                }
                Ok(result_row)
            })
            .unwrap();

        Ok(row_iter.map(|r| r.unwrap()).collect())
    }

    fn query_exec(&self, query: &crate::Query) -> Result<(), Box<dyn std::error::Error>> {
        let query = query.render();
        self.execute(&query, ()).map(|_| ()).map_err(|e| e.into())
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::table::Table;
    use crate::Expression;
    use crate::{expr, Query};

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();

        conn.execute(
            "CREATE TABLE person (
                id   INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                surname TEXT NOT NULL
            )",
            (), // empty list of parameters.
        )
        .unwrap();
        conn.execute(
            "INSERT INTO person (name, surname) VALUES (?1, ?2)",
            ("Steven", "Test"),
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_rusqlite() {
        let conn = setup();

        let query = Query::new("person")
            .add_column_field("name")
            .add_column_field("surname");
        let result = conn.query_fetch(&query).unwrap();
        assert_eq!(result, vec![vec!["Steven", "Test"]]);
    }

    #[test]
    fn test_expressions() {
        let conn = setup();

        let query = Query::new("person").add_column_expr(expr!("name || ' ' || surname"));
        let result = conn.query_fetch(&query).unwrap();
        assert_eq!(result, vec![vec!["Steven Test"]]);
    }

    #[test]
    fn test_table() {
        let conn = setup();

        let table = Table::new("person", Box::new(conn))
            .add_field("name")
            .add_field("surname");

        let q = table.get_select_query();
        assert_eq!(q.render(), "SELECT name, surname FROM person");

        let data = table.get_all_data().unwrap();
        let row = data.get(0).unwrap().clone();
        assert_eq!(row, vec!["Steven", "Test"]);
    }

    #[test]
    fn test_table_into_iter() {
        let conn = setup();

        let table = Table::new("person", Box::new(conn))
            .add_field("name")
            .add_field("surname");

        let data: Vec<Vec<String>> = table.iter().collect();
        let row = data.get(0).unwrap().clone();
        assert_eq!(row, vec!["Steven", "Test"]);
    }
}
