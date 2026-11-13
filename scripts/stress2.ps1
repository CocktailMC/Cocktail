param(
  [string]$Base = "http://127.0.0.1:11011",
  [string]$User = "stressadmin",
  [string]$Pass = "Stress#2026!xz"
)

$ErrorActionPreference = "Continue"
$results = [System.Collections.ArrayList]::new()
$script:token = ""
$script:csrf = ""

function Log($m) { Write-Host $m }

function Record($phase, $ok, $detail) {
  $null = $results.Add([pscustomobject]@{ Phase = $phase; Alive = $ok })
  if ($ok) { Write-Host "  PASS  $phase  $detail" }
  else { Write-Host "  CRASH $phase  $detail" }
}

function Alive {
  try {
    $r = Invoke-WebRequest -Uri "$Base/api/v1/health" -UseBasicParsing -TimeoutSec 5
    return ($r.StatusCode -eq 200)
  } catch { return $false }
}

function Hdr {
  $h = @{ "Authorization" = "Bearer $($script:token)" }
  if ($script:csrf) { $h["x-cocktail-csrf"] = $script:csrf }
  return $h
}

Log "=== token pickup ==="
$tf = "$PWD/run/token.txt"
if (Test-Path $tf) {
  $lines2 = @(Get-Content -Path $tf)
  if ($lines2.Count -ge 1) { $script:token = $lines2[0].Trim() }
  if ($lines2.Count -ge 2) { $script:csrf = $lines2[1].Trim() }
  Log "  reused token len=$($script:token.Length) csrf len=$($script:csrf.Length)"
}
if (-not $script:token) {
  try {
    $lb = @{ username = $User; password = $Pass } | ConvertTo-Json
    $r = Invoke-WebRequest -Uri "$Base/api/v1/auth/login" -Method POST -Body $lb -ContentType "application/json" -UseBasicParsing -TimeoutSec 20
    $j = $r.Content | ConvertFrom-Json
    $script:token = $j.token
    Log "  fresh login len=$($script:token.Length)"
  } catch { Log "  login failed" }
}
if (-not $script:token) { Log "abort"; exit 2 }

try {
  $mr = Invoke-WebRequest -Uri "$Base/api/v1/auth/me" -Headers @{ Authorization = "Bearer $($script:token)" } -UseBasicParsing -TimeoutSec 10
  $mj2 = $mr.Content | ConvertFrom-Json
  if (-not $script:csrf -and $mj2.csrf_token) { $script:csrf = $mj2.csrf_token }
  Log "  me ok role=$($mj2.role) csrf=$($script:csrf.Length)"
} catch { Log "  me failed" }
if (-not $script:csrf) { Log "  no csrf, write ops will 403" }

Log "=== p1 probe instance ==="
$iid = ""
foreach ($pt in @(26100, 26101, 26102, 26103)) {
  try {
    $ib = @{ name = "deep-probe-$pt"; kind = "paper"; version = "1.21.4"; port = $pt } | ConvertTo-Json
    $r = Invoke-WebRequest -Uri "$Base/api/v1/instances" -Method POST -Body $ib -Headers (Hdr) -ContentType "application/json" -UseBasicParsing -TimeoutSec 20
    $iid = ($r.Content | ConvertFrom-Json).id
    if ($iid) { Log "  created on port $pt"; break }
  } catch {
    $msg = ""
    if ($_.Exception.Response) {
      try { $sr = New-Object System.IO.StreamReader($_.Exception.Response.GetResponseStream()); $msg = $sr.ReadToEnd() } catch { }
    }
    Log "  port $pt failed: $($_.Exception.Message) $msg"
  }
}
Log "  iid=$iid"
Record "probe-instance" (Alive) "iid=$iid"

