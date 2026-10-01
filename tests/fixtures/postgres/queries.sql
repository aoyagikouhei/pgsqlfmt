-- 実機検証用。tests/postgres/schema.sql の上で、すべての文がエラーなく実行できること。
with totals as (
  select o.customer_id, sum(o.amount) filter (where o.status <> 'cancelled') as total, count(*) cnt
  from orders o group by o.customer_id
), ranked as (select customer_id, total, rank() over (order by total desc nulls last) as rnk from totals)
select c.id, c."Name", r.total, r.rnk,
  case when r.total > 10000 then 'gold' when r.total > 1000 then 'silver' else 'bronze' end as tier
from customers c join ranked r on r.customer_id = c.id
where c.active and not exists (select 1 from blacklist b where b.customer_id = c.id)
  and c.id in (select customer_id from orders where amount between 100 and 100000)
order by r.rnk, c.id;

select a.id::text || '-' || coalesce(a.code, 'n/a') as label, (a.payload -> 'items' ->> 0)::int as first_item,
  a.tags[1:2] as head_tags, cast(a.price as numeric(10, 2)) * 1.1 as with_tax, a.name ilike '%foo%' as is_foo,
  a.score between symmetric 10 and 1 as in_range, a.deleted_at is null as alive, array[1, 2, 3] && a.ids as overlaps,
  2 = any (a.ids) as has_two, extract(year from a.created_at) as y, a.code is not distinct from null as no_code,
  -a.score as neg, 2 ^ 3 ^ 2 as pow, 10 - 3 - 2 as sub, not a.score > 10 and a.score < 100 as logic
from accounts a order by a.id;

select id from a union all select id from b intersect (select id from c order by id limit 5) order by id;

select x, y from (values (1, 'one'), (2, 'two')) as v (x, y) where x > 1 or y like 'o%' order by x offset 0 rows fetch first 10 rows only;

select status, count(*), string_agg(id::text, ',' order by id desc), percentile_cont(0.5) within group (order by amount)
from orders group by grouping sets ((status), ()) having count(*) > 0 order by status nulls first;

select c.id, o.id as order_id from customers c left outer join orders o using (id) cross join lateral (select c.score * 2 as doubled) d
where d.doubled >= 0 order by 1, 2;

select distinct on (customer_id) customer_id, amount from orders order by customer_id, amount desc;

select id, lag(amount) over w as prev, sum(amount) over (partition by customer_id order by id rows between unbounded preceding and current row) as running
from orders window w as (order by id) order by id;

select exists (select 1 from users where active), (select max(points) from users), array(select id from a order by id), row(1, 'x');

insert into staging_stock (sku, qty, batch_id) values ('kiwi', 7, 9), ('lime', 2, 9) returning sku, qty;

insert into stock as s (sku, qty, updated_at)
select sku, qty, null from staging_stock where batch_id = 1
on conflict (sku) do update set qty = s.qty + excluded.qty where s.locked = false
returning s.sku, s.qty;

update orders o set status = 'shipped', (carrier) = (select c.name from carriers c where c.id = o.carrier_id)
from carriers c where c.id = o.carrier_id and o.status = 'packed' returning o.id, o.status, o.carrier;

delete from sessions s using users u where u.id = s.user_id and not u.active returning s.user_id;

select sku, qty from stock order by sku;

-- 改行を挟んだ文字列はつながる
select 'con'
  'tinued' as s, 1;

-- 抽出方法は入力のまま、TABLESAMPLE と REPEATABLE はキーワード
select c.id from customers as c tablesample bernoulli (100) repeatable (1) where c.id > 0 order by c.id;
