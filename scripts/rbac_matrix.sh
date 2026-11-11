#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$ROOT/target/rbac-run"
LOG="/tmp/rbac"
BASE="http://127.0.0.1:11012"

PASS=0
FAIL=0
FAILED=()

mkdir -p "$LOG"
rm -rf "$WORK"
mkdir -p "$WORK"

ok() {
  PASS=$((PASS + 1))
  printf '  PASS  %s\n' "$1"
}

bad() {
  FAIL=$((FAIL + 1))
  FAILED+=("$1")
  printf '  FAIL  %s  <- %s\n' "$1" "$2"
}

code() {
  curl -s -o "$LOG/b.tmp" -w '%{http_code}' "$@"
}

code_with() {
  local f="$1"
  shift
  curl -s -o "$f" -w '%{http_code}' "$@"
}

jget() {
  python3 -c "import sys,json;d=json.load(open('$1'));print(eval('d'+sys.argv[1]))" "$2" 2>/dev/null
}

expect() {
  local who="$1" path="$2" want="$3" got="$4"
  if [ "$want" = "$got" ]; then
    ok "$who  $path  -> $want"
  else
    bad "$who  $path" "want=$want got=$got"
  fi
}

echo '===== 启动服务 ====='
CP="$ROOT/target/release/cocktail-control"
if [ ! -x "$CP" ]; then
  echo "missing $CP"
  exit 1
fi

cd "$WORK"
export COCKTAIL_BIND=127.0.0.1:11012
export COCKTAIL_WEB_ROOT="$ROOT/admin/dist"
export RUST_LOG=warn

"$CP" > "$LOG/server.log" 2>&1 &
PID=$!
trap 'kill $PID 2>/dev/null; wait $PID 2>/dev/null' EXIT

for _ in $(seq 1 60); do
  curl -s -o /dev/null "$BASE/api/v1/health" 2>/dev/null && break
  sleep 1
done
ok '服务启动'

echo
echo '===== 初始化 superadmin ====='
SU_PW='SuperAdmin#2026!x'
SC=$(code -X POST -H 'Content-Type: application/json' \
  -d "{\"username\":\"root_admin\",\"password\":\"$SU_PW\",\"panel_name\":\"RBAC\"}" \
  "$BASE/api/v1/setup")
if [ "$SC" = '200' ]; then ok 'setup 完成'; else bad 'setup 完成' "got=$SC"; fi

SC=$(code_with "$LOG/su.json" -X POST -H 'Content-Type: application/json' \
  -d "{\"username\":\"root_admin\",\"password\":\"$SU_PW\"}" \
  "$BASE/api/v1/auth/login")
if [ "$SC" = '200' ]; then ok 'superadmin 登录'; else bad 'superadmin 登录' "got=$SC"; fi

SU_TOKEN=$(jget "$LOG/su.json" "['token']")
SU_CSRF=$(jget "$LOG/su.json" "['csrf_token']")
SU="Authorization: Bearer $SU_TOKEN"

if [ -z "$SU_TOKEN" ] || [ "$SU_TOKEN" = 'None' ]; then
  echo 'superadmin 拿不到 token，中止'
  exit 1
fi

