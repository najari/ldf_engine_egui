# LDF Studio

LIN Description File(LDF)을 열어 보고, 검사하고, 고치고, 버전별로 보관하는 Windows 데스크톱 도구.
Rust로 만들고, UI는 [egui](https://github.com/emilk/egui), 저장소는 SQLite([rusqlite](https://github.com/rusqlite/rusqlite))를 쓴다.

> 상태: **M1(읽기 전용 뷰어) 완료**, M2(검사기) 진행 중. 마일스톤은 [PRD](docs/prd.md)를 따른다.

| 마일스톤 | 상태 |
| --- | --- |
| M1 읽기 전용 뷰어 (WP1–4) | 완료 — Vector Clean 10개 임포트·표시, 132개 파일 DB 왕복, UI 1.5 ms/프레임 |
| M2 검사기 (WP5–7) | 진행 중 — Checker 창·`ldf-cli check`·엔진 연동 완료, 규칙 00xx–04xx와 코퍼스 하네스 구현 중 |
| M3 편집·내보내기 (WP8–11) | 예정 |
| M4 리비전·배포 (WP12–14) | 예정 (리비전 저장·열기는 M1에서 이미 동작) |

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

## 실행

```powershell
cargo run -p ldf-studio -- 파일.ldf      # GUI (인자 없이도 실행, 파일은 창에 끌어다 놓아도 된다)
cargo run -p ldf-cli -- check 파일.ldf    # 배치 검사 (종료 코드 0/1/2/3)
```

환경 변수: `LDF_STUDIO_PROJECT`(프로젝트 DB 위치, 기본 `%LOCALAPPDATA%\LDFStudio\default.ldfdb`), `LDF_STUDIO_PERF=1`(상태 바에 프레임당 CPU 시간).

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
