-- Run only in the disposable synthetic fixture database, before measuring anything.
CREATE TABLE db_bench_fixture (
    id INTEGER PRIMARY KEY,
    label VARCHAR(40) NOT NULL,
    quantity INTEGER NULL
);
INSERT INTO db_bench_fixture
SELECT i, 'item-' || i, CASE WHEN i % 97 = 0 THEN NULL ELSE i % 1000 END
FROM generate_series(0, 99999) AS i;
