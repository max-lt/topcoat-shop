-- The shop's schema. Plain SQLite dialect, which is also Turso's and D1's,
-- so moving the data to either is a driver change and not a rewrite.
-- Money is stored in cents as an integer: no float ever touches a price.

create table users (
    id            integer primary key autoincrement,
    email         text    not null unique,
    name          text    not null,
    password_hash text    not null,
    admin         integer not null default 0,
    created_at    text    not null
);

create table sessions (
    -- The raw token never reaches the base; it lives client-side only.
    token_hash blob    primary key,
    user_id    integer not null references users(id) on delete cascade,
    expires_at text    not null
);

create index sessions_user on sessions(user_id);

create table products (
    sku         text    primary key,
    name        text    not null,
    summary     text    not null,
    detail      text    not null,
    price_cents integer not null check (price_cents >= 0),
    stock       integer not null check (stock >= 0), -- sum of the variants
    category    text    not null,
    is_new      integer not null default 0,
    material    text    not null default '',
    hidden      integer not null default 0
);

-- Every product has at least one variant row; a product sold in one size
-- keeps a row with size = '', because the cart reads its stock from here.
create table variants (
    sku   text    not null references products(sku) on delete cascade,
    size  text    not null,
    stock integer not null check (stock >= 0),
    rank  integer not null default 0,
    primary key (sku, size)
);

-- A cart survives logging out and follows an anonymous visitor by its own
-- cookie; user_id is filled in when that visitor signs in.
create table carts (
    id         text    primary key,
    user_id    integer references users(id) on delete cascade,
    created_at text    not null
);

create table cart_lines (
    cart_id  text    not null references carts(id) on delete cascade,
    sku      text    not null references products(sku),
    size     text    not null default '',
    quantity integer not null check (quantity > 0),
    primary key (cart_id, sku, size)
);

create table orders (
    id             integer primary key autoincrement,
    reference      text    not null unique,
    user_id        integer not null references users(id),
    total_cents    integer not null,
    shipping_cents integer not null default 0,
    shipping       text    not null default 'standard',
    status         text    not null, -- paid | packing | shipped | delivered | cancelled
    address        text    not null,
    created_at     text    not null
);

create index orders_user on orders(user_id, created_at desc);

-- Order lines copy the price they were bought at: a later price change must
-- not rewrite history.
create table order_lines (
    order_id    integer not null references orders(id) on delete cascade,
    sku         text    not null,
    name        text    not null,
    size        text    not null default '',
    price_cents integer not null,
    quantity    integer not null,
    primary key (order_id, sku, size)
);

create table tracking (
    id       integer primary key autoincrement,
    order_id integer not null references orders(id) on delete cascade,
    step     text    not null,
    note     text    not null,
    at       text    not null
);

create index tracking_order on tracking(order_id, at);

-- The newsletter list. The email is the key: subscribing twice is a no-op,
-- not an error.
create table subscribers (
    email      text primary key,
    created_at text not null
);

create table reviews (
    id         integer primary key autoincrement,
    sku        text    not null references products(sku) on delete cascade,
    author     text    not null,
    rating     integer not null check (rating between 1 and 5),
    text       text    not null,
    created_at text    not null
);

create index reviews_sku on reviews(sku, created_at desc);

-- Asking twice for the same size and address is a no-op, hence the key.
create table stock_alerts (
    sku        text not null references products(sku) on delete cascade,
    size       text not null default '',
    email      text not null,
    created_at text not null,
    primary key (sku, size, email)
);

create table addresses (
    id         integer primary key autoincrement,
    user_id    integer not null references users(id) on delete cascade,
    label      text    not null,
    text       text    not null,
    is_default integer not null default 0
);

create index addresses_user on addresses(user_id);

-- Photos uploaded from the back office. The catalog's own photography is
-- compiled into the binary; a row here overrides it for that sku.
create table photos (
    sku        text primary key references products(sku) on delete cascade,
    bytes      blob not null,
    created_at text not null
);