Log "=== p2 websocket churn ==="
$wsOk = 0
for ($i = 0; $i -lt 40; $i++) {
  try {
    $c = [System.Net.WebSockets.ClientWebSocket]::new()
    $u = [uri]("ws://127.0.0.1:11011/api/v1/events/ws?token=" + $script:token)
    $t = $c.ConnectAsync($u, [System.Threading.CancellationToken]::None)
    if ($t.Wait(4000)) { $wsOk++ }
    $null = $c.CloseAsync([System.Net.WebSockets.WebSocketCloseStatus]::NormalClosure, "bye", [System.Threading.CancellationToken]::None).Wait(2000)
    $c.Dispose()
  } catch { }
}
Record "ws-churn" (Alive) "connected=$wsOk/40"

Log "=== p3 websocket abort storm ==="
for ($i = 0; $i -lt 30; $i++) {
  try {
    $c = [System.Net.WebSockets.ClientWebSocket]::new()
    $u = [uri]("ws://127.0.0.1:11011/api/v1/events/ws?token=" + $script:token)
    $null = $c.ConnectAsync($u, [System.Threading.CancellationToken]::None).Wait(3000)
    $c.Abort()
    $c.Dispose()
  } catch { }
}
Record "ws-abort" (Alive) "30 aborted"

Log "=== p4 ws bad token ==="
for ($i = 0; $i -lt 20; $i++) {
  try {
    $c = [System.Net.WebSockets.ClientWebSocket]::new()
    $u = [uri]("ws://127.0.0.1:11011/api/v1/events/ws?token=bogus$i")
    $null = $c.ConnectAsync($u, [System.Threading.CancellationToken]::None).Wait(3000)
    $c.Dispose()
  } catch { }
}
Record "ws-badtoken" (Alive) "20 rejects"

