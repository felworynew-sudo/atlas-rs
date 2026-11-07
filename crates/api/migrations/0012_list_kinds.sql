-- Новые виды универсальных списков: модификаторы товаров и шаблоны печати.
ALTER TABLE list_items DROP CONSTRAINT list_items_kind_check;
ALTER TABLE list_items ADD CONSTRAINT list_items_kind_check CHECK (kind IN
    ('payment_methods','purchase_methods','discounts','return_reasons','writeoff_reasons','spending_categories','product_modifiers','print_templates'));
