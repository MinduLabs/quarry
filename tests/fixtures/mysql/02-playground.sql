-- playground.samples: 5000 rows with one column of every MySQL type, for trying the app by hand
-- (grid, editing, copy, filters, sorting, structure). `just db-up` loads it.
--
-- No RAND(): every machine gets the same rows. Rows 1-10 are hand-picked edge cases; rows 11-5000
-- are generated, with a NULL here and there so every column shows NULL too.
--
--   1  smallest value of every type          6  long text, 4 KB blob, deeply nested JSON
--   2  largest value of every type           7  fractional seconds, negative time
--   3  every nullable column NULL            8  zero dates ('0000-00-00')
--   4  empty strings and the text 'NULL'     9  JSON null (not SQL NULL)
--   5  quotes, backslash, newlines, emoji   10  JSON array at the root
--
-- `grp` has ten values (ties for sorting and paging), `code` is unique, `c_virtual` and
-- `c_stored` are generated columns (not editable), `c_text` has a FULLTEXT index.
SET NAMES utf8mb4;

CREATE DATABASE playground;
USE playground;

CREATE TABLE samples (
  id INT AUTO_INCREMENT PRIMARY KEY COMMENT 'Primary key',
  grp TINYINT UNSIGNED NOT NULL COMMENT 'Ten values: ties for sorting, filters and paging',
  code VARCHAR(16) NOT NULL COMMENT 'Unique',
  c_tinyint TINYINT, c_utinyint TINYINT UNSIGNED, c_smallint SMALLINT, c_mediumint MEDIUMINT,
  c_int INT, c_uint INT UNSIGNED, c_bigint BIGINT, c_ubigint BIGINT UNSIGNED, c_bool BOOLEAN,
  c_float FLOAT, c_double DOUBLE, c_decimal DECIMAL(12, 2), c_decimal_wide DECIMAL(40, 10),
  c_char CHAR(8), c_varchar VARCHAR(100), c_text TEXT,
  c_binary BINARY(8), c_varbinary VARBINARY(32), c_blob BLOB,
  c_date DATE, c_time TIME(3), c_datetime DATETIME(6), c_timestamp TIMESTAMP NULL, c_year YEAR,
  c_bit BIT(8), c_json JSON, c_enum ENUM('small', 'medium', 'large'), c_set SET('a', 'b', 'c', 'd'),
  c_point POINT,
  c_virtual BIGINT GENERATED ALWAYS AS (c_int * 2) VIRTUAL,
  c_stored VARCHAR(20) GENERATED ALWAYS AS (CONCAT('#', code)) STORED,
  UNIQUE KEY uq_samples_code (code),
  KEY idx_samples_grp (grp),
  KEY idx_samples_grp_int (grp, c_int),
  FULLTEXT KEY ft_samples_text (c_text)
) COMMENT = 'One column of every type; rows 1-10 are edge cases';

-- 1: smallest value of every type
INSERT INTO samples SET id = 1, grp = 0, code = 'MIN',
  c_tinyint = -128, c_utinyint = 0, c_smallint = -32768, c_mediumint = -8388608,
  c_int = -2147483648, c_uint = 0, c_bigint = -9223372036854775808, c_ubigint = 0, c_bool = FALSE,
  c_float = -3.402823E38, c_double = -1.7976931348623157E308, c_decimal = -9999999999.99,
  c_decimal_wide = -99999999999999999999999999999.9999999999,
  c_char = '', c_varchar = '', c_text = '',
  c_binary = X'0000000000000000', c_varbinary = X'', c_blob = X'',
  c_date = '1000-01-01', c_time = '-838:59:59.000', c_datetime = '1000-01-01 00:00:00.000000',
  c_timestamp = '1970-01-01 00:00:01', c_year = 1901, c_bit = b'00000000',
  c_json = JSON_ARRAY(), c_enum = 'small', c_set = '', c_point = ST_GeomFromText('POINT(-180 -90)');

