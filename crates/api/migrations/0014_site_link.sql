-- Связь с сайтом студии (дев-панель): устойчивые id филиалов и сотрудников сайта.
-- organizations.external_id — какая организация ведётся сайтом; stores.external_id — id филиала сайта;
-- users.external_id — id учётной записи дев-панели, users.staff_external_id — id карточки сотрудника сайта.
ALTER TABLE organizations ADD COLUMN external_id text;
ALTER TABLE stores ADD COLUMN external_id text;
ALTER TABLE users ADD COLUMN external_id text;
ALTER TABLE users ADD COLUMN staff_external_id text;
CREATE UNIQUE INDEX organizations_external_id_key ON organizations (external_id) WHERE external_id IS NOT NULL;
CREATE UNIQUE INDEX stores_external_id_key ON stores (organization_id, external_id) WHERE external_id IS NOT NULL;
CREATE UNIQUE INDEX users_external_id_key ON users (organization_id, external_id) WHERE external_id IS NOT NULL;
