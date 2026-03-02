.PHONY: start migrate test

# Start the project (standalone, with its own postgres)
start:
	docker compose up --build

# Create a new migration (usage: make migrate NAME=create_urls)
migrate:
	sqlx migrate add $(NAME)

# Run tests against the test database
test:
	DATABASE_URL=postgres://dev:somepass@postgres:5432/diminish_test \
		cargo test
