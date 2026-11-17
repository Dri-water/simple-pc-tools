# Regenerates the README screenshots in docs/screenshots/ from the demo data.
#
#   powershell -ExecutionPolicy Bypass -File scripts\screenshots.ps1
#
# Starts a development copy (no admin rights, its own data folder and ports,
# so it can't touch a real install), photographs /?demo with headless Edge,
# then quits the copy. Run it after any visible UI change.

$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$out = Join-Path $repo 'docs\screenshots'
$edge = "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe"
$port = 7903
$instance = 47697
New-Item -ItemType Directory -Force $out | Out-Null

Push-Location $repo
try {
  $ErrorActionPreference = 'Continue'
  cargo build --quiet 2>&1 | ForEach-Object { "$_" }
  $ErrorActionPreference = 'Stop'
  $exe = Join-Path $repo 'target\debug\simple-pc-tools.exe'
  Start-Process $exe -ArgumentList '--no-elevate', '--background', '--dev-server', "127.0.0.1:$port",
    '--instance-port', $instance, '--data-dir', 'target\screenshot-data' -WindowStyle Hidden
  Start-Sleep 3

  # name, query, hash, width, height
  $shots = @(
    @('apps-light', 'noupdate&theme=light', 'apps', 1280, 900),
    @('apps-dark', 'noupdate&theme=dark', 'apps', 1280, 900),
    @('app-editor', 'noupdate&theme=light&shot=editor', 'apps', 1280, 900),
    @('disk-usage', 'noupdate&theme=light&shot=disk', 'disk', 1280, 900),
    @('disk-usage-dark', 'noupdate&theme=dark&shot=disk', 'disk', 1280, 900),
    @('resources', 'noupdate&theme=light&shot=chart', 'resources', 1280, 1000),
    @('resources-dark', 'noupdate&theme=dark&shot=chart', 'resources', 1280, 1000),
    @('resources-port', 'noupdate&theme=light&shot=port', 'resources', 1280, 1720),
    @('settings', 'noupdate&theme=light', 'settings', 1280, 1000),
    @('update-prompt', 'theme=light', 'apps', 1280, 520),
    @('narrow', 'noupdate&theme=light', 'apps', 520, 1000)
  )
  $ErrorActionPreference = 'Continue'   # Edge reports progress on stderr
  foreach ($s in $shots) {
    $file = Join-Path $out "$($s[0]).png"
    $url = "http://127.0.0.1:$port/?demo&$($s[1])#$($s[2])"
    & $edge --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=1.5 `
      --user-data-dir="$repo\target\screenshot-edge" "--window-size=$($s[3]),$($s[4])" `
      --virtual-time-budget=8000 "--screenshot=$file" $url 2>&1 | Out-Null
    "wrote $file"
  }
} finally {
  & (Join-Path $repo 'target\debug\simple-pc-tools.exe') --quit --instance-port $instance 2>$null
  Pop-Location
}
