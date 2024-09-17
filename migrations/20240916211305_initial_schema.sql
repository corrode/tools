-- Create the schema
CREATE SCHEMA IF NOT EXISTS twir;

-- Create the entries table
CREATE TABLE twir.entries (
    id SERIAL PRIMARY KEY,
    title TEXT NOT NULL,
    url TEXT NOT NULL UNIQUE,
    category TEXT NOT NULL,
    date DATE NOT NULL,
    text TEXT
);

-- Create an index on the url column for faster lookups
CREATE INDEX idx_entries_url ON twir.entries (url);

-- Create an index on the date column for faster date-based queries
CREATE INDEX idx_entries_date ON twir.entries (date);
