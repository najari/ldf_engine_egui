# LDF Studio

LIN Description File(LDF)을 열어 보고, 검사하고, 고치고, 버전별로 보관하는 Windows 데스크톱 도구.
Rust로 만들고, UI는 [egui](https://github.com/emilk/egui), 저장소는 SQLite([rusqlite](https://github.com/rusqlite/rusqlite))를 쓴다.

> 상태: **M1~M4 구현 완료**(패키징·서명 제외 마무리 항목은 아래 표). 마일스톤은 [PRD](docs/prd.md)를 따른다.

| 마일스톤 | 상태 |
| --- | --- |
| M1 읽기 전용 뷰어 (WP1–4) | 완료 — Vector Clean 10개 임포트·표시, 132개 파일 DB 왕복, UI 1.5 ms/프레임 |
| M2 검사기 (WP5–7) | 완료 — 규칙 00xx–04xx 구현, Vector TestFiles 125/131(95.4%) 일치, Clean 10/10 진단 0건. 차이는 [docs/check-deviations.md](docs/check-deviations.md) |
| M3 편집·내보내기 (WP8–11) | 완료 — 작성기(ldf-writer)·내보내기, 편집 연산 28종과 Undo/Redo, 편집 대화상자, 규칙 설정, JSON 내보내기, 속성 기반 시험 10,000회 통과. 퍼징은 골격만(아래 알려진 한계) |
| M4 리비전·배포 (WP12–14) | 완료 — 리비전 트리·Diff·삭제, ldf-cli diff/convert/import/export, 휴대용 zip 패키징. 코드 서명은 인증서 대기 |

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
  ldf-engine/   파사드, 명령/이벤트, 편집·Undo, 내보내기, diff 보고서
apps/
  ldf-studio/   eframe(egui) 앱
  ldf-cli/      배치 검사·비교·변환, 프로젝트 파일 조작
```

## 실행

```powershell
cargo run -p ldf-studio -- 파일.ldf      # GUI (인자 없이도 실행, 파일은 창에 끌어다 놓아도 된다)
cargo run -p ldf-cli -- check 파일.ldf    # 배치 검사 (종료 코드 0/1/2/3)
```

### 명령줄 (ldf-cli)

```powershell
ldf-cli check   a.ldf b.ldf [--format text|json]                     # 진단. 종료 코드 0 문제 없음 / 1 경고 / 2 오류 / 3 실패
ldf-cli diff    이전.ldf 이후.ldf [--format text|json|csv|html] [-o 파일]  # 종료 코드 0 차이 없음 / 1 차이 있음 / 3 실패
ldf-cli convert 입력.ldf -o 출력.ldf --to 2.0 [--eol crlf] [--encoding windows-1252]   # 빠지는 항목은 stderr 경고
ldf-cli convert 입력.ldf -o 모델.json --format json                  # 모델 JSON(docs/model.schema.json)
ldf-cli import  프로젝트.ldfdb a.ldf b.ldf                           # 파일마다 리비전을 만든다
ldf-cli list    프로젝트.ldfdb
ldf-cli export  프로젝트.ldfdb [--revision N] -o 출력.ldf
```

### 편집과 리비전

- 속성 창의 값(프레임 이름·ID·크기, 시그널 이름·길이·초기값, 노드 이름·타이밍, 슬롯 delay)은 더블클릭해 바로 고친다. 범위 밖이면 빨갛게 되고 사유가 보이며 적용되지 않는다(Enter 적용, Esc 취소).
- 내비게이션에서 항목을 더블클릭(또는 Enter)하면 편집 대화상자, Delete는 삭제 확인(함께 지워지는 항목을 미리 보여 준다). **편집 → 새로 만들기**로 프레임·시그널·노드·스케줄·인코딩을 만든다.
- Ctrl+Z / Ctrl+Y로 되돌리고, Ctrl+S는 편집 내용을 **새 리비전**으로 저장한다(라벨을 묻는다). 저장하지 않은 변경은 제목에 `*`로 보이고, 닫기·다른 파일 열기 전에 확인한다.
- **파일 → 리비전·비교**: 부모–자식 트리, 두 리비전의 Diff(CSV·HTML 저장), 삭제.
- 편집하지 않고 내보내면 원문을 바이트 그대로 내보낸다. 편집한 파일은 정규 서식으로 다시 쓰며 파일 머리말 주석만 남는다.

### 배포 (휴대용 zip)

```powershell
scripts\package.ps1                      # dist\ldf-studio-<버전>-windows-x64.zip (+ .sha256)
scripts\package.ps1 -Sign 인증서.pfx     # signtool로 서명(비밀번호는 환경 변수 LDF_SIGN_PASSWORD)
```

태그(`v*`)를 밀면 .github/workflows/release.yml이 같은 zip을 만들어 릴리스에 올린다.
```

환경 변수: `LDF_STUDIO_PROJECT`(프로젝트 DB 위치, 기본 `%LOCALAPPDATA%\LDFStudio\default.ldfdb`), `LDF_STUDIO_PERF=1`(상태 바에 프레임당 CPU 시간), `LDF_STUDIO_LOG_DIR`(로그 폴더, 기본 `%LOCALAPPDATA%\LDFStudio\logs`), `LDF_STUDIO_RENDERER=glow|wgpu`(렌더러 하나만 고르기).

렌더러는 OpenGL(glow)을 먼저 쓰고, 시작에 실패하면(그래픽 드라이버가 없는 가상 머신 등) wgpu로 다시 시도한다. 측정(릴리스, 1 MB LDF): glow 작업 집합 약 207 MB, wgpu 약 455 MB, 콜드 스타트 1초 미만.

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
- 속성 기반 시험(PRD G3): `PROPTEST_CASES=10000 cargo test --release -p ldf-engine --test properties` (임의 편집 열의 왕복·고정점, Undo 대칭, 망가뜨린 입력의 무패닉).
- 편집 후 재검사 속도(FR-CHK-09): `cargo test --release -p ldf-engine --test edit -- --ignored --nocapture`.
- 퍼징: `fuzz/`에 cargo-fuzz 대상(parse, roundtrip)이 있다. nightly와 `cargo install cargo-fuzz`가 필요하고, 이 저장소에서는 아직 실행해 보지 못했다.

## 알려진 한계

- 편집한 파일은 정규 서식으로 다시 쓰므로 본문 주석과 원래 서식이 사라진다(파일 머리말 주석만 유지). 편집하지 않은 내보내기는 원문 그대로다.
- 스케줄 슬롯을 내비게이션에서 끌어다 놓는 편집(FR-EDT-05)은 없고, 스케줄 대화상자(↑↓·삭제·추가)로 대신했다.
- 검사 규칙은 Vector TestFiles에서 의미를 추론해 구현했고 131개 중 125개가 기대와 맞는다. 맞지 못한 6개와 사유는 [docs/check-deviations.md](docs/check-deviations.md).
- 코드 서명, 설치 관리자(MSIX/MSI), `.ldfdb` 파일 연결은 아직 없다.
- cargo-fuzz 1시간 실행은 하지 못했다(골격과 수동·주간 workflow만 있다).
