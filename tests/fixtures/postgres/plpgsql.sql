-- 実機検証用。関数を作って呼び出し、整形の前後で結果と NOTICE が同じになること。
create function classify(p_amount numeric, p_label text default 'x') returns text language plpgsql immutable as $$
declare
  v_result text := '';
  v_i int;
  v_ids int[] := array[3, 1, 2];
  v_id int;
begin
  if p_amount is null then
    return 'null';
  elsif p_amount < 0 then
    raise exception 'negative: %', p_amount using errcode = '22023';
  end if;
  case when p_amount > 1000 then v_result := 'large'; when p_amount > 100 then v_result := 'medium'; else v_result := 'small'; end case;
  for v_i in reverse 3..1 loop v_result := v_result || v_i; end loop;
  foreach v_id in array v_ids loop continue when v_id = 1; v_result := v_result || '-' || v_id; end loop;
  <<outer>> while true loop exit outer; end loop;
  return p_label || ':' || v_result;
end $$;

create function order_summary(p_customer bigint, out o_count int, out o_total numeric) language plpgsql stable as $$
declare
  r record;
begin
  o_count := 0; o_total := 0;
  for r in select amount from orders where customer_id = p_customer and status <> 'cancelled' order by id loop
    o_count := o_count + 1;
    o_total = o_total + r.amount;
  end loop;
  select count(*) into strict o_count from orders where customer_id = p_customer;
end $$;

create function sets() returns setof int language plpgsql as $$
begin
  return next 1;
  return query select id from a where id > 1 order by id;
  return query execute 'select $1 + 100' using 5;
end $$;

create function safe_div(a numeric, b numeric) returns numeric language plpgsql as $$
begin
  return a / b;
exception when division_by_zero then
  raise notice 'division by zero: % / %', a, b;
  return null;
end $$;

create function add(a int, b int) returns int language sql immutable strict return a + b;

create function add3(a int, b int, c int) returns int language sql begin atomic select add(add(a, b), c); end;

create procedure bump_points(p_user bigint, p_by int) language plpgsql as $$
begin
  update users set points = points + p_by where id = p_user;
  raise notice 'bumped %', p_user;
end $$;

select classify(null), classify(50), classify(500, 'y'), classify(5000);
select classify(-1);
select * from order_summary(1);
select * from sets() order by 1;
select safe_div(1, 0), safe_div(6, 3);
select add(1, 2), add3(1, 2, 3);
call bump_points(1, 5);
select id, points from users order by id;

do $$
declare
  n int;
begin
  perform classify(1);
  get diagnostics n = row_count;
  raise notice 'rows: %', n;
  begin
    perform 1 / 0;
  exception when others then
    raise notice 'caught: %', sqlerrm;
  end;
end $$;

do $$
declare
  n int;
begin
  select into strict n count(*) from users;
  raise notice 'users: %', n;
end $$;
