# Bernard

A demonstration shop built on [Topcoat](https://crates.io/crates/topcoat):
catalog, product pages and live search, served by one tokio binary over
SQLite. The shop itself is French; the code is English.

## Running it

```sh
cargo build
topcoat asset bundle --bin topcoat-shop
DATABASE_URL=shop.db ./target/debug/topcoat-shop   # HOST/PORT to override 127.0.0.1:3000
```

Migrations run at startup and seed the catalog.

## Product photography

`assets/photos/` is not in the repository. Drop 1600 px JPEGs named after
lowercase SKUs (`coq-mug.jpg`) and build.rs compiles them in; without them
the shop still builds and serves placeholder cards.
