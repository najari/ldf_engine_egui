-- 0004: 규칙별 설정(FR-CHK-06). 프로젝트에 저장되므로 다른 PC에서 같은 .ldfdb를 열어도 유지된다. 기본값과 다른 규칙만 행으로 둔다.
CREATE TABLE rule_setting (
  project_id INTEGER NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  code       INTEGER NOT NULL,
  enabled    INTEGER NOT NULL DEFAULT 1,
  severity   TEXT,
  PRIMARY KEY (project_id, code)
);
