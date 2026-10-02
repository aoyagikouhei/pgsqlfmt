/* block comment at top of file */
-- line comment
select
  a, -- about a
  b /* about b */,
  -- before c
  c
from t1 /* table */
join t2 on t1.id = t2.id -- join condition
where -- condition
  x = 1

  -- comment after a blank line
  and y = 2;


-- between statements

select 1 -- last
