-- 0007: 시그널 그룹(Signal_groups)을 JSON으로 저장한다. 이전 리비전은 NULL이고, 그 그룹은 원문 보관 섹션에 있다.
ALTER TABLE ldf_revision ADD COLUMN signal_groups_json TEXT;
