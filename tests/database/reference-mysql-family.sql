-- Synthetic input shared by the MySQL 8.4.13 and MariaDB 11.4.13 reference databases.
-- Run only in an isolated test database. Use UTF-8 and a UTC server time zone.
CREATE TABLE typed_fixture (
    id INTEGER PRIMARY KEY,
    intv INTEGER,
    bigv BIGINT,
    floatv DOUBLE,
    exactv DECIMAL(38,8),
    boolv BOOLEAN,
    datev DATE,
    timev TIME(6),
    stampv DATETIME(6),
    zstampv TIMESTAMP(6) NULL,
    textv VARCHAR(100) CHARACTER SET utf8mb4,
    emptyv VARCHAR(100),
    nullable VARCHAR(100),
    bytesv BLOB
);
INSERT INTO typed_fixture VALUES
    (1,42,9223372036854775807,1.25,123456789012345678901234567890.12345678,true,
     '2024-01-02','12:34:56.123456','2024-01-02 12:34:56.123456','2024-01-02 10:34:56.123456',
     'Zażółć Привет 東京','',NULL,UNHEX('00017fff')),
    (2,-7,-9223372036854775808,-0.5,-0.00000001,false,
     '1969-12-31','00:00:00','1969-12-31 23:59:59','2024-01-03 00:00:00',
     'synthetic only','',NULL,UNHEX('')),
    (3,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL);
CREATE TABLE unsigned_fixture (id INTEGER PRIMARY KEY, uintv BIGINT UNSIGNED);
INSERT INTO unsigned_fixture VALUES (1,18446744073709551615);
-- BOOLEAN is a TINYINT(1) alias and can contain values other than 0/1.
CREATE TABLE boolean_storage_fixture (id INTEGER PRIMARY KEY, boolv BOOLEAN);
INSERT INTO boolean_storage_fixture VALUES (1,2),(2,-7),(3,0);
