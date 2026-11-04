-- Assets are stored as JSON documents plus the columns that carry
-- relational rules. The rules that matter live in the schema:
--   * a world is attached to at most one instance (UNIQUE world_id)
--   * a world or profile cannot be deleted while an instance uses it

CREATE TABLE worlds (
    id          TEXT PRIMARY KEY,
    data        TEXT NOT NULL,
    created_at  TEXT NOT NULL
);

CREATE TABLE profiles (
    id          TEXT PRIMARY KEY,
    data        TEXT NOT NULL,
    created_at  TEXT NOT NULL
);

CREATE TABLE instances (
    id          TEXT PRIMARY KEY,
    profile_id  TEXT NOT NULL REFERENCES profiles(id) ON DELETE RESTRICT,
    world_id    TEXT NOT NULL UNIQUE REFERENCES worlds(id) ON DELETE RESTRICT,
    data        TEXT NOT NULL,
    created_at  TEXT NOT NULL
);

CREATE TABLE backups (
    id          TEXT PRIMARY KEY,
    world_id    TEXT NOT NULL REFERENCES worlds(id) ON DELETE CASCADE,
    data        TEXT NOT NULL,
    created_at  TEXT NOT NULL
);
CREATE INDEX backups_world ON backups(world_id, created_at);

CREATE TABLE backup_schedules (
    world_id    TEXT PRIMARY KEY REFERENCES worlds(id) ON DELETE CASCADE,
    data        TEXT NOT NULL
);

-- Everyone who has played a world, across every instance that ran it.
CREATE TABLE world_players (
    world_id          TEXT NOT NULL REFERENCES worlds(id) ON DELETE CASCADE,
    name              TEXT NOT NULL,
    uuid              TEXT,
    first_seen        TEXT NOT NULL,
    last_seen         TEXT NOT NULL,
    playtime_seconds  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (world_id, name)
);

CREATE TABLE jobs (
    id          TEXT PRIMARY KEY,
    data        TEXT NOT NULL,
    created_at  TEXT NOT NULL
);

CREATE TABLE settings (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL
);

-- Provider responses (version manifests, search results) with a fetch time.
CREATE TABLE http_cache (
    key         TEXT PRIMARY KEY,
    body        TEXT NOT NULL,
    fetched_at  INTEGER NOT NULL
);
