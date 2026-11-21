-- Импорт продаж: партия, по которой созданные продажи можно откатить целиком.
ALTER TABLE sales ADD COLUMN import_batch_id uuid;
CREATE INDEX sales_import_batch_idx ON sales (import_batch_id) WHERE import_batch_id IS NOT NULL;
