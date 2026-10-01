$ErrorActionPreference = "Continue"
$edge = "C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
$ff = "$env:LOCALAPPDATA\Microsoft\WinGet\Links\ffmpeg.exe"
$root = "C:\Users\C\Projects\ratline\assets"
$template = Get-Content -Raw "$root\demo-render.html"

function Get-SceneLines($demo) {
  if ($demo -eq "commands") {
    return @(
      @{ cls = "sys"; html = "demo · commands" },
      @{ cls = "sys"; html = "&gt; /connect" },
      @{ cls = "sys"; html = "code  <span class='val'>P-XY98-ZW76</span>" },
      @{ cls = "sys"; html = "copied" },
      @{ cls = "sys ok"; html = "line live" },
      @{ cls = "sys"; html = "you   <span class='val'>A3F1-9C2E</span>" },
      @{ cls = "sys"; html = "them  <span class='val'>7B44-D0A1</span>" },
      @{ cls = "sys"; html = "read aloud · then /verify" },
      @{ cls = "sys"; html = "&gt; /verify" },
      @{ cls = "sys ok"; html = "verified · out of band match" },
      @{ cls = ""; html = "&gt; ping" },
      @{ cls = ""; html = "&lt; pong" }
    )
  }
  return @(
    @{ cls = "sys ok"; html = "line live" },
    @{ cls = "sys"; html = "you   <span class='val'>A3F1-9C2E</span>" },
    @{ cls = "sys"; html = "them  <span class='val'>7B44-D0A1</span>" },
    @{ cls = "sys ok"; html = "verified" },
    @{ cls = ""; html = "&gt; line still dark?" },
    @{ cls = ""; html = "&lt; dark enough" },
    @{ cls = ""; html = "&gt; both of us here" },
    @{ cls = ""; html = "&lt; then we cut it" },
    @{ cls = ""; html = "&gt; hold" }
  )
}

function Get-Status($demo) {
  if ($demo -eq "commands") {
    return @{ id = "a1b2c3d4"; conv = "dm:wire · ok"; path = "DIRECT" }
  }
  return @{ id = "a1b2c3d4"; conv = "dm:wire · ok"; path = "DIRECT" }
}

function Write-FrameHtml($demo, $reveal, $path) {
  $lines = Get-SceneLines $demo
  $st = Get-Status $demo
  $shown = $lines | Select-Object -First $reveal
  $log = ($shown | ForEach-Object { "<div class=`"line $($_.cls)`">$($_.html)</div>" }) -join "`n"
  $html = @"
<!DOCTYPE html>
<html><head><meta charset="UTF-8" />
<style>
html,body{margin:0;padding:0;background:#030403;color:#9bc4a4;font:14px/1.5 Cascadia Mono,Consolas,monospace;width:720px;height:405px;overflow:hidden}
.app{height:100%;display:flex;flex-direction:column;box-sizing:border-box;background:#070908}
.status{display:flex;gap:16px;align-items:center;padding:10px 20px 6px;font-size:13px;min-height:40px;box-sizing:border-box}
.brand{letter-spacing:.06em;font-size:15px}.dim{color:#4a6b52}.status-conv{flex:1}
.path{letter-spacing:.04em;text-transform:uppercase;font-size:11px;color:#4a6b52}
.log{flex:1;padding:16px 20px;box-sizing:border-box}
.line{margin-bottom:4px;white-space:pre-wrap;font-weight:400;color:#9bc4a4}
.sys{color:#4a6b52;font-size:13px}.ok{color:#6f9a78}.val{font-weight:700;color:#9bc4a4}
.composer{display:flex;gap:8px;padding:8px 20px;box-sizing:border-box}
.prompt{color:#4a6b52}
</style></head><body>
<div class="app">
  <div class="status">
    <span class="brand">ratline</span>
    <span class="dim">$($st.id)</span>
    <span class="status-conv dim">$($st.conv)</span>
    <span class="path">$($st.path)</span>
  </div>
  <div class="log">$log</div>
  <div class="composer"><span class="prompt">&gt;</span></div>
</div>
</body></html>
"@
  Set-Content -Path $path -Value $html -Encoding utf8
}

function Make-Gif($demo, $gifName) {
  $dir = Join-Path $root "frames-$demo"
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  Get-ChildItem $dir -Filter "*.*" | Remove-Item -Force
  $n = (Get-SceneLines $demo).Count
  for ($r = 1; $r -le $n; $r++) {
    $htmlPath = Join-Path $dir ("frame-{0:D2}.html" -f $r)
    $pngPath = Join-Path $dir ("frame-{0:D2}.png" -f $r)
    Write-FrameHtml $demo $r $htmlPath
    $url = "file:///" + ($htmlPath -replace "\\", "/")
    & $edge --headless=new --disable-gpu --hide-scrollbars --window-size=720,405 --screenshot="$pngPath" "$url" | Out-Null
    if (-not (Test-Path $pngPath)) { throw "missing $pngPath" }
  }
  $list = Join-Path $env:TEMP "list-$demo.txt"
  $lines = New-Object System.Collections.Generic.List[string]
  Get-ChildItem $dir -Filter "frame-*.png" | Sort-Object Name | ForEach-Object {
    $p = $_.FullName -replace "\\", "/"
    $lines.Add("file '$p'")
    $lines.Add("duration 0.55")
  }
  $last = (Get-ChildItem $dir -Filter "frame-*.png" | Sort-Object Name | Select-Object -Last 1).FullName -replace "\\", "/"
  $lines.Add("file '$last'")
  $lines.Add("duration 1.4")
  $lines | Set-Content -Encoding ascii $list

  $palette = Join-Path $env:TEMP "pal-$demo.png"
  $gif = Join-Path $root $gifName
  cmd /c "`"$ff`" -y -f concat -safe 0 -i `"$list`" -vf `"fps=10,scale=720:405:flags=lanczos,format=rgb24,palettegen=stats_mode=full`" `"$palette`" >nul 2>&1"
  cmd /c "`"$ff`" -y -f concat -safe 0 -i `"$list`" -i `"$palette`" -lavfi `"fps=10,scale=720:405:flags=lanczos,format=rgb24[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=3`" -loop 0 `"$gif`" >nul 2>&1"
  if (-not (Test-Path $gif)) { throw "gif failed: $gif" }
  Get-Item $gif | Select-Object FullName, Length
}

Make-Gif "conversation" "demo-conversation.gif"
Make-Gif "commands" "demo-commands.gif"
