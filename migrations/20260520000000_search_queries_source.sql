-- Migration: expose `source` ('ui' | 'api') on the `search_queries` view.
--
-- Both the HTML and the JSON API search handlers now log their requests
-- with the same message ('Search request') but distinguish themselves via a
-- structured `source` field. Surfacing it on the view lets the monitoring
-- dashboard filter the query log by traffic source.
--
-- Legacy rows (logged before this change) did not carry a `source` field;
-- COALESCE them to 'ui' since the only producer at that time was the HTML
-- handler.

DROP VIEW IF EXISTS search_queries;
CREATE VIEW search_queries AS
SELECT
    id,
    timestamp,
    json_extract(fields, '$.query')        AS query,
    json_extract(fields, '$.results')      AS result_count,
    json_extract(fields, '$.duration_ms')  AS latency_ms,
    json_extract(fields, '$.content_type') AS content_type,
    json_extract(fields, '$.sort_by')      AS sort_by,
    json_extract(fields, '$.page')         AS page,
    json_extract(fields, '$.start_year')   AS start_year,
    json_extract(fields, '$.end_year')     AS end_year,
    json_extract(fields, '$.referer')      AS referer,
    COALESCE(json_extract(fields, '$.source'), 'ui') AS source
FROM events
WHERE message = 'Search request';
