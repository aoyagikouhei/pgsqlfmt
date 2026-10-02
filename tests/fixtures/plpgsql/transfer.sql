create or replace function public.transfer(p_from bigint, p_to bigint, p_amount numeric(12,2) default 0, out o_balance numeric)
returns numeric
language plpgsql
security definer
set search_path = public, pg_temp
as $body$
<<main>>
declare
  v_balance accounts.balance%type;
  v_row accounts%rowtype;
  c_log cursor (p_id bigint) for select * from logs where account_id = p_id order by id;
  k constant int := 10;
  v_ids bigint[] default '{}';
begin
  -- check the balance
  select balance into strict v_balance from accounts where id = p_from for update;
  if v_balance < p_amount then
    raise exception 'insufficient balance: % < %', v_balance, p_amount using errcode = 'P0001', hint = 'check';
  elsif p_amount = 0 then
    return 0;
  else
    update accounts set balance = balance - p_amount where id = p_from returning balance into o_balance;
  end if;

  case p_amount when 1, 2 then v_balance := 1; else null; end case;

  for r in select id, name from users where active loop
    continue when r.id = 0;
    perform notify_user(r.id);
  end loop;
  for i in reverse 10..1 by 2 loop exit main when i < 3; end loop;
  foreach v_id slice 1 in array v_ids loop v_balance = v_balance + 1; end loop;
  while v_balance > 0 loop v_balance := v_balance - k; end loop;
  loop exit; end loop;

  execute format('select count(*) from %I', 'logs') into v_balance using p_from;
  get diagnostics k = row_count;
  open c_log(p_from);
  fetch next from c_log into v_row;
  close c_log;
  create temp table tmp (id int) on commit drop;
  assert v_balance >= 0, 'negative';
  return query select 1;
  return next;
exception
  when division_by_zero or unique_violation then
    raise notice 'error: %', sqlerrm;
  when sqlstate '22012' then
    begin
      rollback;
    end;
end main;
$body$;
