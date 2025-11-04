# Introduction

Welcome to this Book on Vantage. Before we dive in, few assumptions:

- You have read the Introduction (README): <https://github.com/romaninsh/vantage>
- You have cloned this repository, installed postgres and ran `0-intro` example.
- You have interest in software development efficiency, safety and maintainability.
- You have a bit of patience to get to the bottom of various components of Vantage as they are explained.
- You might need some Rust experience, but I'll try to provide pointers and keep things simple.

To get us on the same page, I'll need to go over a basic principles of enterprise software
requirements. Those won't apply to majority of Rust projects, but they are crucial
in commercial software development.

If you are familiar with the concepts (such as from work by Martin Fowler or Eric Evans),
you can skip this section.

## The Purpose of separation of concerns and complexity abstraction

Terms `concern separation` and `complexity abstraction` may be new to you, so I'm going to
look at the enterprise software more generally and also explain why languages such as Java,
C# and TypeScript are considered suitable, while Rust so far had no traction.

Majority of code for the enterprise is designed to "codify" business logic. As company
grows, business rules become more complex. When you open a bank account, you can choose
from few account types, but under the hood banks have hundreds of legacy account types.
A lot of hidden use-cases are needed for regulations or to support legacy systems.

In order to keep the code maintainable, old code must be pruned without affecting stability
of remaining code. Refactoring will always face resistance from product leads, who prefer
rolling out new features.

Companies prefer systems and frameworks which are more modular and easier to maintain.
In many cases, developers who originally wrote the code would no longer be with the company
and a lot of code is written by junior developers too.

### Separation of Concerns

Within your average product team (say - 20 developers) engineers who share the code, engineers
would find preference towards a certain sub-sections of your app. Some developers may prefer
working on the "order processing" while others will dive into "onboarding" flow. Those are
called areas of concern. The practice of separating those areas is called SoC - Separation of Concerns.

Rust generally relies on sub-crates to separate concerns, but there is one area, where
those areas intersect - and that is the "persistence layer".

### Persistence layer

As a business software is dealing with events - it needs to store it's state. Data needs
to persist between execution as well as propogate between containers. Business software
is state-less - when request comes in, the data is loaded, processed and stored back.

Request can come in from an API interface or event queue. Similarly data can be loaded/stored
from Database, APIs or Caches.

It is quite common that in order to process a single request the data would need to be loaded
and stored several times. Quite often external API would rely on intermediate APIs (middleware)
to achieve the task.

### Idempotence

Any request can fail. It can fail before any data is loaded or it can fail after the data
is processed and stored. Idempotence is a design pattern that ensures that any requset can
be retried by the caller. If the original request was successful, retry will also be successful,
but it will not cause any duplicates. This is called Retry safety.

To give you an example, request to send payment of $10 should not send a total of $20 if
it is "retried" due to communication issue inside a middleware.

### Complexity Abstraction

I have just went through some of the essential design principles of enterprise software systems.
If this is new to you, it may sound excessively complex.

Well, there is another trick we use in enterprise system and it is complexity abstraction.
An application can be built in layers, where each layer hides complexity.

In fact - when you were going through the `introduction` code, you have experienced
complexity abstraction first hand. You saw a simple and readable code, that was hiding
complexity through abstraction.

Rust is a complex language. Rust also does not provide any obvious way to implement
complexity abstraction.

Vantage has a ready-to-use way to do this in a consistent way - for instance SoftDelete extension
could provide `retry safety` for your delete operation.

### Safety and Performance

In my opinion, the most important aspect of enterprise software is safety. As humans
we all make mistakes. Quite often in a pursuit of performance, we sacrifice safety.

A seasoned developer may write a chunk of SQL code that quickly and efficiently
performs a complex aggregation. When junior developer comes in, they may introduce
a serious safety bug just by trying to tweak or replicate this complex SQL code.

Vantage offers a way to generate performant SQL code without sacrificing safety.

### Impact of Change

I've seen many projects which were stuck with a "legacy" data structure because
the codebase was riddled with hardcoded SQL queries. As business logic evolves,
it will require changes in the data structure.

Persistence abstraction of Vantage creates a layer for backward-compatibility. Operations
like splitting a table into two tables, moving columns between tables can be
handled without affecting change in business logic.

### Testability

It is crucial that business logic is testable. However - quite often the logic only
works when it has access to the data. The `integration tests` usually provides
test-data to a test-suite. Generally those are slow, they can't track code
coverage and they are hard to debug.

Vantage provides a way to test business logic through `unit tests`. By mocking your
data source, you can focus on the business logic. More importantly you can adopt
`test-driven development` and create standards for your code with test coverage.
Use of faster `unit-tests` also reduces your release cycle time - a metric that
companies are actively looking at.

## Business Appps with Vantage

Vantage addresses many of the challenges of enterprise software development.

Ready to learn how? The answers are coming.
