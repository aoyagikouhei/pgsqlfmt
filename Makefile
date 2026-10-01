COMPOSE := docker compose
# CI のように端末がない環境では `make check RUN_FLAGS=-T` とする
RUN := $(COMPOSE) run --rm $(RUN_FLAGS) dev

.PHONY: up down build shell test fmt clippy check psql db-reset logs snapshot-review image

up: ## コンテナを起動
	$(COMPOSE) up -d --build

down: ## コンテナを停止
	$(COMPOSE) down

build: ## cargo build
	$(RUN) cargo build

shell: ## 開発コンテナに入る
	$(RUN) bash

test: ## cargo test
	$(RUN) cargo test

fmt: ## cargo fmt
	$(RUN) cargo fmt

clippy: ## cargo clippy
	$(RUN) cargo clippy --all-targets -- -D warnings

check: ## fmt チェック + clippy + test
	$(RUN) sh -c 'cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test'

psql: ## PostgreSQL に psql で接続
	$(COMPOSE) exec db psql -U postgres -d sql_formatter

db-reset: ## DB ボリュームを削除して作り直す
	$(COMPOSE) rm -sfv db
	-docker volume rm sql-formatter-rs_pg-data
	$(COMPOSE) up -d db

image: ## 配布用の sql-formatter イメージを作る
	docker build -t sql-formatter .

logs:
	$(COMPOSE) logs -f

snapshot-review: ## スナップショットの差分を確認して承認
	$(RUN) cargo insta test --review