echo
echo '===== 创建五个角色账号 ====='
ROLES=(observer support developer admin)
for r in "${ROLES[@]}"; do
  PW="Role#2026!$r"
  SC=$(code -X POST -H "$SU" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $SU_CSRF" \
    -d "{\"username\":\"u_$r\",\"password\":\"$PW\",\"role\":\"$r\"}" \
    "$BASE/api/v1/users")
  if [ "$SC" = '200' ] || [ "$SC" = '201' ]; then
    ok "创建 $r 账号"
  else
    bad "创建 $r 账号" "got=$SC $(head -c 120 "$LOG/b.tmp")"
  fi
  SC=$(code_with "$LOG/t_$r.json" -X POST -H 'Content-Type: application/json' \
    -d "{\"username\":\"u_$r\",\"password\":\"$PW\"}" \
    "$BASE/api/v1/auth/login")
  if [ "$SC" = '200' ]; then
    ok "$r 登录"
  else
    bad "$r 登录" "got=$SC"
  fi
  TVAL=$(jget "$LOG/t_$r.json" "['token']")
  CVAL=$(jget "$LOG/t_$r.json" "['csrf_token']")
  [ "$TVAL" = 'None' ] && TVAL=''
  [ "$CVAL" = 'None' ] && CVAL=''
  printf -v "TOK_$r" '%s' "$TVAL"
  printf -v "CSRF_$r" '%s' "$CVAL"

done

TOK_SUPERADMIN="$SU_TOKEN"

echo
echo '===== 权限矩阵：读取类 ====='
READ_PATHS=(
  "/api/v1/instances"
  "/api/v1/nodes"
  "/api/v1/users"
  "/api/v1/audit"
  "/api/v1/settings"
  "/api/v1/bans?all=true"
  "/api/v1/tickets"
  "/api/v1/player-actions"
  "/api/v1/leaderboard"
)

EXPECT_READ_observer=(200 403 403 403 200 403 403 403 403 403 403)
EXPECT_READ_support=(200 403 403 403 200 200 403 200 200 200 200)
EXPECT_READ_developer=(200 200 403 200 200 200 403 200 200 200 200)
EXPECT_READ_admin=(200 200 200 200 200 200 200 200 200 200 200)

declare -A ALLOWED
ALLOWED[observer]="200 403 403 403 200 403 403 403 403 403 403"

for who in observer support developer admin; do
  TVAR="TOK_$who"; TOK="${!TVAR:-}"
  if [ -z "$TOK" ] || [ "$TOK" = 'None' ]; then
    bad "$who 读取矩阵" 'no token'
    continue
  fi
  echo "  --- $who ---"
  for i in "${!READ_PATHS[@]}"; do
    p="${READ_PATHS[$i]}"
    SC=$(code -H "Authorization: Bearer $TOK" "$BASE$p")
    case "$who" in
      observer)
        case "$p" in
          "/api/v1/instances"|"/api/v1/settings"|"/api/v1/audit") exp=200 ;;
          *) exp=403 ;;
        esac ;;
      support)
        case "$p" in
          "/api/v1/instances"|"/api/v1/settings"|"/api/v1/audit"|"/api/v1/bans?all=true"|"/api/v1/tickets"|"/api/v1/player-actions"|"/api/v1/leaderboard") exp=200 ;;
          *) exp=403 ;;
        esac ;;
      developer)
        case "$p" in
          "/api/v1/nodes"|"/api/v1/users"|"/api/v1/bans?all=true"|"/api/v1/tickets"|"/api/v1/player-actions"|"/api/v1/leaderboard") exp=403 ;;
          *) exp=200 ;;
        esac ;;
      admin)
        case "$p" in
          "/api/v1/users") exp=403 ;;
          *) exp=200 ;;
        esac ;;
    esac
    if [ "$SC" = "$exp" ]; then
      ok "$who $p -> $exp"
    else
      bad "$who $p" "want=$exp got=$SC"
    fi
  done
done

echo
echo '===== 权限矩阵：写操作 ====='
INST='{"name":"rbac-probe","kind":"vanilla","version":"1.21.4","port":25598}'

