The `*.sql` / `*.sh` files in this directory are executed in file-name order only on the
first start, when the database volume is empty (the postgres official image's `/docker-entrypoint-initdb.d`).
To apply changes, recreate the volume with `make db-reset`.
