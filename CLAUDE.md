# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Run the API server
cargo run -- serve

# Run all tests
cargo test

# Run tests for a specific crate
cargo test -p services

# Lint
cargo clippy

# Database migrations
cargo run -- migrate up          # Apply all pending migrations
cargo run -- migrate up 5        # Apply next 5 migrations
cargo run -- migrate down        # Rollback last migration
cargo run -- migrate down 3      # Rollback last 3 migrations
cargo run -- migrate fresh       # Drop all tables and reapply from scratch
cargo run -- migrate refresh     # Rollback all, then reapply all
cargo run -- migrate reset       # Rollback all migrations
cargo run -- migrate status      # Show pending/applied status

# Entities regeneration after migration
sea-orm-cli generate entity -u <connection_url> -o entities/src/entities
```

The API listens on `127.0.0.1:3000`.

## Configuration

`config.yaml` (root) is the sole config source — no `.env`. Required fields:

```yaml
jwt:
  secret: <signing secret>
db:
  connection_url: postgres://postgres:postgres@localhost/mia_development
  schema: public
tmdb:
  authorization_token: <TMDB bearer token>
logging:
  level: debug        # trace | debug | info | warning | error
media:
  unset_title: "Unknown"
images:
  store_path: /path/to/images
```

## Architecture

This is a Rust workspace where each crate is a strict layer — dependencies only flow downward.

```
src/main.rs          CLI entrypoint (parses "serve" / "migrate" subcommands)
api/                 HTTP layer: Axum routing, middleware, endpoint handlers
services/            Business logic and all SeaORM database queries
entities/            SeaORM entity definitions (auto-generated from DB schema)
views/               Response DTOs (Serde serialization)
transpiler/          Transpilation of search queries into SeaORM queries
integrations/        External API clients (TMDB only)
infrastructure/      Config loading, error types, shared initialisation
migrations/          SeaORM migration files
```

### Image handling

Images are downloaded from TMDB, resized to multiple fixed dimensions, and stored on the local filesystem under `config.images.store_path`.
