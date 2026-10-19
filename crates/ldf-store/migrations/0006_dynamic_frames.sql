-- 0006: 동적 프레임(dynamic_frames)을 JSON으로 저장한다. 이전 리비전은 NULL이고, 그 ID들은 원문 보관 섹션에 있다.
ALTER TABLE ldf_revision ADD COLUMN dynamic_frames_json TEXT;