if ($iid) {
  Log "=== p5 multipart abuse ==="
  $cases = @(
    @{ b = "X"; ct = "multipart/form-data; boundary=X"; body = "--X\r\nContent-Disposition: form-data; name=`"file`"; filename=`"a.txt`"\r\n\r\nhello\r\n--X--\r\n" },
    @{ b = "Y"; ct = "multipart/form-data; boundary=Y"; body = "--Y\r\nContent-Disposition: form-data; name=`"file`"; filename=`"../../evil.txt`"\r\n\r\nx\r\n--Y--\r\n" },
    @{ b = "Z"; ct = "multipart/form-data"; body = "garbage-no-boundary" },
    @{ b = "W"; ct = "multipart/form-data; boundary=W"; body = "--W\r\nContent-Disposition: form-data; name=`"file`"; filename=`"测试.bin`"\r\n\r\n\u4e2d\u6587\r\n--W--\r\n" }
  )
  foreach ($cs in $cases) {
    try {
      $null = Invoke-WebRequest -Uri "$Base/api/v1/instances/$iid/files/upload?path=up.bin" -Method POST -Headers (Hdr) -ContentType $cs.ct -Body $cs.body -UseBasicParsing -TimeoutSec 15
    } catch { }
  }
  Record "multipart-abuse" (Alive) "4 payloads"

  Log "=== p6 sustained load 45s ==="
  $paths = @("/api/v1/instances", "/api/v1/bans", "/api/v1/tickets", "/api/v1/leaderboard", "/api/v1/audit")
  $tok = $script:token
  $base = $Base
  $jobs = @()
  foreach ($p in $paths) {
    $jobs += Start-Job -ScriptBlock {
      param($b, $pth, $tk)
      $deadline = (Get-Date).AddSeconds(45)
      $n = 0
      while ((Get-Date) -lt $deadline) {
        try { $null = Invoke-WebRequest -Uri "$b$pth" -Headers @{ Authorization = "Bearer $tk" } -UseBasicParsing -TimeoutSec 10; $n++ } catch { }
      }
      $n
    } -ArgumentList $base, $p, $tok
  }
  $null = $jobs | Wait-Job -Timeout 90
  $total = 0
  foreach ($j in $jobs) {
    try { $total += [int](Receive-Job -Job $j -ErrorAction SilentlyContinue) } catch { }
    Remove-Job -Job $j -Force -ErrorAction SilentlyContinue
  }
  Record "sustained-load" (Alive) "requests=$total in 45s"

  Log "=== p7 duplicate create race ==="
  $dup = @{ name = "dup-race"; kind = "paper"; version = "1.21.4"; port = 25613 } | ConvertTo-Json
  for ($i = 0; $i -lt 12; $i++) {
    try {
      $null = Invoke-WebRequest -Uri "$Base/api/v1/instances" -Method POST -Body $dup -Headers (Hdr) -ContentType "application/json" -UseBasicParsing -TimeoutSec 15
    } catch { }
  }
  Record "duplicate-create" (Alive) "12 same-name"

  Log "=== p8 query abuse ==="
  $q1 = "?path=" + ("a" * 60000)
  $q2 = "?limit=" + ("%20" * 20000)
  try { $null = Invoke-WebRequest -Uri "$Base/api/v1/instances/$iid/files$q1" -Headers (Hdr) -UseBasicParsing -TimeoutSec 10 } catch { }
  try { $null = Invoke-WebRequest -Uri "$Base/api/v1/bans$q2" -Headers (Hdr) -UseBasicParsing -TimeoutSec 10 } catch { }
  $deep = "/api/v1/instances/" + ("../" * 2000) + "x/files"
  try { $null = Invoke-WebRequest -Uri "$Base$deep" -Headers (Hdr) -UseBasicParsing -TimeoutSec 10 } catch { }
  Record "query-abuse" (Alive) "60KB path + deep dots"

  Log "=== p9 json array flood ==="
  $arr = '[' + (('"x",' * 40000)) + '"end"]'
  try { $null = Invoke-WebRequest -Uri "$Base/api/v1/bans" -Method POST -Headers (Hdr) -ContentType "application/json" -Body $arr -UseBasicParsing -TimeoutSec 20 } catch { }
  $obj = '{"instances":' + $arr + '}'
  try { $null = Invoke-WebRequest -Uri "$Base/api/v1/bans" -Method POST -Headers (Hdr) -ContentType "application/json" -Body $obj -UseBasicParsing -TimeoutSec 20 } catch { }
  Record "array-flood" (Alive) "40k elements"
}

Log "=== p10 session flood ==="
for ($i = 0; $i -lt 60; $i++) {
  try {
    $lb = @{ username = $User; password = $Pass } | ConvertTo-Json
    $null = Invoke-WebRequest -Uri "$Base/api/v1/auth/login" -Method POST -Body $lb -ContentType "application/json" -UseBasicParsing -TimeoutSec 10
  } catch { }
}
Record "session-flood" (Alive) "60 sessions"

Log "=== p11 repeated setup ==="
for ($i = 0; $i -lt 10; $i++) {
  try {
    $sb = @{ username = "late$i"; password = "Late#2026!xz"; panel_name = "x" } | ConvertTo-Json
    $null = Invoke-WebRequest -Uri "$Base/api/v1/setup" -Method POST -Body $sb -ContentType "application/json" -UseBasicParsing -TimeoutSec 10
  } catch { }
}
Record "repeat-setup" (Alive) "10 post-init"

Log "=== p12 metrics under load ==="
$tok = $script:token
$murl = if ($iid) { "$Base/api/v1/instances/$iid/metrics" } else { "$Base/api/v1/instances" }
$mj = Start-Job -ScriptBlock {
  param($u, $tk)
  $n = 0
  for ($i = 0; $i -lt 80; $i++) {
    try { $null = Invoke-WebRequest -Uri $u -Headers @{ Authorization = "Bearer $tk" } -UseBasicParsing -TimeoutSec 8; $n++ } catch { }
  }
  $n
} -ArgumentList $murl, $tok
$null = $mj | Wait-Job -Timeout 120
$mc = 0
try { $mc = [int](Receive-Job -Job $mj -ErrorAction SilentlyContinue) } catch { }
Remove-Job -Job $mj -Force -ErrorAction SilentlyContinue
Record "metrics-load" (Alive) "ok=$mc/80"

Log ""
Log "============================"
$bad = $results | Where-Object { -not $_.Alive }
Log "phases=$($results.Count) crashed=$($bad.Count)"
if ($bad.Count -gt 0) {
  $bad | ForEach-Object { Log "  - $($_.Phase)" }
  exit 1
}
Log "SURVIVED_ALL_2"
exit 0
