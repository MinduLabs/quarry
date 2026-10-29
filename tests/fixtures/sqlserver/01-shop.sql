-- Sample data for tests and demos: the same shop as the other fixtures, in SQL Server types.
CREATE DATABASE shop;
GO
USE shop;
GO

CREATE TABLE customers (
  id int IDENTITY PRIMARY KEY,
  email nvarchar(255) NOT NULL UNIQUE,
  full_name nvarchar(120) NOT NULL,
  country char(2) NOT NULL DEFAULT 'VN',
  created_at datetime2 NOT NULL DEFAULT sysdatetime()
);

CREATE TABLE orders (
  id bigint IDENTITY PRIMARY KEY,
  customer_id int NOT NULL CONSTRAINT fk_orders_customer REFERENCES customers (id) ON DELETE CASCADE,
  status varchar(12) NOT NULL DEFAULT 'pending',
  total decimal(12, 2) NOT NULL,
  note nvarchar(max) NULL,
  ordered_at datetimeoffset NOT NULL
);
CREATE INDEX idx_orders_status ON orders (status);
GO

CREATE VIEW paid_orders AS SELECT * FROM orders WHERE status = 'paid';
GO
CREATE FUNCTION order_count (@customer int) RETURNS bigint AS
BEGIN
  RETURN (SELECT count_big(*) FROM orders WHERE customer_id = @customer);
END;
GO
CREATE PROCEDURE touch_customer @customer int AS
  UPDATE customers SET full_name = full_name WHERE id = @customer;
GO
CREATE TRIGGER customers_touch ON customers AFTER UPDATE AS SET NOCOUNT ON;
GO

INSERT INTO customers (email, full_name, country) VALUES
  (N'an@example.com', N'Nguyễn Văn An', 'VN'),
  (N'binh@example.com', N'Trần Thị Bình', 'VN'),
  (N'chloe@example.com', N'Chloé Martin', 'FR'),
  (N'dev@example.com', N'Dev Patel', 'IN'),
  (N'emma@example.com', N'Emma Schmidt', 'DE');

-- 5000 orders, enough to page and to hit the row limit.
WITH n AS (
  SELECT TOP (5000) ROW_NUMBER() OVER (ORDER BY (SELECT NULL)) AS i
  FROM sys.all_objects a CROSS JOIN sys.all_objects b
)
INSERT INTO orders (customer_id, status, total, note, ordered_at)
SELECT 1 + i % 5,
       CHOOSE(1 + i % 4, 'pending', 'paid', 'shipped', 'cancelled'),
       CAST((i * 7.31) % 1000 AS decimal(12, 2)),
       CASE WHEN i % 10 = 0 THEN N'gift wrap' END,
       DATEADD(hour, i, CAST('2026-01-01T00:00:00+00:00' AS datetimeoffset))
FROM n;
GO

-- One column of most SQL Server types. The first row holds edge values, the second is all NULL.
CREATE SCHEMA playground;
GO
CREATE TABLE playground.samples (
  id int PRIMARY KEY,
  c_bit bit,
  c_tinyint tinyint,
  c_smallint smallint,
  c_bigint bigint,
  c_real real,
  c_float float,
  c_decimal decimal(38, 10),
  c_money money,
  c_char char(4),
  c_varchar varchar(50),
  c_nvarchar nvarchar(50),
  c_varbinary varbinary(16),
  c_date date,
  c_time time(7),
  c_datetime datetime,
  c_smalldatetime smalldatetime,
  c_datetime2 datetime2(7),
  c_datetimeoffset datetimeoffset(7),
  c_uniqueidentifier uniqueidentifier,
  c_xml xml
);
INSERT INTO playground.samples VALUES (
  1, 1, 255, -32768, 9223372036854775807, 1.5, 0.1,
  -1234567890123456789012345678.0123456789, 12.34, 'abcd', 'plain', N'đã sửa ✓', 0x00FF,
  '1999-12-31', '23:59:59.9999999', '2000-01-01T00:00:01.500', '2000-01-01T12:30:00',
  '2000-01-01T00:00:01.5000000', '2026-10-29T16:02:26.5560000+07:00',
  '01234567-89AB-CDEF-0123-456789ABCDEF', '<a>1</a>'
);
INSERT INTO playground.samples (id) VALUES (2);
GO
