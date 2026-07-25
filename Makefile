include .env
export

.PHONY: up down logs restart build

up:
	@echo "Starting with PostgreSQL ($(DB_DEPLOYMENT)) + ClickHouse ($(CLICKHOUSE_DEPLOYMENT))..."
ifeq ($(DB_DEPLOYMENT),remote)
	$(eval PSQL_FRAG := )
else
	$(eval PSQL_FRAG := -f compose.psql.yml)
endif
ifeq ($(CLICKHOUSE_DEPLOYMENT),local)
	$(eval CH_FRAG := -f compose.clickhouse.yml)
else
	$(eval CH_FRAG := )
endif
	docker compose -f docker-compose.yml $(PSQL_FRAG) $(CH_FRAG) up -d

down:
	-docker compose -f docker-compose.yml -f compose.psql.yml -f compose.clickhouse.yml down 2>/dev/null
	-docker compose -f docker-compose.yml down 2>/dev/null

logs:
	docker compose -f docker-compose.yml logs -f

restart: down up

build:
	docker compose build $(filter-out build,$(MAKECMDGOALS))

# Allow build target to receive extra docker compose build flags (e.g. --no-cache)
%:
	@true
