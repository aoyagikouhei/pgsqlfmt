/* ファイル先頭のブロックコメント */
-- 行コメント
select
  a, -- a の説明
  b /* b の説明 */,
  -- c の前
  c
from t1 /* 表 */
join t2 on t1.id = t2.id -- 結合条件
where -- 条件
  x = 1

  -- 空行のあとのコメント
  and y = 2;


-- 文と文の間

select 1 -- 最後
