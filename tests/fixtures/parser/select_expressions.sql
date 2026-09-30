SELECT a.id::text || '-' || coalesce(a.code, 'n/a') AS label,
       (a.payload -> 'items' ->> 0)::int AS first_item,
       a.tags[1:2],
       a.created_at AT TIME ZONE 'Asia/Tokyo',
       CAST(a.price AS numeric(10, 2)) * 1.1,
       extract(year FROM a.created_at),
       a.name ILIKE '%foo%' ESCAPE '\',
       a.score BETWEEN 1 AND 10,
       a.deleted_at IS NULL,
       ARRAY[1, 2, 3] && a.ids,
       $1 = ANY (a.ids)
FROM accounts a;
