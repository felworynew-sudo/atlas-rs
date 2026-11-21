-- Связь движений кассы CRM с отчётами инкассации дев-панели сайта.
-- До этого сайт вёл свою инкассацию в отдельном файле, а смена в CRM её не видела:
-- «Должно быть в кассе» не уменьшалось после изъятия. Теперь запись с сайта
-- становится обычным движением кассы смены, а external_id защищает от дублей
-- при повторной отправке того же отчёта.
ALTER TABLE cashbox_transactions ADD COLUMN IF NOT EXISTS external_id text;
ALTER TABLE cashbox_transactions ADD COLUMN IF NOT EXISTS source text NOT NULL DEFAULT 'crm';

CREATE UNIQUE INDEX IF NOT EXISTS cashbox_transactions_external_uniq
    ON cashbox_transactions (organization_id, external_id) WHERE external_id IS NOT NULL;
