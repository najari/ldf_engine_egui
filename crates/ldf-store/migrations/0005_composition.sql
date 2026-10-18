-- 0005: 노드 구성(composite/Node_composition)을 JSON으로 저장한다. 이전 리비전은 NULL이고, 그 구성은 원문 보관 섹션에 있다.
ALTER TABLE ldf_revision ADD COLUMN composition_json TEXT;
