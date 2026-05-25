CREATE TABLE IF NOT EXISTS similar_groups (
  id         TEXT PRIMARY KEY,
  method     TEXT NOT NULL,            -- M1 fixed 'phash_burst'
  params     TEXT NOT NULL             -- JSON, e.g. {"threshold":8,"version":1}
);

CREATE TABLE IF NOT EXISTS group_members (
  group_id TEXT NOT NULL REFERENCES similar_groups(id) ON DELETE CASCADE,
  photo_id TEXT NOT NULL REFERENCES photos(id)         ON DELETE CASCADE,
  PRIMARY KEY (group_id, photo_id)
);

CREATE INDEX IF NOT EXISTS idx_group_members_photo   ON group_members(photo_id);
CREATE INDEX IF NOT EXISTS idx_similar_groups_method ON similar_groups(method);
