-- Blocked clients (features/wifi.md, Client actions; D-084): when a user blocked a client, which
-- the controller then keeps off every managed access point; NULL while it isn't blocked.
ALTER TABLE clients ADD COLUMN blocked INTEGER;
