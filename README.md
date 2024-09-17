# TWIR

This project crawls and analyzes "This Week in Rust" posts, storing the data in a PostgreSQL database.

Show HN: I built a search engine for 'This Week in Rust'

## Getting Started

Follow these steps to set up and run the project on your local machine.

### PostgreSQL Setup

1. **Install PostgreSQL**:
   - On Ubuntu/Debian: `sudo apt-get install postgresql`
   - On macOS with Homebrew: `brew install postgresql`
   - For other systems, follow the [official PostgreSQL installation guide](https://www.postgresql.org/download/).

2. **Start PostgreSQL**:
   - On Ubuntu/Debian: `sudo service postgresql start`
   - On macOS: `brew services start postgresql`

### PostgreSQL Init

There's probably a better way to do this, but you have to set up your DB user manually for now:

```sh
psql postgres
postgres=# CREATE USER username WITH PASSWORD 'password' CREATEDB;
```

Put this into a `.env` file:

```sh
DATABASE_URL=postgres://username:password@localhost/twir
```

```sh
sqlx database create                                                                                           ✘
sqlx migrate add initial_schema
```