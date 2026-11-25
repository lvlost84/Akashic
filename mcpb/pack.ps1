# .mcpb 번들을 만든다: (cargo 릴리스 빌드) -> manifest 버전 채움 -> mcpb pack
# 사용: powershell -File mcpb/pack.ps1 [-SkipBuild]
# 번들 버전은 앱 버전(Cargo.toml)이 아니라 mcpb/version.txt다. MCP 도구(src/mcp.rs, MCP가 쓰는 db 함수, manifest)가 바뀔 때만 올린다.
# 릴리스 때 같은 버전의 번들이 이미 올라가 있으면 그 파일을 그대로 첨부하므로(.github/workflows/release.yml), 도구가 안 바뀌면 번들은 바뀌지 않는다.
param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$ver = (Get-Content mcpb/version.txt -Raw).Trim()
if ($ver -notmatch '^\d+\.\d+\.\d+$') { throw "mcpb/version.txt 형식이 잘못됨: $ver" }
if (-not $SkipBuild) { cargo build --release; if ($LASTEXITCODE) { throw 'cargo build 실패' } }
$stage = Join-Path $root 'out\mcpb-stage'
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Path "$stage\server" | Out-Null
Copy-Item target\release\akashic.exe "$stage\server\akashic.exe"
$m = (Get-Content mcpb\manifest.template.json -Raw -Encoding UTF8).Replace('__VERSION__', $ver)
[IO.File]::WriteAllText("$stage\manifest.json", $m, (New-Object Text.UTF8Encoding $false))
New-Item -ItemType Directory -Force (Join-Path $root 'out') | Out-Null
$outFile = Join-Path $root "out\akashic-$ver.mcpb"
# 버전을 고정해 두어, CI가 어느 날 갑자기 다른 도구를 받아 실행하지 않게 한다.
npx -y @anthropic-ai/mcpb@2.1.2 pack $stage $outFile
if ($LASTEXITCODE) { throw 'mcpb pack 실패' }
Write-Host "만듦: $outFile"
