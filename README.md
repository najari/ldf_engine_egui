# LDF Studio

LIN Description File(LDF)을 열어 보고, 검사하고, 고치고, 버전별로 보관하는 Windows 데스크톱 도구.
Rust로 만들고, UI는 [egui](https://github.com/emilk/egui), 저장소는 SQLite([rusqlite](https://github.com/rusqlite/rusqlite))를 쓴다.

> 상태: 개발 초기. 설계와 요구사항이 확정되어 있고 구현은 PRD의 마일스톤(M1–M4) 순서로 진행한다.

## 문서

| 문서 | 내용 |
| --- | --- |
| [docs/prd.md](docs/prd.md) | 개발지시서(PRD): 목표, 요구사항(FR/NFR), 작업 패키지(WP1–14), 마일스톤과 인수 기준 |
| [docs/design.md](docs/design.md) | 설계서: Rust 오픈소스 조사, 아키텍처, 파서, SQLite 스키마, egui UI, 흐름, 테스트 |

요구사항과 설계서가 충돌하면 PRD가 우선한다.

## 구조

```
crates/
  ldf-model/    도메인 타입, 이름 해석, PID·체크섬   (UI·DB 의존 금지)
  ldf-syntax/   토크나이저, winnow 파서, 스팬 포함 AST
  ldf-check/    검사 규칙 레지스트리, Diagnostic
  ldf-writer/   모델 → LDF 텍스트
  ldf-store/    rusqlite 저장소, 마이그레이션
  ldf-engine/   파사드, 명령/이벤트, Undo, diff
apps/
  ldf-studio/   eframe(egui) 앱
  ldf-cli/      배치 검사·변환
```

## 개발

```powershell
cargo build --locked
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

- Rust stable(`rust-toolchain.toml`로 고정), egui/eframe 0.36, rusqlite 0.40(`bundled`).
- Vector LDF Explorer의 테스트 LDF는 상용 제품 동봉물이라 저장소에 넣지 않는다. 로컬에서만 `LDF_VECTOR_TESTFILES` 환경 변수로 경로를 지정해 코퍼스 테스트를 돌린다.
- 고객 LDF와 `*.ldfdb` 파일은 커밋하지 않는다.
