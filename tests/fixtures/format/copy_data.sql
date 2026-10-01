-- COPY のデータは入力のまま残し、後ろの文は整形する
copy accounts (id, owner) from stdin;
1	it's
2	a;b

\.

select id, owner from accounts where id = 1;

COPY logs FROM STDIN WITH (FORMAT csv);
"x","/* not a comment"
\.
insert into logs values (1, 1, 'done');
