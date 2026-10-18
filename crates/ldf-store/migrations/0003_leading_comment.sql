-- 0003: 파일 머리말 주석(첫 문장 앞의 주석 블록). 이전에 저장된 리비전은 빈 문자열이다.
ALTER TABLE ldf_revision ADD COLUMN leading_comment TEXT NOT NULL DEFAULT '';
