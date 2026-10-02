COMPOSE := docker compose
# In environments without a terminal, such as CI, use `make check RUN_FLAGS=-T`
RUN := $(COMPOSE) run --rm $(RUN_FLAGS) dev

.PHONY: up down build shell test fmt clippy check psql db-reset logs snapshot-review image

up: ## Start the containers
	$(COMPOSE) up -d --build

down: ## Stop the containers
	$(COMPOSE) down

build: ## cargo build
	$(RUN) cargo build

shell: ## Open a shell in the dev container
	$(RUN) bash

test: ## cargo test
	$(RUN) cargo test

fmt: ## cargo fmt
	$(RUN) cargo fmt

clippy: ## cargo clippy
	$(RUN) cargo clippy --all-targets -- -D warnings

check: ## fmt check + clippy + test
	$(RUN) sh -c 'cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test'

psql: ## Connect to PostgreSQL with psql
	$(COMPOSE) exec db psql -U postgres -d sql_formatter

db-reset: ## Delete and recreate the database volume
	$(COMPOSE) rm -sfv db
	-docker volume rm sql-formatter-rs_pg-data
	$(COMPOSE) up -d db

image: ## Build the pgsqlfmt distribution image
	docker build -t pgsqlfmt .

logs:
	$(COMPOSE) logs -f

snapshot-review: ## Review and accept snapshot diffs
	$(RUN) cargo insta test --review