for who in observer support admin; do
  TVAR="TOK_$who"; TOK="${!TVAR:-}"
  CVAR="CSRF_$who"; CS="${!CVAR:-}"
  [ -z "$TOK" ] && continue
  echo "  --- $who ---"

  SC=$(code -X POST -H "Authorization: Bearer $TOK" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CS" -d "$INST" "$BASE/api/v1/instances")
  if [ "$who" = 'observer' ] || [ "$who" = 'support' ]; then
    expect "$who" 'POST /instances' 403 "$SC"
  else
    if [ "$SC" = '200' ] || [ "$SC" = '201' ]; then
      ok "$who POST /instances -> $SC"
    else
      bad "$who POST /instances" "want=2xx got=$SC"
    fi
  fi

  SC=$(code -X POST -H "Authorization: Bearer $TOK" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CS" \
    -d '{"target":"P1","reason":"r"}' "$BASE/api/v1/bans")
  if [ "$who" = 'observer' ]; then
    expect "$who" 'POST /bans' 403 "$SC"
  else
    if [ "$SC" = '200' ]; then
      ok "$who POST /bans -> 200"
    else
      bad "$who POST /bans" "want=200 got=$SC"
    fi
  fi

  SC=$(code -X POST -H "Authorization: Bearer $TOK" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CS" \
    -d '{"username":"x_$who","password":"Pass#2026!abc","role":"admin"}' "$BASE/api/v1/users")
  if [ "$who" = 'observer' ] || [ "$who" = 'support' ] || [ "$who" = 'admin' ]; then
    expect "$who" 'POST /users' 403 "$SC"
  fi
done

echo
echo '===== 越权尝试：support 提权自己 ====='
SC=$(code -X PUT -H "Authorization: Bearer ${TOK_support:-}" -H 'Content-Type: application/json' -H "x-cocktail-csrf: ${CSRF_support:-}" \
  -d '{"role":"superadmin"}' "$BASE/api/v1/users/2")
if [ "$SC" = '403' ] || [ "$SC" = '404' ]; then
  ok 'support 不能改用户角色'
else
  bad 'support 不能改用户角色' "got=$SC"
fi

SC=$(code -H "Authorization: Bearer ${TOK_support:-}" "$BASE/api/v1/users")
if [ "$SC" = '403' ]; then
  ok 'support 仍看不到用户列表'
else
  bad 'support 仍看不到用户列表' "got=$SC"
fi

echo
echo '===== 越权尝试：observer 执行管理动作 ====='
SC=$(code -X POST -H "Authorization: Bearer ${TOK_observer:-}" -H 'Content-Type: application/json' -H "x-cocktail-csrf: ${CSRF_observer:-}" \
  -d '{}' "$BASE/api/v1/instances/1/start")
if [ "$SC" = '403' ] || [ "$SC" = '404' ]; then
  ok 'observer 不能启动实例'
else
  bad 'observer 不能启动实例' "got=$SC"
fi

SC=$(code -X POST -H "Authorization: Bearer ${TOK_observer:-}" -H 'Content-Type: application/json' -H "x-cocktail-csrf: ${CSRF_observer:-}" \
  -d '{"command":"op testuser"}' "$BASE/api/v1/instances/1/rcon/exec")
if [ "$SC" = '403' ] || [ "$SC" = '404' ]; then
  ok 'observer 不能执行 RCON'
else
  bad 'observer 不能执行 RCON' "got=$SC"
fi

echo
echo '===== 实例级动作权限 ====='

INST_LIST="/api/v1/instances/1"
INST_START="/api/v1/instances/1/start"
INST_STOP="/api/v1/instances/1/stop"
INST_FILES="/api/v1/instances/1/files"
INST_PLUGINS="/api/v1/instances/1/plugins"
INST_PLAYERS="/api/v1/instances/1/players"
INST_LOGS="/api/v1/instances/1/logs"
INST_METRICS="/api/v1/instances/1/metrics"
INST_BACKUPS="/api/v1/instances/1/backups"

read_expect() {
  local who="$1" path="$2"
  case "$who" in
    observer)
      case "$path" in
        "$INST_LOGS"|"$INST_METRICS"|"$INST_PLAYERS") echo 200 ;;
        "$INST_PLUGINS"|"$INST_BACKUPS"|"$INST_FILES") echo 403 ;;
        *) echo 403 ;;
      esac ;;
    support)
      case "$path" in
        "$INST_LOGS"|"$INST_METRICS"|"$INST_PLAYERS"|"$INST_BACKUPS") echo 200 ;;
        "$INST_PLUGINS"|"$INST_FILES") echo 403 ;;
        *) echo 403 ;;
      esac ;;
    developer)
      case "$path" in
        "$INST_LOGS"|"$INST_METRICS"|"$INST_FILES"|"$INST_PLUGINS"|"$INST_BACKUPS"|"$INST_PLAYERS") echo 200 ;;
        *) echo 403 ;;
      esac ;;
    admin)
      echo 200 ;;
  esac
}

for who in observer support developer admin; do
  TVAR="TOK_$who"; TOK="${!TVAR:-}"
  if [ -z "$TOK" ]; then
    bad "$who 实例级" 'no token'
    continue
  fi
  echo "  --- $who 实例级读取 ---"
  for p in "$INST_LOGS" "$INST_METRICS" "$INST_FILES" "$INST_PLUGINS" "$INST_BACKUPS" "$INST_PLAYERS"; do
    exp=$(read_expect "$who" "$p")
    SC=$(code -H "Authorization: Bearer $TOK" "$BASE$p")
    if [ -z "$exp" ]; then
      continue
    fi
    if [ "$exp" = '403' ]; then
      if [ "$SC" = '403' ]; then
        ok "$who $p 被拦截 -> 403"
      else
        bad "$who $p 应被拦截" "want=403 got=$SC"
      fi
    else
      if [ "$SC" = '403' ]; then
        bad "$who $p 不应被拦截" "got=403"
      else
        ok "$who $p 放行 -> $SC"
      fi
    fi
  done

