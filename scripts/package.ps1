<#
.SYNOPSIS
  릴리스 빌드를 만들고 배포용 zip(휴대용)을 dist\ 에 만든다.
.DESCRIPTION
  ldf-studio.exe(GUI)와 ldf-cli.exe(명령줄), 문서(README, 검사 규칙 편차, JSON 스키마)를 한 폴더에 담아 zip으로 묶는다.
  -Sign 에 코드 서명 인증서(.pfx) 경로를 주면 signtool로 실행 파일에 서명한다(인증서 비밀번호는 환경 변수 LDF_SIGN_PASSWORD).
  인증서가 없으면 서명 없이 만든다(PRD 오픈 이슈: 서명 인증서).
.EXAMPLE
  scripts\package.ps1
  scripts\package.ps1 -Sign C:\keys\vendor.pfx
#>
param(
  [string]$Sign = ""
)
$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$name = "ldf-studio-$version-windows-x64"
$dist = Join-Path (Get-Location) "dist"
$stage = Join-Path $dist $name

cargo build --release --locked -p ldf-studio -p ldf-cli
if ($LASTEXITCODE -ne 0) { throw "빌드 실패" }

if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item target\release\ldf-studio.exe, target\release\ldf-cli.exe $stage
Copy-Item README.md $stage
New-Item -ItemType Directory -Force (Join-Path $stage "docs") | Out-Null
Copy-Item docs\check-deviations.md, docs\model.schema.json (Join-Path $stage "docs")

if ($Sign) {
  $signtool = (Get-Command signtool.exe -ErrorAction SilentlyContinue).Source
  if (-not $signtool) { throw "signtool.exe를 찾을 수 없다(Windows SDK 필요)" }
  foreach ($exe in "ldf-studio.exe", "ldf-cli.exe") {
    & $signtool sign /fd SHA256 /f $Sign /p $env:LDF_SIGN_PASSWORD /tr http://timestamp.digicert.com /td SHA256 (Join-Path $stage $exe)
    if ($LASTEXITCODE -ne 0) { throw "서명 실패: $exe" }
  }
}

$zip = Join-Path $dist "$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path $stage -DestinationPath $zip
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$hash  $name.zip" | Set-Content -Encoding ascii (Join-Path $dist "$name.zip.sha256")
"{0}  ({1:N1} MB)  sha256 {2}" -f $zip, ((Get-Item $zip).Length / 1MB), $hash
