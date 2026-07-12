include .env
export

.PHONY: up down logs restart

up:
ifeq ($(DB_TYPE),docker)
	@echo "Starting with local PostgreSQL..."
	docker compose -f docker-compose.yml -f compose.psql.yml up -d
else
	@echo "Starting with remote PostgreSQL..."
	docker compose -f docker-compose.yml up -d
endif

down:
	-docker compose -f docker-compose.yml -f compose.psql.yml down 2>/dev/null
	-docker compose -f docker-compose.yml down 2>/dev/null

logs:
	docker compose -f docker-compose.yml logs -f

restart: down up
