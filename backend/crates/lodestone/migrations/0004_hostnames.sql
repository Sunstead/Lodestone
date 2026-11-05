-- Address names: a server can be reached as <hostname>.<public address>
-- through an SRV record. One name per server, and no two servers share one
-- (whoever owns them, since DNS is host-wide). NULLs don't collide.
ALTER TABLE instances ADD COLUMN hostname TEXT;
CREATE UNIQUE INDEX instances_hostname ON instances(hostname);
