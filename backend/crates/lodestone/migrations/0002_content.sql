-- A profile's mods and plugins: its lockfile. Each row pins one file by
-- hash, so a profile is reproducible and its jars come from the blob cache.
CREATE TABLE profile_content (
    id          TEXT PRIMARY KEY,
    profile_id  TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    provider    TEXT NOT NULL,
    -- NULL for uploads that no provider recognised.
    project_id  TEXT,
    filename    TEXT NOT NULL,
    data        TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    UNIQUE (profile_id, provider, project_id),
    UNIQUE (profile_id, filename)
);
CREATE INDEX profile_content_profile ON profile_content(profile_id, created_at);
