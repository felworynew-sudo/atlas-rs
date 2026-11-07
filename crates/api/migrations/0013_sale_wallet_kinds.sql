-- Выручка по методам оплаты зачисляется на счёт, заданный в настройках метода для филиала (проводки «sale» и «sale_return»).
ALTER TABLE wallet_transactions DROP CONSTRAINT wallet_transactions_kind_check;
ALTER TABLE wallet_transactions ADD CONSTRAINT wallet_transactions_kind_check CHECK (kind IN
    ('spending','supply_payment','manual','transfer','complex_sale_payment','salary','cashbox','payment','shift_close','sale','sale_return'));
