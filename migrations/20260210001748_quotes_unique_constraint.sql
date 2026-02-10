-- Add unique constraint on quotes(text, author) to support ON CONFLICT clause
CREATE UNIQUE INDEX idx_quotes_text_author ON quotes(text, author);