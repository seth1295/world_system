$ErrorActionPreference = "Stop"

$cargo = (Get-Command cargo -ErrorAction SilentlyContinue).Source
if (-not $cargo) {
    $cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
}
$repoRoot = Split-Path -Parent $PSScriptRoot
$fixtureDir = Join-Path (Join-Path $repoRoot "tests") "lint_fixtures"
$manifest = Join-Path $fixtureDir "Cargo.toml"
$previousPreference = $ErrorActionPreference
$ErrorActionPreference = "Continue"
$output = & $cargo clippy --locked --manifest-path $manifest -- -D warnings 2>&1
$exitCode = $LASTEXITCODE
$ErrorActionPreference = $previousPreference
$text = $output | Out-String
if ($exitCode -eq 0) {
    throw "Clippy accepted the deliberate nondeterminism fixture."
}
if ($text -notmatch "disallowed method" -or $text -notmatch "disallowed type") {
    throw "Clippy failed for an unrelated reason:`n$text"
}
Write-Output "PASS: Clippy rejected the disallowed float method and unordered collection fixture."
