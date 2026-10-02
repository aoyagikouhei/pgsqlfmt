-- monthly sales report per customer
WITH monthly AS (
  SELECT c.id AS customer_id,
         date_trunc('month', o.ordered_at) AS month,
         sum(o.amount) FILTER (WHERE o.status <> 'cancelled') AS total
  FROM customers c
  JOIN orders o ON o.customer_id = c.id
  WHERE o.ordered_at >= now() - interval '1 year'
  GROUP BY 1, 2
)
SELECT m.customer_id,
       m.month,
       m.total,
       rank() OVER (PARTITION BY m.month ORDER BY m.total DESC NULLS LAST) AS rnk, /* rank within the month */
       CASE WHEN m.total > 10000 THEN 'gold' WHEN m.total > 1000 THEN 'silver' ELSE 'bronze' END AS tier
FROM monthly AS m
WHERE m.customer_id IN (SELECT id FROM customers WHERE active)
  AND NOT EXISTS (SELECT 1 FROM blacklist b WHERE b.customer_id = m.customer_id)
ORDER BY m.month DESC, rnk
LIMIT 100;
