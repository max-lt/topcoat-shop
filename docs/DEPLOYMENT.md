# Deploying the edge build

The Worker answers from its own bindings: pages and data from D1, photos
from the R2 bucket, stylesheet and fonts from Workers Assets. It makes no
request to any other host.

## First deployment

```sh
npx wrangler login
npx wrangler d1 create bernard-en           # copy database_id into wrangler.toml
for f in migrations/*.sql; do npx wrangler d1 execute DB --remote --file "$f"; done
npx wrangler r2 bucket create bernard-photos
for p in photos/*.jpg; do
    npx wrangler r2 object put "bernard-photos/$(basename "$p")" --file "$p" --remote
done

cargo build                                 # tailwind, then the asset bundle
topcoat asset bundle --bin topcoat-shop
cargo run --bin static-bundle               # writes public/
npx wrangler deploy
```

Bindings, declared in wrangler.toml: `DB` (D1), `PHOTOS` (R2), `IMAGES`
(transforms; without it `/img` serves the originals) and the `public/`
asset directory.

Redeploy after changing the stylesheet, the fonts or the client runtime:
`public/` is generated, not committed, and `wrangler deploy` uploads
whatever is there.

## What differs from the native host

- No open transactions on D1: checkout takes stock with guarded
  decrements and compensates if it loses the race. Stock never goes
  negative; a lost race costs a few statements, not an oversold order.
- An upload is bounded by the IMAGES binding rather than by a decoder in
  process, and lands in R2 rather than in a directory.
- No dominant colours behind the photos: computing one needs a decoder.

## Operational notes

- Procedure ids are minted per build: browser tabs opened before a deploy
  answer 404 on their buttons until reloaded.
- When testing interactively, use a foreground browser tab or curl:
  background tabs throttle timers and fetch delivery, which looks exactly
  like a hung worker and is not one.
