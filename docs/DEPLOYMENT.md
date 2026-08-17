# Deploying the edge build

The Worker serves every page from D1 and proxies photography and static
assets from a running native shop (`ORIGIN` in src/images/edge.rs). Cut
that dependency by moving the originals to R2 and changing the constant.

## First deployment

```sh
npx wrangler login
npx wrangler d1 create bernard        # copy database_id into wrangler.toml
npx wrangler d1 execute DB --remote --file migrations/0001_schema.sql
npx wrangler d1 execute DB --remote --file migrations/0002_catalog.sql
npx wrangler d1 execute DB --remote --file migrations/0003_reviews.sql
npx wrangler deploy
```

Bindings, declared in wrangler.toml: `DB` (D1) and `IMAGES` (image
transforms; without it `/img` falls back to plain proxying).

## What differs from the native host

- No open transactions on D1: checkout takes stock with guarded
  decrements and compensates if it loses the race. Stock never goes
  negative; a lost race costs a few statements, not an oversold order.
- Photo upload happens on the native back office; the edge admin links
  there. Uploaded photos live in the database either way.
- Asset links in the page head are pinned to the native bundle's hashed
  names; rebundle the native shop, then refresh them in `head_assets`
  (src/app.rs).

## Operational notes

- Procedure ids are minted per build: browser tabs opened before a deploy
  answer 404 on their buttons until reloaded.
- When testing interactively, use a foreground browser tab or curl:
  background tabs throttle timers and fetch delivery, which looks exactly
  like a hung worker and is not one.
