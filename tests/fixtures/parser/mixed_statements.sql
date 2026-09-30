-- 対応していない文はそのまま残る
CREATE TABLE users (
  id bigint PRIMARY KEY,
  name text NOT NULL
);

INSERT INTO users (id, name) VALUES (1, 'alice');

SELECT * FROM users WHERE id = 1;

SELECT a,, b FROM t WHERE; -- 壊れた文