-- 2: largest value of every type
INSERT INTO samples SET id = 2, grp = 9, code = 'MAX',
  c_tinyint = 127, c_utinyint = 255, c_smallint = 32767, c_mediumint = 8388607,
  c_int = 2147483647, c_uint = 4294967295, c_bigint = 9223372036854775807,
  c_ubigint = 18446744073709551615, c_bool = TRUE,
  c_float = 3.402823E38, c_double = 1.7976931348623157E308, c_decimal = 9999999999.99,
  c_decimal_wide = 99999999999999999999999999999.9999999999,
  c_char = 'zzzzzzzz', c_varchar = REPEAT('x', 100), c_text = REPEAT('The quick brown fox. ', 200),
  c_binary = X'FFFFFFFFFFFFFFFF', c_varbinary = REPEAT(X'FF', 32), c_blob = REPEAT(X'FF', 4096),
  c_date = '9999-12-31', c_time = '838:59:59.000', c_datetime = '9999-12-31 23:59:59.999999',
  c_timestamp = '2038-01-19 03:14:07', c_year = 2155, c_bit = b'11111111',
  c_json = JSON_OBJECT('max', TRUE), c_enum = 'large', c_set = 'a,b,c,d',
  c_point = ST_GeomFromText('POINT(180 90)');

-- 3: every nullable column NULL
INSERT INTO samples (id, grp, code) VALUES (3, 0, 'NULLS');

-- 4: empty strings and the text 'NULL', which must not look like NULL
INSERT INTO samples SET id = 4, grp = 1, code = 'EMPTY',
  c_tinyint = 0, c_utinyint = 0, c_smallint = 0, c_mediumint = 0, c_int = 0, c_uint = 0,
  c_bigint = 0, c_ubigint = 0, c_bool = FALSE, c_float = 0, c_double = 0, c_decimal = 0,
  c_decimal_wide = 0, c_char = 'ab  ', c_varchar = 'NULL', c_text = '  padded  ',
  c_binary = X'', c_varbinary = X'', c_blob = X'', c_date = '2000-01-01', c_time = '00:00:00',
  c_datetime = '2000-01-01 00:00:00', c_timestamp = '2000-01-01 00:00:01', c_year = 2000,
  c_bit = b'0', c_json = JSON_OBJECT(), c_enum = 'medium', c_set = '',
  c_point = ST_GeomFromText('POINT(0 0)');

-- 5: quotes, backslash, % and _, newlines, tab, emoji, right-to-left and Vietnamese text
INSERT INTO samples SET id = 5, grp = 2, code = 'SPECIAL',
  c_varchar = 'He said "hi" & it''s 50% off_ \\ back',
  c_text = 'line 1\nline 2\ttabbed\r\nline 3: Xin chào 👋 Nguyễn Văn An · مرحبا · שלום · 你好',
  c_json = JSON_OBJECT('quote', 'he said "hi"', 'emoji', '👋', 'name', 'Nguyễn Văn An');

-- 6: long text, a 4 KB blob and deeply nested JSON
INSERT INTO samples SET id = 6, grp = 3, code = 'LONG',
  c_varchar = REPEAT('w', 100),
  c_text = REPEAT('Lorem ipsum dolor sit amet. ', 300),
  c_blob = REPEAT(X'DEADBEEF', 1024),
  c_json = JSON_OBJECT('a', JSON_OBJECT('b', JSON_OBJECT('c',
    JSON_ARRAY(1, JSON_OBJECT('d', JSON_ARRAY(1, 2, 3, JSON_OBJECT('e', 'deep'))))))) ;

-- 7: fractional seconds and a negative time
INSERT INTO samples SET id = 7, grp = 4, code = 'TIMES',
  c_date = '2026-10-16', c_time = '-12:34:56.789', c_datetime = '2026-10-16 13:37:00.123456',
  c_timestamp = '2026-10-16 06:37:00', c_year = 2026;

-- 8: zero dates, which only a relaxed sql_mode accepts (the app shows them as text)
SET @strict = @@SESSION.sql_mode;
SET SESSION sql_mode = '';
INSERT INTO samples SET id = 8, grp = 5, code = 'ZERO',
  c_date = '0000-00-00', c_datetime = '0000-00-00 00:00:00';
SET SESSION sql_mode = @strict;

-- 9: JSON null, which is not SQL NULL
INSERT INTO samples SET id = 9, grp = 6, code = 'JSONNULL', c_json = CAST('null' AS JSON);

-- 10: an array at the root of the JSON, with every JSON scalar
INSERT INTO samples SET id = 10, grp = 7, code = 'JSONARRAY',
  c_json = JSON_ARRAY(1, 'two', NULL, TRUE, 1.5, JSON_OBJECT('k', JSON_ARRAY()));

