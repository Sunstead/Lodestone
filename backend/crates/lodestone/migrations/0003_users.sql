-- People, signed in through the identity provider (Authentik). Each owns
-- their worlds, profiles and instances; admins (by provider group) also
-- manage Lodestone's settings and can open anyone's assets.
CREATE TABLE users (
    id                TEXT PRIMARY KEY,
    issuer            TEXT NOT NULL,
    subject           TEXT NOT NULL,
    username          TEXT NOT NULL,
    display_name      TEXT NOT NULL,
    admin             INTEGER NOT NULL DEFAULT 0,
    eula_accepted_at  TEXT,
    created_at        TEXT NOT NULL,
    last_seen_at      TEXT NOT NULL,
    UNIQUE (issuer, subject)
);

-- Browser sessions. The cookie holds a random token; only its hash is kept.
CREATE TABLE sessions (
    token_hash  TEXT PRIMARY KEY,
    user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at  INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL
);
CREATE INDEX sessions_user ON sessions(user_id);

-- Sign-ins in progress (state, PKCE verifier, nonce), for ten minutes.
CREATE TABLE oidc_flows (
    state       TEXT PRIMARY KEY,
    data        TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);

-- NULL owner: made before accounts existed; the first admin to sign in
-- claims them.
ALTER TABLE worlds ADD COLUMN owner_id TEXT REFERENCES users(id);
ALTER TABLE profiles ADD COLUMN owner_id TEXT REFERENCES users(id);
ALTER TABLE instances ADD COLUMN owner_id TEXT REFERENCES users(id);
CREATE INDEX worlds_owner ON worlds(owner_id);
CREATE INDEX profiles_owner ON profiles(owner_id);
CREATE INDEX instances_owner ON instances(owner_id);
