CREATE TABLE test(i INTEGER, j INTEGER, t BIGINT TIME INDEX);

INSERT INTO test VALUES (1, 1, 1), (NULL, 1, 2), (1, NULL, 3);

SELECT * FROM test ORDER BY i NULLS FIRST, j NULLS LAST;

SELECT * FROM test ORDER BY i NULLS FIRST, j NULLS FIRST;

SELECT * FROM test ORDER BY i NULLS LAST, j NULLS FIRST;

-- TODO(ruihang): The following two SQL will fail under distributed mode with error
-- Error: 1003(Internal), status: Internal, message: "Failed to collect recordbatch, source: Failed to poll stream, source: Arrow error: Invalid argument error: batches[0] schema is different with argument schema.\n            batches[0] schema: Schema { fields: [Field { name: \"i\", data_type: Int32, nullable: true, dict_id: 0, dict_is_ordered: false, metadata: {} }, Field { name: \"j\", data_type: Int32, nullable: true, dict_id: 0, dict_is_ordered: false, metadata: {} }, Field { name: \"t\", data_type: Int64, nullable: false, dict_id: 0, dict_is_ordered: false, metadata: {\"greptime:time_index\": \"true\"} }], metadata: {\"greptime:version\": \"0\"} },\n            argument schema: Schema { fields: [Field { name: \"i\", data_type: Int32, nullable: true, dict_id: 0, dict_is_ordered: false, metadata: {} }, Field { name: \"j\", data_type: Int32, nullable: true, dict_id: 0, dict_is_ordered: false, metadata: {} }, Field { name: \"t\", data_type: Int64, nullable: false, dict_id: 0, dict_is_ordered: false, metadata: {\"greptime:time_index\": \"true\"} }], metadata: {} }\n            ", details: [], metadata: MetadataMap { headers: {"inner_error_code": "Internal"} }
-- SELECT i, j, row_number() OVER (PARTITION BY i ORDER BY j NULLS FIRST) FROM test ORDER BY i NULLS FIRST, j NULLS FIRST;

-- SELECT i, j, row_number() OVER (PARTITION BY i ORDER BY j NULLS LAST) FROM test ORDER BY i NULLS FIRST, j NULLS FIRST;

SELECT * FROM test ORDER BY i NULLS FIRST, j NULLS LAST LIMIT 2;

SELECT * FROM test ORDER BY i NULLS LAST, j NULLS LAST LIMIT 2;

SELECT * FROM test ORDER BY i;

SELECT * FROM test ORDER BY i NULLS FIRST;

SELECT * FROM test ORDER BY i NULLS LAST;

DROP TABLE test;
