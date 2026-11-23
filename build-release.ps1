# Builds BatteryHub and writes the release files (the only files users download) to release\:
#   release\BatteryHub-Taskbar-Setup-v1.0.exe
#   release\SHA256SUMS.txt
# cargo prints progress on stderr, so failures are detected through exit codes.
Set-Location $PSScriptRoot

# MSVC link.exe fails (LNK1104) when the build path gets close to MAX_PATH; build elsewhere then.
if (-not $env:CARGO_TARGET_DIR -and $PSScriptRoot.Length -gt 100) {
    $env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'batteryhub-target'
}

cargo build --release --bins
if ($LASTEXITCODE) { throw 'cargo build failed' }

$target = (cargo metadata --format-version 1 --no-deps | ConvertFrom-Json).target_directory
$release = Join-Path $target 'release'

$env:BH_PAYLOAD_DIR = $release
cargo build --release --manifest-path installer\Cargo.toml
if ($LASTEXITCODE) { throw 'setup build failed' }
# The setup is its own crate: without CARGO_TARGET_DIR it builds into installer\target.
$setupDir = Join-Path (cargo metadata --format-version 1 --no-deps --manifest-path installer\Cargo.toml | ConvertFrom-Json).target_directory 'release'

$version = (Get-Content VERSION -Raw).Trim()
$label = 'v' + ($version -replace '\.0$', '')
Remove-Item release -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory release | Out-Null
foreach ($e in 'Taskbar') {
    Copy-Item (Join-Path $setupDir "BatteryHub-$e-Setup.exe") "release\BatteryHub-$e-Setup-$label.exe" -ErrorAction Stop
}
Get-ChildItem release\*.exe | ForEach-Object { '{0}  {1}' -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name } |
    Set-Content release\SHA256SUMS.txt -Encoding ascii
Get-ChildItem release | ForEach-Object { '{0,-40} {1,8:N0} KB' -f $_.Name, ($_.Length / 1KB) }
