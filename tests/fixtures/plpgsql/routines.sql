create function add(a int, b int) returns int language sql immutable strict as $$ select a + b $$;

create procedure p() begin atomic insert into t values (1); select 1; end;

do $$ begin perform 1; end $$;

call p();
