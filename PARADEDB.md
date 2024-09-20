Here's some docs for how to do fulltext search with paradedb. Show me how to change my Rust code to provide 

API

Quickstart

This guide will walk you through the following steps to get started with ParadeDB:

Full text search

Similarity (i.e. vector) search

Hybrid search

​

Full Text Search

ParadeDB comes with a helpful procedure that creates a table populated with mock data to help you get started. Once connected with psql, run the following commands to create and inspect this table.

CALL paradedb.create_bm25_test_table(
  schema_name => 'public',
  table_name => 'mock_items'
);

SELECT description, rating, category
FROM mock_items
LIMIT 3;


Next, let’s create a BM25 index called search_idx on this table. We’ll index the description and category fields and configure the tokenizer on the description field.

CALL paradedb.create_bm25(
        index_name => 'search_idx',
        schema_name => 'public',
        table_name => 'mock_items',
        key_field => 'id',
        text_fields => paradedb.field('description', tokenizer => paradedb.tokenizer('en_stem')) ||
                       paradedb.field('category'),
        numeric_fields => paradedb.field('rating')
);


Note the mandatory key_field option. Every BM25 index needs a key_field, which should be the name of a column that will function as a row’s unique identifier within the index. Usually, the key_field can just be the name of your table’s primary key column.

We’re now ready to execute a full-text search. We’ll look for rows with a rating greater than 2 where description matches keyboard or category matches electronics.

SELECT description, rating, category
FROM search_idx.search(
  '(description:keyboard OR category:electronics) AND rating:>2',
  limit_rows => 5
);


Note the usage of limit_rows instead of the SQL LIMIT clause. For optimal performance, we recommend always using limit_rows and offset_rows instead of LIMIT and OFFSET.

Similarly, the rating column was indexed and the rating:>2 filter was used instead of the SQL WHERE clause for efficient filtering.

Next, let’s see how ParadeDB handles a phrase query like bluetooth speaker. Let’s also surface results even if there is a word between bluetooth and speaker.

SELECT description, rating, category
FROM search_idx.search('description:"bluetooth speaker"~1');


Note that phrases must be wrapped in double quotes. Also note our use of the ~1 slop operator, which tells ParadeDB to return matches even if they are separated by 1 word.

Next, let’s match against a partial word like blue. To do this, we’ll create a new index that uses the ngrams tokenizer, which splits text into chunks of size n.

CALL paradedb.create_bm25(
        index_name => 'ngrams_idx',
        schema_name => 'public',
        table_name => 'mock_items',
        key_field => 'id',
        text_fields => paradedb.field('description', tokenizer => paradedb.tokenizer('ngram', min_gram => 4, max_gram => 4, prefix_only => false)) ||
                       paradedb.field('category')
);

SELECT description, rating, category
FROM ngrams_idx.search('description:blue');


Finally, let’s use the snippet function to examine the BM25 scores and generate highlighted snippets for our results.

SELECT * FROM ngrams_idx.snippet(
  'description:blue',
  highlight_field => 'description'
);


Let’s join this result with our mock_items table to see the BM25 scores and highlighted snippets next to the original data:

WITH snippet AS (
    SELECT * FROM ngrams_idx.snippet(
      'description:blue',
      highlight_field => 'description'
    )
)
SELECT description, snippet, score_bm25
FROM snippet
LEFT JOIN mock_items ON snippet.id = mock_items.id;


Please refer to our advanced search queries guide for all available query types.

