-- 実機検証用。DDL と MERGE。整形の前後でカタログ上の定義と結果が同じになること。
create table products (id bigint generated always as identity (start with 100) primary key, sku text not null unique, name text collate "C" not null default '',
  price numeric(12, 2) not null default 0 check (price >= 0), tax numeric generated always as (price * 0.1) stored,
  category text check (category in ('food', 'tool')), created_on date default '2026-01-01', constraint products_name_check check (length(name) < 100));
create unlogged table if not exists product_tags (product_id bigint references products (id) on delete cascade, tag text, primary key (product_id, tag)) with (fillfactor = 90);
create table events (id bigint not null, happened_on date not null, payload jsonb) partition by range (happened_on);
create table events_2026 partition of events for values from ('2026-01-01') to ('2027-01-01');
create table product_copy (like products including defaults including constraints);
create index products_name_idx on products (lower(name) text_pattern_ops desc nulls last) include (price) where price > 0;
create unique index if not exists product_tags_tag_idx on product_tags using btree (tag, product_id);
create view expensive_products (id, name) as select id, name from products where price > 1000 with local check option;
create materialized view product_counts as select category, count(*) as n from products group by category with no data;
create table cheap as select id, sku from products where price < 10 with no data;
alter table products add column stock int not null default 0, add constraint products_stock_check check (stock >= 0) not valid,
  alter column category set default 'food', alter column created_on type timestamp using created_on::timestamp;
alter table products rename column sku to code;
alter table product_tags rename to tags;
create function products_upper_name() returns trigger language plpgsql as $$ begin new.name := upper(new.name); return new; end $$;
create function noop_trigger() returns trigger language plpgsql as $$ begin return null; end $$;
create trigger products_upper_name before insert or update of name, code on products for each row
  when (new.name is not null and new.name <> '') execute function products_upper_name();
create or replace trigger products_upper_name before insert or update of name on products for each row execute function products_upper_name();
create constraint trigger products_check after insert on products deferrable initially deferred for each row execute procedure noop_trigger();
create trigger tags_changed after update on tags referencing old table as old_rows new table new_rows for each statement execute function noop_trigger('x', 1);
create trigger expensive_insert instead of insert on expensive_products for each row execute function noop_trigger();
comment on table products is 'products';
comment on column products.price is 'price'
  ' (tax excluded)';
comment on function products_upper_name() is E'upper\'s';
comment on constraint products_name_check on products is $$check$$;
comment on trigger products_upper_name on products is 'trigger';
comment on materialized view product_counts is 'counts';
comment on index product_tags_tag_idx is 'tag index';
comment on column products.code is null;
insert into products (code, name, price, category) values ('a', 'apple', 100, 'food'), ('h', 'hammer', 2000, 'tool');
merge into products as p using (values ('a', 150::numeric), ('b', 5::numeric)) as s (code, price) on s.code = p.code
  when matched and s.price > 1000 then delete
  when matched then update set price = s.price, stock = p.stock + 1
  when not matched then insert (code, name, price) values (s.code, 'new', s.price)
  returning merge_action(), p.code, p.price;
select code, name, price, tax, stock, category from products order by code;
insert into product_copy (id, sku, name, price, category) values (1, 'c', 'copy', 1, 'food');
truncate table only product_copy, cheap restart identity cascade;
select count(*) from product_copy;
drop index if exists products_name_idx;
drop view if exists expensive_products cascade;
