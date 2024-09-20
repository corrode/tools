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

Put this into a `.env` file:

```sh
DATABASE_PASSWORD=password
DATABASE_URL=postgres://user:password@localhost/twir
```

Now start the local database in the background:

```sh
docker-compose up -d  
```