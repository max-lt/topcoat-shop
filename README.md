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

Photos are content, not code: neither host keeps them in the binary or in
the base.

The native shop reads and writes `PHOTOS_DIR` (default `photos/`), one
1600 px JPEG per SKU in lowercase (`coq-mug.jpg`). The back office writes
into the same directory. With an empty directory the shop serves flat
placeholder cards and everything else works.

The Worker reads the `PHOTOS` R2 bucket, same naming, and writes back to
it on upload.

## Running at the edge

```sh
for f in migrations/*.sql; do npx wrangler d1 execute DB --local --file "$f"; done
for p in photos/*.jpg; do
    npx wrangler r2 object put "bernard-photos/$(basename "$p")" --file "$p" --local
done
cargo run --bin static-bundle   # writes public/: stylesheet, runtime, fonts
npx wrangler dev
```

`static-bundle` renders those files out of the shop's own router, in
process. Workers Assets serves them; the Worker fetches nothing from
anywhere.

See docs/DEPLOYMENT.md for the production checklist.
