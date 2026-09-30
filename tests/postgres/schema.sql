-- 実機検証用のスキーマと初期データ。
-- tests/postgres_equivalence.rs がトランザクションの中で流し、最後に ROLLBACK する。
-- 既存のフィクスチャ（tests/fixtures/*/*.sql）が参照する表と列をそろえている。

CREATE TABLE customers (
    id bigint PRIMARY KEY,
    "Name" text NOT NULL,
    tags text[] NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT '2026-01-01 00:00:00+00',
    score numeric NOT NULL DEFAULT 0,
    active boolean NOT NULL DEFAULT true
);

CREATE TABLE orders (
    id bigint PRIMARY KEY,
    customer_id bigint NOT NULL REFERENCES customers (id),
    ordered_at timestamptz NOT NULL,
    amount numeric NOT NULL,
    status text NOT NULL,
    carrier_id bigint,
    shipped_at timestamptz,
    carrier text
);

CREATE TABLE blacklist (customer_id bigint PRIMARY KEY);

CREATE TABLE accounts (
    id bigint PRIMARY KEY,
    code text,
    payload jsonb NOT NULL DEFAULT '{}',
    tags text[] NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT '2026-01-01 00:00:00+00',
    price numeric NOT NULL DEFAULT 0,
    name text NOT NULL DEFAULT '',
    score int NOT NULL DEFAULT 0,
    deleted_at timestamptz,
    ids int[] NOT NULL DEFAULT '{}',
    balance numeric NOT NULL DEFAULT 0
);

CREATE TABLE logs (id bigint PRIMARY KEY, account_id bigint NOT NULL, message text);

CREATE TABLE users (
    id bigint PRIMARY KEY,
    name text NOT NULL,
    active boolean NOT NULL DEFAULT true,
    deleted_at timestamptz,
    points int NOT NULL DEFAULT 0
);

CREATE TABLE sessions (user_id bigint NOT NULL);
CREATE TABLE carriers (id bigint PRIMARY KEY, name text NOT NULL);
CREATE TABLE staging_stock (sku text NOT NULL, qty int NOT NULL, batch_id int NOT NULL);
CREATE TABLE stock (
    sku text PRIMARY KEY,
    qty int NOT NULL,
    updated_at timestamptz,
    locked boolean NOT NULL DEFAULT false
);

CREATE TABLE a (id int);
CREATE TABLE b (id int);
CREATE TABLE c (id int);
CREATE TABLE only_this (id int);
CREATE TABLE t (a int, b int, id int);
CREATE TABLE t1 (id int, a int, b int, c int, x int, y int);
CREATE TABLE t2 (id int);

INSERT INTO customers (id, "Name", tags, score, active) VALUES
    (1, 'alice', '{vip}', 10, true),
    (2, 'bob', '{}', 55.5, true),
    (3, 'carol', '{vip,new}', 0, false);
INSERT INTO orders (id, customer_id, ordered_at, amount, status, carrier_id) VALUES
    (1, 1, '2026-06-01 10:00:00+00', 1200, 'packed', 1),
    (2, 1, '2026-06-15 10:00:00+00', 300, 'cancelled', NULL),
    (3, 2, '2026-07-01 10:00:00+00', 20000, 'packed', 2),
    (4, 3, '2026-07-02 10:00:00+00', 50, 'shipped', 1);
INSERT INTO blacklist VALUES (3);
INSERT INTO accounts (id, code, payload, tags, price, name, score, ids, balance) VALUES
    (1, 'A1', '{"items": [3, 4]}', '{x,y,z}', 100, 'Foo bar', 5, '{1,2,3}', 500),
    (2, NULL, '{"items": []}', '{}', 12.5, 'baz', 20, '{}', 0);
INSERT INTO logs VALUES (1, 1, 'opened'), (2, 1, 'deposit'), (3, 2, 'opened');
INSERT INTO users (id, name, active, points) VALUES (1, 'alice', true, 0), (2, 'bob', false, 3);
INSERT INTO sessions VALUES (1), (2);
INSERT INTO carriers VALUES (1, 'yamato'), (2, 'sagawa');
INSERT INTO staging_stock VALUES ('apple', 3, 1), ('pear', 5, 1), ('plum', 1, 2);
INSERT INTO stock (sku, qty) VALUES ('apple', 10);
INSERT INTO a VALUES (1), (2), (3);
INSERT INTO b VALUES (2), (3), (4);
INSERT INTO c VALUES (3), (5);
INSERT INTO t VALUES (1, 2, 1), (3, 4, 2);
INSERT INTO t1 VALUES (1, 1, 2, 3, 1, 2);
INSERT INTO t2 VALUES (1);
