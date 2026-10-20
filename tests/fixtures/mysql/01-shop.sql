-- Sample data for tests and demos.
SET NAMES utf8mb4;
USE shop;

CREATE TABLE customers (
  id INT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  email VARCHAR(255) NOT NULL UNIQUE,
  full_name VARCHAR(120) NOT NULL,
  country CHAR(2) NOT NULL DEFAULT 'VN',
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE products (
  id INT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  sku VARCHAR(32) NOT NULL UNIQUE,
  name VARCHAR(200) NOT NULL,
  price DECIMAL(12, 2) NOT NULL,
  in_stock BOOLEAN NOT NULL DEFAULT TRUE,
  attributes JSON NULL
);

CREATE TABLE orders (
  id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  customer_id INT UNSIGNED NOT NULL,
  status ENUM('pending', 'paid', 'shipped', 'cancelled') NOT NULL DEFAULT 'pending',
  total DECIMAL(12, 2) NOT NULL,
  note TEXT NULL,
  ordered_at DATETIME NOT NULL,
  CONSTRAINT fk_orders_customer FOREIGN KEY (customer_id) REFERENCES customers(id),
  INDEX idx_orders_status (status)
);

CREATE TABLE all_types (
  id INT PRIMARY KEY,
  c_tinyint TINYINT, c_bigint BIGINT, c_ubigint BIGINT UNSIGNED,
  c_float FLOAT, c_double DOUBLE, c_decimal DECIMAL(30, 10),
  c_char CHAR(4), c_varchar VARCHAR(50), c_text TEXT, c_blob BLOB,
  c_date DATE, c_time TIME, c_datetime DATETIME(3), c_timestamp TIMESTAMP NULL,
  c_year YEAR, c_bit BIT(8), c_json JSON, c_enum ENUM('a', 'b'), c_set SET('x', 'y')
);

CREATE VIEW paid_orders AS SELECT * FROM orders WHERE status = 'paid';

-- A trigger and a disabled event, so the tree lists both kinds. Neither changes data.
CREATE TRIGGER products_touch BEFORE UPDATE ON products FOR EACH ROW SET NEW.name = NEW.name;
CREATE EVENT nightly_report ON SCHEDULE EVERY 1 DAY DISABLE DO SELECT 1;

INSERT INTO customers (email, full_name, country) VALUES
  ('an@example.com', 'Nguyễn Văn An', 'VN'),
  ('binh@example.com', 'Trần Thị Bình', 'VN'),
  ('chloe@example.com', 'Chloé Martin', 'FR'),
  ('dev@example.com', 'Dev Patel', 'IN'),
  ('emma@example.com', 'Emma Schmidt', 'DE');

INSERT INTO products (sku, name, price, in_stock, attributes) VALUES
  ('KB-01', 'Mechanical Keyboard', 89.90, TRUE, '{"switch": "brown", "layout": "ANSI"}'),
  ('MS-02', 'Wireless Mouse', 29.50, TRUE, '{"dpi": 1600}'),
  ('MN-27', '27" Monitor', 249.00, FALSE, NULL),
  ('HB-07', 'USB-C Hub', 39.99, TRUE, '{"ports": 7}');

-- 5000 orders to exercise paging
SET SESSION cte_max_recursion_depth = 10000;
INSERT INTO orders (customer_id, status, total, note, ordered_at)
WITH RECURSIVE seq (n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < 5000)
SELECT 1 + (n % 5),
       ELT(1 + (n % 4), 'pending', 'paid', 'shipped', 'cancelled'),
       ROUND(10 + (n * 7.31) % 490, 2),
       IF(n % 7 = 0, CONCAT('Gift wrap #', n), NULL),
       TIMESTAMP('2026-01-01') + INTERVAL n HOUR
FROM seq;

INSERT INTO all_types VALUES
  (1, -128, -9223372036854775808, 18446744073709551615, 1.5, 3.141592653589793, 12345678901234567890.0123456789,
   'abcd', 'Xin chào 👋', 'long text', X'DEADBEEF', '2026-10-16', '13:37:00', '2026-10-16 13:37:00.123',
   '2026-10-16 06:37:00', 2026, b'10101010', '{"a": [1, 2, 3]}', 'b', 'x,y'),
  (2, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL);

CREATE DATABASE inventory;
CREATE TABLE inventory.warehouses (id INT PRIMARY KEY, name VARCHAR(100), city VARCHAR(100));
INSERT INTO inventory.warehouses VALUES (1, 'Main', 'Hà Nội'), (2, 'South', 'TP.HCM');
GRANT ALL PRIVILEGES ON inventory.* TO 'quarry'@'%';

-- Read-only user without a password, for trying the UI where no keyring exists.
CREATE USER 'demo'@'%' IDENTIFIED BY '';
GRANT SELECT, SHOW VIEW ON shop.* TO 'demo'@'%';
GRANT SELECT ON inventory.* TO 'demo'@'%';
