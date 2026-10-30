-- One column of most PostgreSQL types, for decoding tests. The first row holds edge values,
-- the second is all NULL.
CREATE SCHEMA playground;

CREATE TABLE playground.samples (
  id integer PRIMARY KEY,
  c_bool boolean,
  c_int2 smallint,
  c_int8 bigint,
  c_oid oid,
  c_float4 real,
  c_float8 double precision,
  c_numeric numeric(40, 10),
  c_money money,
  c_text text,
  c_bytea bytea,
  c_date date,
  c_time time(6),
  c_timetz timetz,
  c_timestamp timestamp(6),
  c_timestamptz timestamptz,
  c_interval interval,
  c_uuid uuid,
  c_json json,
  c_jsonb jsonb,
  c_inet inet,
  c_cidr cidr,
  c_macaddr macaddr,
  c_bit bit(4),
  c_int_array integer[],
  c_text_array text[],
  c_point point
);

INSERT INTO playground.samples VALUES (
  1, true, -32768, 9223372036854775807, 4294967295, 1.5, 0.1,
  -123456789012345678901234567890.0123456789, 12.34, 'đã sửa ✓', '\x00ff', '1999-12-31',
  '23:59:59.999999', '12:00:00+07', '2000-01-01 00:00:01.5', '2026-10-29 16:02:26.556+07',
  '1 year 2 mons 3 days 04:00:05', '01234567-89ab-cdef-0123-456789abcdef', '{"a": [1, 2]}',
  '{"b": true}', '10.0.0.1', '10.0.0.0/24', '08:00:2b:01:02:03', B'1010', '{{1,2},{3,NULL}}',
  '{"x","y \"z\""}', '(1,2)'
);
INSERT INTO playground.samples (id) VALUES (2);
