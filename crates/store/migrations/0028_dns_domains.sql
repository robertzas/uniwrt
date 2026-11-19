-- DNS query statistics' top domains (features/network.md, Query statistics; D-133): a covering
-- index so counting one outcome's queries by domain reads the index alone, in domain order.
CREATE INDEX dns_queries_domains ON dns_queries (outcome, domain, at);
