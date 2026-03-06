.PHONY: start migrate migrate-prod test

# Start the project (standalone, with its own postgres)
start: build proxy
	docker compose up

start-dev: build-dev proxy
	docker compose -f compose.yaml -f compose.dev.yaml up --build

build-dev:
	docker compose -f compose.yaml -f compose.dev.yaml build

build:
	docker compose build

# Create a new migration (usage: make migrate NAME=create_urls)
migrate:
	sqlx migrate add $(NAME)

# Apply all pending migrations to the production database.
# DATABASE_URL env var takes precedence over settings.yaml.
# Usage (host):
#   make migrate-prod
#   DATABASE_URL=postgres://user:pass@host:5432/db make migrate-prod
# Usage (inside the prod container):
#   /home/app/scripts/migrate-prod.sh
migrate-prod:
	./scripts/migrate-prod.sh

# Run tests against the test database
test:
	cargo test

proxy:
	docker network create proxy --attachable &2> /dev/null
