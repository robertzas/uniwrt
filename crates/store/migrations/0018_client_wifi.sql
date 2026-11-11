-- Wi-Fi clients' capabilities (features/wifi.md, Roaming: client capabilities; D-088): what each
-- showed the access points over its associations, as JSON (`ClientWifi`); NULL until it showed
-- anything.
ALTER TABLE clients ADD COLUMN wifi TEXT;
