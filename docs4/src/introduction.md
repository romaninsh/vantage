# Vantage Framework

Vantage is a data entity persistence and abstraction framework for Rust.

Rather than being a traditional ORM, Vantage introduces the concept of a **DataSet** — an abstract,
composable handle to records living in a remote data store. You define structure, conditions,
relations, and operations without loading data eagerly, and Vantage translates your intent into
efficient queries for whichever backend you're using.

This documentation covers Vantage **0.4**.
