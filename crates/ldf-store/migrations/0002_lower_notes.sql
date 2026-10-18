-- 0002: 변환(lowering) 중 모델에 담지 못한 사실(검사 규칙 0021·0107용)을 리비전에 JSON으로 보관한다.
-- 이전에 저장된 리비전은 빈 배열이 된다.
ALTER TABLE ldf_revision ADD COLUMN lower_notes TEXT NOT NULL DEFAULT '[]';
