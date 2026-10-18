-- LDF Studio 프로젝트 DB 스키마 v1.
--
-- 리비전 하나 = LDF 파일 하나의 스냅샷. 모든 엔티티 행은 revision_id 로 리비전에 속하고,
-- (revision_id, idx) 복합 키의 idx 는 모델 arena 의 인덱스다. 그래서 저장 -> 복원이 손실 없이
-- 같은 모델을 만들고, 참조 무결성은 복합 외래키가 보장한다.
--
-- 참조 컬럼은 `*_idx`(해석됨) 또는 `*_name`(해석되지 않아 이름만 보관) 중 하나만 채운다.
-- 둘 다 NULL 이면 참조가 없다. 복합 외래키는 *_idx 가 NULL 이면 검사하지 않는다(MATCH SIMPLE).
-- 검사 규칙이 잡아야 하는 값(길이 범위, ID 범위 등)에는 CHECK 를 두지 않는다 —
-- 깨진 LDF도 손실 없이 저장하고 되돌려야 하기 때문이다.

CREATE TABLE project (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE ldf_revision (
  id                INTEGER PRIMARY KEY,
  project_id        INTEGER NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  parent_id         INTEGER REFERENCES ldf_revision(id) ON DELETE SET NULL,
  label             TEXT NOT NULL,
  source_name       TEXT,
  created_at        TEXT NOT NULL DEFAULT (datetime('now')),
  has_marker        INTEGER NOT NULL,
  protocol_ver      TEXT,
  language_ver      TEXT,
  speed_bps         INTEGER,
  channel           TEXT,
  ldf_file_revision TEXT,
  big_endian        INTEGER NOT NULL DEFAULT 0,
  encoding          TEXT NOT NULL DEFAULT 'utf-8',
  source_hash       INTEGER NOT NULL,       -- FNV-1a 64 (i64 로 비트 변환). 동일 파일 재임포트 감지용
  source_text       BLOB NOT NULL,          -- 원본 바이트 전체
  section_order     TEXT NOT NULL           -- JSON 배열: 최상위 문장 순서
);
CREATE INDEX ldf_revision_project ON ldf_revision(project_id, id);
CREATE INDEX ldf_revision_hash    ON ldf_revision(project_id, source_hash);

CREATE TABLE node (
  revision_id            INTEGER NOT NULL REFERENCES ldf_revision(id) ON DELETE CASCADE,
  idx                    INTEGER NOT NULL,
  name                   TEXT NOT NULL,
  role                   TEXT NOT NULL CHECK (role IN ('master', 'slave')),
  timebase_ms            REAL,
  jitter_ms              REAL,
  max_header_bits        INTEGER,
  response_tolerance_pct REAL,
  src_start              INTEGER,
  src_end                INTEGER,
  PRIMARY KEY (revision_id, idx)
) WITHOUT ROWID;
CREATE INDEX node_name ON node(revision_id, name);

CREATE TABLE encoding (
  revision_id INTEGER NOT NULL REFERENCES ldf_revision(id) ON DELETE CASCADE,
  idx         INTEGER NOT NULL,
  name        TEXT NOT NULL,
  src_start   INTEGER,
  src_end     INTEGER,
  PRIMARY KEY (revision_id, idx)
) WITHOUT ROWID;
CREATE INDEX encoding_name ON encoding(revision_id, name);

CREATE TABLE encoding_entry (
  revision_id   INTEGER NOT NULL,
  encoding_idx  INTEGER NOT NULL,
  pos           INTEGER NOT NULL,
  kind          TEXT NOT NULL CHECK (kind IN ('logical', 'physical', 'bcd', 'ascii')),
  logical_value INTEGER,                    -- u64 를 i64 로 비트 변환
  text          TEXT,
  min_raw       INTEGER,
  max_raw       INTEGER,
  scale         REAL,
  offset        REAL,
  unit          TEXT,
  PRIMARY KEY (revision_id, encoding_idx, pos),
  FOREIGN KEY (revision_id, encoding_idx) REFERENCES encoding(revision_id, idx) ON DELETE CASCADE
) WITHOUT ROWID;

CREATE TABLE signal (
  revision_id    INTEGER NOT NULL REFERENCES ldf_revision(id) ON DELETE CASCADE,
  idx            INTEGER NOT NULL,
  name           TEXT NOT NULL,
  size_bits      INTEGER NOT NULL,
  init_scalar    INTEGER,                   -- u64 를 i64 로 비트 변환
  init_bytes     BLOB,                      -- 배열 초기값. init_scalar 와 둘 중 하나만 채운다
  kind           TEXT NOT NULL CHECK (kind IN ('normal', 'diagnostic')),
  publisher_idx  INTEGER,
  publisher_name TEXT,
  src_start      INTEGER,
  src_end        INTEGER,
  PRIMARY KEY (revision_id, idx),
  FOREIGN KEY (revision_id, publisher_idx) REFERENCES node(revision_id, idx)
) WITHOUT ROWID;
CREATE INDEX signal_name ON signal(revision_id, name);

CREATE TABLE signal_subscriber (
  revision_id INTEGER NOT NULL,
  signal_idx  INTEGER NOT NULL,
  pos         INTEGER NOT NULL,
  node_idx    INTEGER,
  node_name   TEXT,
  PRIMARY KEY (revision_id, signal_idx, pos),
  FOREIGN KEY (revision_id, signal_idx) REFERENCES signal(revision_id, idx) ON DELETE CASCADE,
  FOREIGN KEY (revision_id, node_idx)   REFERENCES node(revision_id, idx)
) WITHOUT ROWID;

CREATE TABLE signal_encoding (
  revision_id INTEGER NOT NULL,
  signal_idx  INTEGER NOT NULL,
  pos         INTEGER NOT NULL,
  enc_idx     INTEGER,
  enc_name    TEXT,
  PRIMARY KEY (revision_id, signal_idx, pos),
  FOREIGN KEY (revision_id, signal_idx) REFERENCES signal(revision_id, idx) ON DELETE CASCADE,
  FOREIGN KEY (revision_id, enc_idx)    REFERENCES encoding(revision_id, idx)
) WITHOUT ROWID;

CREATE TABLE schedule (
  revision_id INTEGER NOT NULL REFERENCES ldf_revision(id) ON DELETE CASCADE,
  idx         INTEGER NOT NULL,
  name        TEXT NOT NULL,
  src_start   INTEGER,
  src_end     INTEGER,
  PRIMARY KEY (revision_id, idx)
) WITHOUT ROWID;
CREATE INDEX schedule_name ON schedule(revision_id, name);

CREATE TABLE frame (
  revision_id    INTEGER NOT NULL REFERENCES ldf_revision(id) ON DELETE CASCADE,
  idx            INTEGER NOT NULL,
  name           TEXT NOT NULL,
  kind           TEXT NOT NULL CHECK (kind IN ('unconditional', 'sporadic', 'event_triggered', 'diagnostic')),
  lin_id         INTEGER,
  size_bytes     INTEGER,
  publisher_idx  INTEGER,
  publisher_name TEXT,
  collision_idx  INTEGER,
  collision_name TEXT,
  src_start      INTEGER,
  src_end        INTEGER,
  PRIMARY KEY (revision_id, idx),
  FOREIGN KEY (revision_id, publisher_idx) REFERENCES node(revision_id, idx),
  FOREIGN KEY (revision_id, collision_idx) REFERENCES schedule(revision_id, idx)
) WITHOUT ROWID;
CREATE INDEX frame_name ON frame(revision_id, name);

CREATE TABLE frame_signal (
  revision_id INTEGER NOT NULL,
  frame_idx   INTEGER NOT NULL,
  pos         INTEGER NOT NULL,
  signal_idx  INTEGER,
  signal_name TEXT,
  bit_offset  INTEGER NOT NULL,
  PRIMARY KEY (revision_id, frame_idx, pos),
  FOREIGN KEY (revision_id, frame_idx)  REFERENCES frame(revision_id, idx) ON DELETE CASCADE,
  FOREIGN KEY (revision_id, signal_idx) REFERENCES signal(revision_id, idx)
) WITHOUT ROWID;

-- 스포라딕·이벤트 트리거 프레임의 연관 프레임
CREATE TABLE frame_assoc (
  revision_id INTEGER NOT NULL,
  frame_idx   INTEGER NOT NULL,
  pos         INTEGER NOT NULL,
  assoc_idx   INTEGER,
  assoc_name  TEXT,
  PRIMARY KEY (revision_id, frame_idx, pos),
  FOREIGN KEY (revision_id, frame_idx) REFERENCES frame(revision_id, idx) ON DELETE CASCADE,
  FOREIGN KEY (revision_id, assoc_idx) REFERENCES frame(revision_id, idx)
) WITHOUT ROWID;

CREATE TABLE schedule_slot (
  revision_id  INTEGER NOT NULL,
  schedule_idx INTEGER NOT NULL,
  pos          INTEGER NOT NULL,
  delay_ms     REAL NOT NULL,
  kind         TEXT NOT NULL CHECK (kind IN ('frame', 'command')),
  frame_idx    INTEGER,
  frame_name   TEXT,
  entry_json   TEXT,                        -- kind='command' 일 때 SlotEntry JSON
  src_start    INTEGER,
  src_end      INTEGER,
  PRIMARY KEY (revision_id, schedule_idx, pos),
  FOREIGN KEY (revision_id, schedule_idx) REFERENCES schedule(revision_id, idx) ON DELETE CASCADE,
  FOREIGN KEY (revision_id, frame_idx)    REFERENCES frame(revision_id, idx)
) WITHOUT ROWID;

CREATE TABLE node_attr (
  revision_id      INTEGER NOT NULL REFERENCES ldf_revision(id) ON DELETE CASCADE,
  idx              INTEGER NOT NULL,
  node_idx         INTEGER,
  node_name        TEXT,
  lin_protocol     TEXT,
  configured_nad   INTEGER,
  initial_nad      INTEGER,
  product_supplier INTEGER,
  product_function INTEGER,
  product_variant  INTEGER,
  resp_err_idx     INTEGER,
  resp_err_name    TEXT,
  p2_min_ms        REAL,
  st_min_ms        REAL,
  n_as_ms          REAL,
  n_cr_ms          REAL,
  extra_json       TEXT NOT NULL,           -- 해석하지 않은 속성 JSON 배열
  src_start        INTEGER,
  src_end          INTEGER,
  PRIMARY KEY (revision_id, idx),
  FOREIGN KEY (revision_id, node_idx)     REFERENCES node(revision_id, idx),
  FOREIGN KEY (revision_id, resp_err_idx) REFERENCES signal(revision_id, idx)
) WITHOUT ROWID;

CREATE TABLE node_attr_fault_signal (
  revision_id INTEGER NOT NULL,
  attr_idx    INTEGER NOT NULL,
  pos         INTEGER NOT NULL,
  signal_idx  INTEGER,
  signal_name TEXT,
  PRIMARY KEY (revision_id, attr_idx, pos),
  FOREIGN KEY (revision_id, attr_idx)   REFERENCES node_attr(revision_id, idx) ON DELETE CASCADE,
  FOREIGN KEY (revision_id, signal_idx) REFERENCES signal(revision_id, idx)
) WITHOUT ROWID;

CREATE TABLE node_config_frame (
  revision_id INTEGER NOT NULL,
  attr_idx    INTEGER NOT NULL,
  pos         INTEGER NOT NULL,
  frame_idx   INTEGER,
  frame_name  TEXT,
  message_id  INTEGER,
  PRIMARY KEY (revision_id, attr_idx, pos),
  FOREIGN KEY (revision_id, attr_idx)  REFERENCES node_attr(revision_id, idx) ON DELETE CASCADE,
  FOREIGN KEY (revision_id, frame_idx) REFERENCES frame(revision_id, idx)
) WITHOUT ROWID;

-- 모델링하지 않는 섹션(왕복 보존용)
CREATE TABLE raw_section (
  revision_id INTEGER NOT NULL REFERENCES ldf_revision(id) ON DELETE CASCADE,
  pos         INTEGER NOT NULL,
  name        TEXT NOT NULL,
  text        TEXT NOT NULL,
  src_start   INTEGER,
  src_end     INTEGER,
  PRIMARY KEY (revision_id, pos)
) WITHOUT ROWID;

-- 진단 캐시. 프로젝트를 다시 열 때 재검사 없이 상태 바 경고를 바로 보여준다.
CREATE TABLE diagnostic (
  revision_id INTEGER NOT NULL REFERENCES ldf_revision(id) ON DELETE CASCADE,
  pos         INTEGER NOT NULL,
  code        INTEGER NOT NULL,
  severity    TEXT NOT NULL CHECK (severity IN ('info', 'warning', 'error')),
  src_start   INTEGER,
  src_end     INTEGER,
  message     TEXT NOT NULL,
  PRIMARY KEY (revision_id, pos)
) WITHOUT ROWID;
