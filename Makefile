include .env
export

.PHONY: up down logs restart build

up:
ifeq ($(DB_DEPLOYMENT),remote)
	@echo "Starting with remote PostgreSQL..."
	docker compose -f docker-compose.yml up -d
else
	@echo "Starting with local PostgreSQL..."
	docker compose -f docker-compose.yml -f compose.psql.yml up -d
endif

down:
	-docker compose -f docker-compose.yml -f compose.psql.yml down 2>/dev/null
	-docker compose -f docker-compose.yml down 2>/dev/null

logs:
	docker compose -f docker-compose.yml logs -f

restart: down up

build:
	docker compose build $(filter-out build,$(MAKECMDGOALS))

# Allow build target to receive extra docker compose build flags (e.g. --no-cache)
%:
	@true