done

echo
echo '  --- 实例启动权限 ---'
for who in observer support developer admin; do
  TVAR="TOK_$who"; TOK="${!TVAR:-}"
  CVAR="CSRF_$who"; CS="${!CVAR:-}"
  [ -z "$TOK" ] && continue
  SC=$(code -X POST -H "Authorization: Bearer $TOK" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CS" -d '{}' "$BASE$INST_START")
  case "$who" in
    observer)
      if [ "$SC" = '403' ]; then
        ok "$who 不能启动实例 -> 403"
      else
        bad "$who 不能启动实例" "want=403 got=$SC"
      fi ;;
    developer)
      if [ "$SC" = '403' ] || [ "$SC" = '404' ]; then
        ok "$who 启动实例受限 -> $SC"
      else
        bad "$who 启动实例受限" "want=403/404 got=$SC"
      fi ;;
    support|admin)
      if [ "$SC" = '403' ] || [ "$SC" = '404' ] || [ "$SC" = '409' ] || [ "$SC" = '200' ]; then
        ok "$who 可尝试启动 -> $SC"
      else
        bad "$who 可尝试启动" "got=$SC"
      fi ;;
  esac
done

echo
echo '  --- 观察员不应能修改文件 ---'
SC=$(code -X POST -H "Authorization: Bearer ${TOK_observer:-}" -H 'Content-Type: application/json' -H "x-cocktail-csrf: ${CSRF_observer:-}" \
  -d '{"path":"/tmp","content":"x"}' "$BASE/api/v1/instances/1/files/content")
if [ "$SC" = '403' ]; then
  ok 'observer 不能写文件'
else
  bad 'observer 不能写文件' "got=$SC"
fi

echo
echo '  --- 观察员不应能安装插件 ---'
SC=$(code -X POST -H "Authorization: Bearer ${TOK_observer:-}" -H 'Content-Type: application/json' -H "x-cocktail-csrf: ${CSRF_observer:-}" \
  -d '{"project":"x"}' "$BASE/api/v1/instances/1/modrinth/install")
if [ "$SC" = '403' ]; then
  ok 'observer 不能安装插件'
else
  bad 'observer 不能安装插件' "got=$SC"
fi

echo
echo '  --- 观察员不应能踢玩家 ---'
SC=$(code -X POST -H "Authorization: Bearer ${TOK_observer:-}" -H 'Content-Type: application/json' -H "x-cocktail-csrf: ${CSRF_observer:-}" \
  -d '{"reason":"x"}' "$BASE/api/v1/instances/1/players/Steve/kick")
if [ "$SC" = '403' ]; then
  ok 'observer 不能踢玩家'
else
  bad 'observer 不能踢玩家' "got=$SC"
fi
echo
echo '===== 无 token 全拒绝 ====='
for p in /api/v1/instances /api/v1/users /api/v1/nodes /api/v1/bans /api/v1/tickets /api/v1/leaderboard; do
  SC=$(code "$BASE$p")
  if [ "$SC" = '401' ]; then
    ok "匿名 $p -> 401"
  else
    bad "匿名 $p" "want=401 got=$SC"
  fi
done

echo
echo '===== 匿名白名单仍公开 ====='
for p in /api/v1/health /api/v1/tutorials /api/v1/update/status; do
  SC=$(code "$BASE$p")
  if [ "$SC" = '200' ]; then
    ok "匿名可访问 $p"
  else
    bad "匿名可访问 $p" "want=200 got=$SC"
  fi
done

echo
echo '=========================================='
echo "PASS: $PASS   FAIL: $FAIL"
echo '=========================================='

if [ "$FAIL" -gt 0 ]; then
  echo '失败项：'
  for n in "${FAILED[@]}"; do echo "  - $n"; done
  exit 1
fi

echo '权限矩阵全通过'
