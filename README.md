# Bernard

A complete demonstration shop built on [Topcoat](https://crates.io/crates/topcoat):
catalog, cart, accounts, checkout with real stock accounting, order tracking,
reviews, and a back office. One crate, two hosts:

- **native** (default feature): a single tokio binary over SQLite.
- **edge**: the same pages compiled to wasm, run by a Cloudflare Worker
  over D1. Only the data layer and the image pipeline differ; every page,
  procedure and shard is shared code.

The shop itself is French; the code is English.

## Running the native shop

```sh
cargo build
topcoat asset bundle --bin topcoat-shop
DATABASE_URL=shop.db ./target/debug/topcoat-shop   # HOST/PORT to override 127.0.0.1:3000
```

Migrations run at startup and seed the catalog. The first admin is
promoted by hand: `update users set admin = 1 where email = '...'`.

## Product photography

`assets/photos/` is not in the repository. Drop 1600 px JPEGs named after
lowercase SKUs (`coq-mug.jpg`) and build.rs compiles them in; without them
the shop serves flat placeholder cards and everything else works. Photos
uploaded through the back office are stored in the database and override
the compiled-in ones.

## Running at the edge

```sh
npx wrangler d1 execute DB --local --file migrations/0001_schema.sql   # then 0002, 0003
npx wrangler dev
```

See docs/DEPLOYMENT.md for the production checklist.
