# .mcpb 번들을 만든다: cargo 릴리스 빌드 -> manifest 버전 채움 -> mcpb pack
# 사용: powershell -File mcpb/pack.ps1 [-SkipBuild]
param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$ver = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $SkipBuild) { cargo build --release; if ($LASTEXITCODE) { throw 'cargo build 실패' } }
$stage = Join-Path $root 'out\mcpb-stage'
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Path "$stage\server" | Out-Null
Copy-Item target\release\akashic.exe "$stage\server\akashic.exe"
$m = (Get-Content mcpb\manifest.template.json -Raw -Encoding UTF8).Replace('__VERSION__', $ver)
[IO.File]::WriteAllText("$stage\manifest.json", $m, (New-Object Text.UTF8Encoding $false))
$outFile = Join-Path $root "out\akashic-$ver.mcpb"
npx -y @anthropic-ai/mcpb pack $stage $outFile
if ($LASTEXITCODE) { throw 'mcpb pack 실패' }
Write-Host "만듦: $outFile"
