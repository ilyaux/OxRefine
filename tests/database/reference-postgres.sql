-- Synthetic, redistributable input for OpenRefine 3.10.1 database-extension references.
-- Run only in an isolated test database; credentials and role setup stay outside this file.
CREATE TABLE typed_fixture (
    id integer PRIMARY KEY,
    intv integer,
    bigv bigint,
    floatv double precision,
    exactv numeric(38,8),
    boolv boolean,
    datev date,
    timev time,
    stampv timestamp,
    zstampv timestamptz,
    textv text,
    emptyv text,
    nullable text,
    bytesv bytea
);
INSERT INTO typed_fixture VALUES
    (1,42,9223372036854775807,1.25,123456789012345678901234567890.12345678,true,
     '2024-01-02','12:34:56.123456','2024-01-02 12:34:56.123456','2024-01-02 12:34:56.123456+02',
     'Zażółć Привет 東京','',NULL,decode('00017fff','hex')),
    (2,-7,-9223372036854775808,-0.5,-0.00000001,false,
     '1969-12-31','00:00:00','1969-12-31 23:59:59','1969-12-31 23:59:59+00',
     'synthetic only','',NULL,decode('','hex')),
    (3,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL);
