このディレクトリの `*.sql` / `*.sh` は、DB ボリュームが空の初回起動時にだけ
ファイル名順で実行されます（postgres 公式イメージの `/docker-entrypoint-initdb.d`）。
変更を反映したいときは `make db-reset` でボリュームごと作り直してください。
