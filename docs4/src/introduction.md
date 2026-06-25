# Vantage Framework

Vantage is a data entity persistence and abstraction framework for Rust.

Rather than being a traditional ORM, Vantage introduces the concept of a **DataSet** — an abstract,
composable handle to records living in a remote data store. You define structure, conditions,
relations, and operations without loading data eagerly, and Vantage translates your intent into
efficient queries for whichever backend you're using.

This documentation tracks the current **0.6** release line.

## Getting Started

Vantage covers a lot of ground — multiple databases, type systems, entity frameworks, UI adapters —
but none of that matters until you've seen it do something useful.

This guide introduces Vantage concepts one at a time, each building on the last. We'll start with
something you already know — SQL — and work our way up to the bigger abstractions. Along the way
we'll build a small CLI tool that grows with each chapter.

You'll need basic Rust experience (structs, traits, async/await, cargo). No prior Vantage knowledge
required.

**Start here:** [SQLite and the Query Builder](./intro/step1-first-query.md)
