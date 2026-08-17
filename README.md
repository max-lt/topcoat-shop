# Bernard

A demonstration shop on [Topcoat](https://crates.io/crates/topcoat): catalog,
cart, accounts, checkout with stock accounting, order tracking, reviews and a
back office. One crate, two hosts:

- **native** (default feature): one tokio binary over SQLite.
- **edge**: the same pages as wasm, run by a Cloudflare Worker over D1.

The data layer and the image store are per host; every page, procedure and
shard is shared.

## Running the native shop

```sh
cargo build
topcoat asset bundle --bin topcoat-shop            # --release for a release binary
DATABASE_URL=shop.db ./target/debug/topcoat-shop   # HOST/PORT override 127.0.0.1:3000
```

Migrations run at startup and seed the catalog. The first admin is promoted by
hand: `update users set admin = 1 where email = '...'`.

## Product photography

`PHOTOS_DIR` (default `photos/`) holds one 1600 px JPEG per SKU in lowercase,
`coq-mug.jpg`; the back office writes there too. An empty directory serves flat
placeholder cards and nothing else changes.

The Worker keeps the same tree in the `PHOTOS` R2 bucket.

## Running at the edge

```sh
for f in migrations/*.sql; do npx wrangler d1 execute DB --local --file "$f"; done
for p in photos/*.jpg; do
    npx wrangler r2 object put "bernard-photos/$(basename "$p")" --file "$p" --local
done
cargo run --bin static-bundle   # writes public/
npx wrangler dev
```

`static-bundle` renders the stylesheet, the client runtime and the fonts out of
the shop's own router; Workers Assets serves them.

See docs/DEPLOYMENT.md for the production checklist.
