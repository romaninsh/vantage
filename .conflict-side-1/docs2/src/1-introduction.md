# Introduction

A modern enterprise stores data in multiple locations - SQL, NoSQL and Graph databases
if you are lucky. If you are not - throw dozen of Excel documents and some legacy Oracle
database into a mix.

When it comes to data manipulation, there are more. Queues and Events is a typical
way how large companies handle change in the data. It's hard to find an architect who
would fully understand where data is and how it is managed.

## Data Mesh

A principle of having a way to access any data in the company in the way how it is
indended to be accessed is one of a legents. But this is exactly the challenge that
Vantage framework solves.

For a moment - lets assume you have been given permission to access the data in the
way intended. To read data from one source, use a different mechanic to modify data
and also augment data with additional sources - how can you make this useful?

Modern architecture approaches this through "middeware". Sadly - each middleware
is yet another way to interact with your data. If not maintained correctly, your
organisation ends up with even a bigger mess.

Data Mesh solves this by providing an interface to all of your companys data (I
will real-time databases, not data analytics).

## What problem does Vantage framework address?

With Vantage it's possible to build an interface for all the real-time data in your
company. Because Vantage is built in rust - you can use this interface from any
device and any programming language (if you have been granted permission of course).

And since Rust is very safe language with advanced type system - your entire software
ecosystem will have safe and type-save way of access it.

You might already have dozens of questions. Answers are coming. This book will
be able to address abstraction, cross-database queries, use of stored procedures
and schema mapping, however I will need to start with the basics.

You see - Vantage brings methodology to a system design in order to truthly
revolutioise the way how data is accessed and if you follow me closely, you will
understand the way how to once and for all solve build a distributed data mesh
for your enterprise without renaming any column or migrating your data unnecessarily.
