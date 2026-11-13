param(
  [string]$Base = "http://127.0.0.1:11011",
  [string]$User = "stressadmin",
  [string]$Pass = "Stress#2026!xz"
)

$ErrorActionPreference = "Continue"
$results = [System.Collections.ArrayList]::new()
$script:token = ""
$script:csrf = ""

function Log($msg) { Write-Host $msg }

function Record($phase, $ok, $detail) {
  $null = $results.Add([pscustomobject]@{ Phase = $phase; Alive = $ok; Detail = $detail })
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

function Req($method, $path, $body, $timeout = 10, $extra = @{}) {
  $h = Hdr
  foreach ($k in $extra.Keys) { $h[$k] = $extra[$k] }
  $p = @{ Uri = "$Base$path"; Method = $method; Headers = $h; UseBasicParsing = $true; TimeoutSec = $timeout }
  if ($null -ne $body) {
    $p["Body"] = $body
    if (-not $h.ContainsKey("Content-Type")) { $h["Content-Type"] = "application/json" }
  }
  try { return (Invoke-WebRequest @p).StatusCode } catch {
    if ($_.Exception.Response) { return [int]$_.Exception.Response.StatusCode }
    return -1
  }
}

Log "=== setup ==="
$setupBody = @{ username = $User; password = $Pass; panel_name = "stress" } | ConvertTo-Json
$null = Req "POST" "/api/v1/setup" $setupBody 20
$loginBody = @{ username = $User; password = $Pass } | ConvertTo-Json
try {
  $lr = Invoke-WebRequest -Uri "$Base/api/v1/auth/login" -Method POST -Body $loginBody -ContentType "application/json" -UseBasicParsing -TimeoutSec 20
  $lj = $lr.Content | ConvertFrom-Json
  $script:token = $lj.token
  $script:csrf = $lj.csrf_token
  Log "  login ok, token len=$($script:token.Length)"
} catch { Log "  login failed: $_" }

if (-not $script:token) { Log "no token, abort"; exit 2 }

Log "=== phase 1 malformed json ==="
$bad = @('{', '[]', 'null', '{"a":}', '{"target":}', ('{"a":' * 200), '{"x":' + ('1' * 5000) + '}')
foreach ($b in $bad) { $null = Req "POST" "/api/v1/bans" $b 10 }
Record "malformed-json" (Alive) "7 payloads"

Log "=== phase 2 deep nesting ==="
$deep = '{"a":' * 2000 + '1' + '}' * 2000
$null = Req "POST" "/api/v1/bans" $deep 15
Record "deep-nesting" (Alive) "4000 level"

Log "=== phase 3 oversize body ==="
$big = '{"target":"p","reason":"' + ('x' * 20000000) + '"}'
$null = Req "POST" "/api/v1/bans" $big 20
Record "oversize-20mb" (Alive) "20MB body"

Log "=== phase 4 path traversal ==="
$trav = @('/api/v1/instances/../../../../etc/passwd/files', '/api/v1/instances/1/files?path=../../../../etc/passwd', '/api/v1/instances/1/files?path=..%2f..%2f..%2fetc%2fpasswd', '/api/v1/profiles/..%2f..%2fetc')
foreach ($t in $trav) { $null = Req "GET" $t $null 10 }
Record "path-traversal" (Alive) "4 payloads"

Log "=== phase 5 header injection ==="
try {
  $ib = @{ name = "hdrprobe"; kind = "paper"; version = "1.21.4"; port = 25611 } | ConvertTo-Json
  $ir = Invoke-WebRequest -Uri "$Base/api/v1/instances" -Method POST -Body $ib -Headers (Hdr) -ContentType "application/json" -UseBasicParsing -TimeoutSec 20
  $iid = ($ir.Content | ConvertFrom-Json).id
  Log "  instance=$iid"
  if ($iid) {
    $names = @("\u6d4b\u8bd5.txt", "\u4e2d\u6587.jar", "caf\u00e9.conf", "a b.txt", "x\u00e9y.log")
    foreach ($n in $names) {
      $enc = [uri]::EscapeDataString($n)
      $null = Req "GET" "/api/v1/instances/$iid/files/download?path=$enc" $null 10
      $null = Req "GET" "/api/v1/instances/$iid/worlds/$enc/download" $null 10
    }
  }
  Record "header-injection" (Alive) "5 non-ascii names"
} catch { Record "header-injection" (Alive) "setup error: $_" }

Log "=== phase 5b non-ascii download with real file ==="
try {
  $fn = "temp" + [char]0x6D4B + [char]0x8BD5 + ".txt"
  $wb = @{ path = $fn; content = "probe" } | ConvertTo-Json
  $st = Req "PUT" "/api/v1/instances/$iid/files/content" $wb 15
  Log "  write status=$st name=$fn"
  $enc2 = [uri]::EscapeDataString($fn)
  $dl = Req "GET" "/api/v1/instances/$iid/files/download?path=$fn" $null 15
  Log "  raw path status=$dl"
  $dl2 = Req "GET" "/api/v1/instances/$iid/files/download?path=$enc2" $null 15
  Log "  encoded path status=$dl2"
  if ($dl -ne 200 -and $dl2 -ne 200) { Log "  download did not return 200" }
  Record "non-ascii-download" (Alive) "raw=$dl enc=$dl2"
} catch { Record "non-ascii-download" (Alive) "error: $_" }

Log "=== phase 6 login brute force ==="
for ($i = 0; $i -lt 120; $i++) {
  $bb = @{ username = $User; password = "wrong$i" } | ConvertTo-Json
  $null = Req "POST" "/api/v1/auth/login" $bb 6
}
Record "login-bruteforce" (Alive) "120 attempts"

Log "=== phase 7 unicode flood ==="
$uni = @("\ud83d\ude00\ud83d\udd25\ud83c\udfae", "\u4e2d\u6587\u6d4b\u8bd5", "\u0000\u0001\u0002", "a\tb\nc", "' OR 1=1 --", "<script>alert(1)</script>", "\u202e\u200b\u200c")
foreach ($u in $uni) {
  $b = @{ target = $u; reason = $u; kind = "player" } | ConvertTo-Json
  $null = Req "POST" "/api/v1/bans" $b 10
  $tb = @{ kind = "support"; player = $u; subject = $u; body = $u } | ConvertTo-Json
  $null = Req "POST" "/api/v1/tickets" $tb 10
}
Record "unicode-flood" (Alive) "7 variants x 2 endpoints"

Log "=== phase 8 instance churn ==="
$created = @()
for ($i = 0; $i -lt 25; $i++) {
  $b = @{ name = "churn-$i"; kind = "paper"; version = "1.21.4"; port = (25700 + $i) } | ConvertTo-Json
  try {
    $r = Invoke-WebRequest -Uri "$Base/api/v1/instances" -Method POST -Body $b -Headers (Hdr) -ContentType "application/json" -UseBasicParsing -TimeoutSec 15
    $created += ($r.Content | ConvertFrom-Json).id
  } catch { }
}
foreach ($id in $created) { $null = Req "DELETE" "/api/v1/instances/$id" $null 15 }
Record "instance-churn" (Alive) "$($created.Count) created+deleted"

Log "=== phase 9 concurrent storm ==="
$paths = @("/api/v1/health", "/api/v1/instances", "/api/v1/bans", "/api/v1/tickets", "/api/v1/leaderboard", "/api/v1/nodes", "/api/v1/audit", "/api/v1/tutorials")
$tok = $script:token
$paths | ForEach-Object -Parallel {
  $p = $_
  $base = $using:Base
  $tkn = $using:tok
  1..25 | ForEach-Object {
    try { $null = Invoke-WebRequest -Uri "$base$p" -Headers @{ Authorization = "Bearer $tkn" } -UseBasicParsing -TimeoutSec 10 } catch { }
  }
} -ThrottleLimit 16
Record "concurrent-storm" (Alive) "8 x 25 parallel"

Log "=== phase 10 rapid auth churn ==="
for ($i = 0; $i -lt 40; $i++) {
  $lb = @{ username = $User; password = $Pass } | ConvertTo-Json
  try {
    $r = Invoke-WebRequest -Uri "$Base/api/v1/auth/login" -Method POST -Body $lb -ContentType "application/json" -UseBasicParsing -TimeoutSec 10
    $t = ($r.Content | ConvertFrom-Json).token
    $null = Req "POST" "/api/v1/auth/logout" $null 6
  } catch { }
}
Record "auth-churn" (Alive) "40 login/logout"

Log "=== phase 11 odd limits ==="
$odd = @("/api/v1/bans?limit=0", "/api/v1/bans?limit=-1", "/api/v1/bans?limit=999999999999999999999", "/api/v1/tickets?limit=abc", "/api/v1/leaderboard?limit=" + ("9" * 100), "/api/v1/player-actions?limit=1.5", "/api/v1/audit?limit=0x10")
foreach ($o in $odd) { $null = Req "GET" $o $null 10 }
Record "odd-limits" (Alive) "7 variants"

Log "=== phase 12 header abuse ==="
$hax = @(
  @{ "x-cocktail-csrf" = ("A" * 100000) },
  @{ "Authorization" = "Bearer " + ("B" * 100000) },
  @{ "Content-Type" = "" },
  @{ "X-Forwarded-For" = "1.1.1.1,2.2.2.2," + ("3.3.3.3," * 5000) }
)
foreach ($h in $hax) { $null = Req "GET" "/api/v1/instances" $null 10 $h }
Record "header-abuse" (Alive) "4 variants"

Log "=== phase 13 numeric extremes ==="
try { $null = Req "POST" "/api/v1/bans" '{"target":"p","reason":"r","hours":-999999}' 10 } catch { }
try { $null = Req "POST" "/api/v1/bans" '{"target":"p","reason":"r","hours":0}' 10 } catch { }
try { $null = Req "POST" "/api/v1/bans" '{"target":"p","reason":"r","hours":9223372036854775807}' 10 } catch { }
try { $null = Req "POST" "/api/v1/bans" '{"target":"p","reason":"r","expires_at":""}' 10 } catch { }
try { $null = Req "POST" "/api/v1/bans" '{"target":"p","reason":"r","expires_at":"0000-00-00T00:00:00Z"}' 10 } catch { }
try { $null = Req "POST" "/api/v1/bans" '{"target":"p","reason":"r","hours":1e309}' 10 } catch { }
Record "numeric-extremes" (Alive) "6 variants"
Log "=== phase 14 missing fields ==="
$miss = @('{}', '{"target":null,"reason":null}', '{"target":[],"reason":{}}', '{"target":1,"reason":true}')
foreach ($m in $miss) { $null = Req "POST" "/api/v1/bans" $m 10; $null = Req "POST" "/api/v1/tickets" $m 10 }
Record "missing-fields" (Alive) "4 x 2 endpoints"

Log ""
Log "============================"
$crashed = $results | Where-Object { -not $_.Alive }
Log "phases=$($results.Count) crashed=$($crashed.Count)"
if ($crashed.Count -gt 0) {
  Log "crash at:"
  $crashed | ForEach-Object { Log "  - $($_.Phase)" }
  exit 1
}
try {
  @($script:token, $script:csrf) | Set-Content -Path "$PWD/run/token.txt" -ErrorAction SilentlyContinue
} catch { }
Log "SURVIVED_ALL"
exit 0