-- 11-5000: generated
SET SESSION cte_max_recursion_depth = 10000;
INSERT INTO samples (
  id, grp, code,
  c_tinyint, c_utinyint, c_smallint, c_mediumint, c_int, c_uint, c_bigint, c_ubigint, c_bool,
  c_float, c_double, c_decimal, c_decimal_wide,
  c_char, c_varchar, c_text, c_binary, c_varbinary, c_blob,
  c_date, c_time, c_datetime, c_timestamp, c_year, c_bit, c_json, c_enum, c_set, c_point
)
WITH RECURSIVE seq (n) AS (SELECT 11 UNION ALL SELECT n + 1 FROM seq WHERE n < 5000)
SELECT
  n, n % 10, CONCAT('S', LPAD(n, 5, '0')),
  IF(n % 13 = 0, NULL, (n % 256) - 128),
  IF(n % 17 = 0, NULL, n % 256),
  IF(n % 19 = 0, NULL, (n * 7) % 65536 - 32768),
  IF(n % 23 = 0, NULL, (n * 1009) % 16777216 - 8388608),
  IF(n % 29 = 0, NULL, (n * 1000003) % 4294967296 - 2147483648),
  IF(n % 31 = 0, NULL, (n * 1000003) % 4294967296),
  IF(n % 37 = 0, NULL, n * 1234567891011 * IF(n % 2 = 0, -1, 1)),
  IF(n % 41 = 0, NULL, CAST(n AS UNSIGNED) * 3000000000000000),
  IF(n % 43 = 0, NULL, n % 2),
  IF(n % 47 = 0, NULL, ROUND(n * 0.37, 2)),
  IF(n % 53 = 0, NULL, n / 7),
  IF(n % 59 = 0, NULL, ROUND((n * 7.31) % 99999999, 2)),
  IF(n % 61 = 0, NULL, n * 123456789012345.6789012345),
  IF(n % 67 = 0, NULL, LEFT(MD5(n), 8)),
  IF(n % 71 = 0, NULL,
     CONCAT('Sample ', n, ' ', ELT(1 + n % 5, 'alpha', 'beta', 'gamma', 'delta', 'epsilon'))),
  IF(n % 73 = 0, NULL, IF(n % 50 = 0, REPEAT(CONCAT('line ', n, '\n'), 40), CONCAT('Text of row ', n))),
  IF(n % 79 = 0, NULL, UNHEX(LEFT(MD5(n), 16))),
  IF(n % 83 = 0, NULL, UNHEX(MD5(n))),
  IF(n % 89 = 0, NULL, IF(n % 100 = 0, REPEAT(UNHEX(MD5(n)), 256), UNHEX(MD5(n)))),
  IF(n % 97 = 0, NULL, DATE('2000-01-01') + INTERVAL n DAY),
  IF(n % 101 = 0, NULL, SEC_TO_TIME(IF(n % 11 = 0, -1, 1) * ((n * 97) % 86400))),
  IF(n % 103 = 0, NULL, TIMESTAMP('2026-01-01') + INTERVAL n HOUR + INTERVAL n MICROSECOND),
  IF(n % 107 = 0, NULL, TIMESTAMP('2020-01-01 00:00:01') + INTERVAL n MINUTE),
  IF(n % 109 = 0, NULL, 1990 + n % 100),
  IF(n % 113 = 0, NULL, n % 256),
  IF(n % 127 = 0, NULL,
     JSON_OBJECT('n', n, 'name', CONCAT('item-', n), 'tags', JSON_ARRAY('a', n % 3),
                 'even', n % 2 = 0)),
  IF(n % 131 = 0, NULL, ELT(1 + n % 3, 'small', 'medium', 'large')),
  IF(n % 137 = 0, NULL, ELT(1 + n % 5, '', 'a', 'a,b', 'b,c,d', 'a,b,c,d')),
  IF(n % 139 = 0, NULL,
     ST_GeomFromText(CONCAT('POINT(', 105 + (n % 100) / 100, ' ', 10 + (n % 200) / 100, ')')))
FROM seq;

GRANT ALL PRIVILEGES ON playground.* TO 'quarry'@'%';
GRANT SELECT, SHOW VIEW ON playground.* TO 'demo'@'%';
