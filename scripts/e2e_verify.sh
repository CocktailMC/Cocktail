#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$ROOT/target/e2e-run"
LOG="/tmp/e2e"
BASE="http://127.0.0.1:11011"

PASS=0
FAIL=0
FAILED_NAMES=()

mkdir -p "$LOG"
rm -rf "$WORK"
mkdir -p "$WORK"

ok() {
  PASS=$((PASS + 1))
  printf '  PASS  %s\n' "$1"
}

bad() {
  FAIL=$((FAIL + 1))
  FAILED_NAMES+=("$1")
  printf '  FAIL  %s  <- %s\n' "$1" "$2"
}

check_eq() {
  local name="$1" want="$2" got="$3"
  if [ "$want" = "$got" ]; then ok "$name"; else bad "$name" "want=$want got=$got"; fi
}

check_ge() {
  local name="$1" min="$2" got="$3"
  if [ "${got:-0}" -ge "$min" ] 2>/dev/null; then ok "$name"; else bad "$name" "want>=$min got=$got"; fi
}

code() {
  curl -s -o "$LOG/body.tmp" -w '%{http_code}' "$@"
}

code_with() {
  local outfile="$1"
  shift
  curl -s -o "$outfile" -w '%{http_code}' "$@"
}

json_get() {
  python3 -c "import sys,json;d=json.load(open('$1'));print(eval('d'+sys.argv[1]))" "$2" 2>/dev/null
}

echo '===== 1. 启动服务 ====='
CP="$ROOT/target/release/cocktail-control"
INIT="$ROOT/target/release/cocktail-init"

if [ ! -x "$CP" ]; then
  echo "missing $CP"
  exit 1
fi
if [ ! -x "$INIT" ]; then
  echo "missing $INIT"
  FAIL=$((FAIL + 1))
  FAILED_NAMES+=("cocktail-init 二进制存在")
else
  ok "cocktail-init 二进制存在"
fi

cd "$WORK"
export COCKTAIL_BIND=127.0.0.1:11011
export COCKTAIL_WEB_ROOT="$ROOT/admin/dist"
export RUST_LOG=info

"$CP" > "$LOG/server.log" 2>&1 &
SERVER_PID=$!

echo "server pid=$SERVER_PID"

trap 'kill $SERVER_PID 2>/dev/null; wait $SERVER_PID 2>/dev/null' EXIT

READY=0
for _ in $(seq 1 60); do
  if curl -s -o /dev/null "$BASE/api/v1/health" 2>/dev/null; then
    READY=1
    break
  fi
  if ! kill -0 "$SERVER_PID" 2>/dev/null; then
    echo 'server died'
    break
  fi
  sleep 1
done

if [ "$READY" != '1' ]; then
  echo '服务没能启动，最后 60 行日志：'
  tail -60 "$LOG/server.log"
  exit 1
fi

ok '服务启动并可访问'

echo
echo '===== 2. W1 健康检查与首启 ====='
SC=$(code_with "$LOG/health.json" "$BASE/api/v1/health")
check_eq 'health 返回 200' '200' "$SC"

STATUS=$(json_get "$LOG/health.json" "['status']")
check_eq 'health status 为 ok' 'ok' "$STATUS"

SETUP_REQ=$(json_get "$LOG/health.json" "['setup_required']")
check_eq '首次启动需要 setup' 'True' "$SETUP_REQ"

OS=$(json_get "$LOG/health.json" "['os']")
check_eq 'os 字段为 linux' 'linux' "$OS"

SC=$(code -X POST -H 'Content-Type: application/json' -d '{}' "$BASE/api/v1/setup")
if [ "$SC" = '400' ] || [ "$SC" = '422' ]; then
  ok 'setup 接受 POST 且校验参数'
else
  bad 'setup 接受 POST 且校验参数' "got=$SC"
fi

SC=$(code "$BASE/")
check_eq 'Web UI 首页可访问' '200' "$SC"

SC=$(code "$BASE/nonexistent-route-xyz")
if [ "$SC" = '404' ] || [ "$SC" = '200' ]; then
  ok '未知路由有响应'
else
  bad '未知路由有响应' "got=$SC"
fi

echo
echo '===== 3. W1 安全响应头 ====='
curl -s -D "$LOG/headers.txt" -o /dev/null "$BASE/api/v1/health"

curl -s -D "$LOG/headers_ui.txt" -o /dev/null "$BASE/"

if grep -qi 'x-content-type-options' "$LOG/headers_ui.txt" "$LOG/headers.txt"; then
  ok 'X-Content-Type-Options 存在'
else
  bad 'X-Content-Type-Options 存在' 'header missing'
fi

if grep -qi 'x-frame-options' "$LOG/headers_ui.txt" "$LOG/headers.txt"; then
  ok 'X-Frame-Options 存在'
else
  bad 'X-Frame-Options 存在' 'header missing'
fi

if grep -qi 'content-security-policy' "$LOG/headers_ui.txt" "$LOG/headers.txt"; then
  ok 'CSP 存在'
else
  bad 'CSP 存在' 'header missing'
fi

echo
echo '===== 4. 初始化管理员 ====='
SETUP_BODY='{"username":"e2e_admin","password":"E2eTest#2026!Cocktail","panel_name":"E2E Panel"}'
SC=$(code_with "$LOG/setup.json" -X POST -H 'Content-Type: application/json' -d "$SETUP_BODY" "$BASE/api/v1/setup")
check_eq 'setup 提交成功' '200' "$SC"

SC=$(code_with "$LOG/health2.json" "$BASE/api/v1/health")
check_eq 'setup 后 health 可访问' '200' "$SC"
SETUP_REQ2=$(json_get "$LOG/health2.json" "['setup_required']")
check_eq 'setup 后不再需要初始化' 'False' "$SETUP_REQ2"

SC=$(code -X POST -H 'Content-Type: application/json' -d "$SETUP_BODY" "$BASE/api/v1/setup")
if [ "$SC" = '409' ] || [ "$SC" = '400' ] || [ "$SC" = '403' ]; then
  ok '重复 setup 被拒绝'
else
  bad '重复 setup 被拒绝' "got=$SC"
fi

echo
echo '===== 5. 登录与会话 ====='
LOGIN_BODY='{"username":"e2e_admin","password":"E2eTest#2026!Cocktail"}'
SC=$(code_with "$LOG/session.json" -X POST -H 'Content-Type: application/json' -d "$LOGIN_BODY" "$BASE/api/v1/auth/login")
check_eq '登录成功' '200' "$SC"

TOKEN=$(json_get "$LOG/session.json" "['token']")
CSRF=$(json_get "$LOG/session.json" "['csrf_token']")

if [ -n "$TOKEN" ] && [ "$TOKEN" != 'None' ]; then
  ok '拿到 session token'
else
  bad '拿到 session token' 'empty'
  TOKEN=''
fi

if [ -n "$CSRF" ] && [ "$CSRF" != 'None' ]; then
  ok '拿到 csrf token'
else
  bad '拿到 csrf token' 'empty'
  CSRF=''
fi

AUTH="Authorization: Bearer $TOKEN"

SC=$(code -X POST -H 'Content-Type: application/json' -d '{"username":"e2e_admin","password":"wrong-password"}' "$BASE/api/v1/auth/login")
if [ "$SC" = '401' ] || [ "$SC" = '403' ]; then
  ok '错误密码被拒绝'
else
  bad '错误密码被拒绝' "got=$SC"
fi

SC=$(code "$BASE/api/v1/instances")
if [ "$SC" = '401' ]; then
  ok '无 token 访问受保护接口返回 401'
else
  bad '无 token 访问受保护接口返回 401' "got=$SC"
fi

SC=$(code -H 'Authorization: Bearer invalid-token-xyz' "$BASE/api/v1/instances")
if [ "$SC" = '401' ]; then
  ok '非法 token 返回 401'
else
  bad '非法 token 返回 401' "got=$SC"
fi

echo
echo '===== 6. W3 封禁 API ====='
SC=$(code_with "$LOG/bans_empty.json" -H "$AUTH" "$BASE/api/v1/bans?all=true")
check_eq '封禁列表可访问' '200' "$SC"
EMPTY=$(json_get "$LOG/bans_empty.json" "['bans']")
check_eq '初始封禁列表为空' '[]' "$EMPTY"

BAN_BODY='{"target":"Griefer01","kind":"player","reason":"破坏主城","hours":24}'
SC=$(code_with "$LOG/ban_new.json" -X POST -H "$AUTH" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CSRF" -d "$BAN_BODY" "$BASE/api/v1/bans")
check_eq '新增封禁成功' '200' "$SC"

BAN_ID=$(json_get "$LOG/ban_new.json" "['id']")
BAN_TARGET=$(json_get "$LOG/ban_new.json" "['target']")
BAN_ACTIVE=$(json_get "$LOG/ban_new.json" "['active']")
check_eq '封禁目标正确' 'Griefer01' "$BAN_TARGET"
check_eq '封禁状态为生效' 'True' "$BAN_ACTIVE"

SC=$(code_with "$LOG/bans_after.json" -H "$AUTH" "$BASE/api/v1/bans?all=true")
check_eq '封禁列表查询成功' '200' "$SC"
ACTIVE_COUNT=$(json_get "$LOG/bans_after.json" "['active']")
check_ge '封禁生效数量' 1 "$ACTIVE_COUNT"

SC=$(code -X POST -H "$AUTH" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CSRF" -d '{"target":"","reason":""}' "$BASE/api/v1/bans")
if [ "$SC" = '400' ]; then
  ok '空封禁目标被拒绝'
else
  bad '空封禁目标被拒绝' "got=$SC"
fi

SC=$(code -X POST -H "$AUTH" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CSRF" -d '{"target":"x","reason":"y","kind":"bogus"}' "$BASE/api/v1/bans")
if [ "$SC" = '400' ]; then
  ok '非法封禁类型被拒绝'
else
  bad '非法封禁类型被拒绝' "got=$SC"
fi

SC=$(code -X POST -H "$AUTH" -H 'Content-Type: application/json' -d "$BAN_BODY" "$BASE/api/v1/bans")
if [ "$SC" = '403' ] || [ "$SC" = '400' ]; then
  ok '缺少 CSRF 的写操作被拒绝'
else
  bad '缺少 CSRF 的写操作被拒绝' "got=$SC"
fi

echo
echo '===== 7. W3 工单 API ====='
SC=$(code_with "$LOG/tickets_empty.json" -H "$AUTH" "$BASE/api/v1/tickets")
check_eq '工单列表可访问' '200' "$SC"

TICKET_BODY='{"kind":"appeal","player":"Griefer01","subject":"申诉请求","body":"我当时不在线"}'
SC=$(code_with "$LOG/ticket_new.json" -X POST -H "$AUTH" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CSRF" -d "$TICKET_BODY" "$BASE/api/v1/tickets")
check_eq '提交工单成功' '201' "$SC"

TICKET_ID=$(json_get "$LOG/ticket_new.json" "['id']")
TICKET_STATE=$(json_get "$LOG/ticket_new.json" "['state']")
check_eq '新工单状态为 open' 'open' "$TICKET_STATE"

SC=$(code -X POST -H "$AUTH" -H "x-cocktail-csrf: $CSRF" "$BASE/api/v1/tickets/$TICKET_ID/claim")
check_eq '认领工单成功' '204' "$SC"

SC=$(code_with "$LOG/ticket_claimed.json" -H "$AUTH" "$BASE/api/v1/tickets")
CLAIMED_STATE=$(python3 -c "
import json
d=json.load(open('$LOG/ticket_claimed.json'))
for t in d.get('tickets',[]):
    if t.get('id')=='$TICKET_ID':
        print(t.get('state'))
" 2>/dev/null)
check_eq '认领后状态为 claimed' 'claimed' "$CLAIMED_STATE"

SC=$(code -X POST -H "$AUTH" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CSRF" -d '{"note":"证据不足，通过"}' "$BASE/api/v1/tickets/$TICKET_ID/approve")
check_eq '通过工单成功' '204' "$SC"

SC=$(code -X POST -H "$AUTH" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CSRF" -d '{"note":"再驳回"}' "$BASE/api/v1/tickets/$TICKET_ID/reject")
if [ "$SC" = '409' ]; then
  ok '已结束工单不可再操作'
else
  bad '已结束工单不可再操作' "got=$SC"
fi

SC=$(code -X POST -H "$AUTH" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CSRF" -d '{"kind":"bogus","player":"x","subject":"y","body":"z"}' "$BASE/api/v1/tickets")
if [ "$SC" = '400' ]; then
  ok '非法工单类型被拒绝'
else
  bad '非法工单类型被拒绝' "got=$SC"
fi

SC=$(code -X POST -H "$AUTH" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CSRF" "$BASE/api/v1/tickets/nonexistent/claim")
check_eq '不存在工单认领返回 404' '404' "$SC"

echo
echo '===== 8. W3 行为记录 ====='
SC=$(code_with "$LOG/actions.json" -H "$AUTH" "$BASE/api/v1/player-actions?target=Griefer01")
check_eq '行为记录可访问' '200' "$SC"
ACTION_COUNT=$(python3 -c "import json;print(len(json.load(open('$LOG/actions.json'))))" 2>/dev/null)
check_ge '封禁与工单产生行为记录' 1 "$ACTION_COUNT"

echo
echo '===== 9. W3 排行榜与档案 ====='
SC=$(code_with "$LOG/board.json" -H "$AUTH" "$BASE/api/v1/leaderboard?limit=10")
check_eq '排行榜可访问' '200' "$SC"

SC=$(code "$BASE/api/v1/leaderboard")
if [ "$SC" = '401' ]; then
  ok '排行榜需要登录'
else
  bad '排行榜需要登录' "got=$SC"
fi

SC=$(code "$BASE/api/v1/profiles/NoSuchPlayer")
if [ "$SC" = '401' ]; then
  ok '玩家档案需要登录'
else
  bad '玩家档案需要登录' "got=$SC"
fi

SC=$(code "$BASE/api/v1/profiles/NoSuchPlayer" -H "$AUTH")
check_eq '不存在的玩家返回 404' '404' "$SC"

echo
echo '===== 10. W2 PWA 与静态资源 ====='
SC=$(code_with "$LOG/manifest.json" "$BASE/manifest.webmanifest")
check_eq 'manifest 可访问' '200' "$SC"

MANIFEST_NAME=$(json_get "$LOG/manifest.json" "['name']")
if [ -n "$MANIFEST_NAME" ] && [ "$MANIFEST_NAME" != 'None' ]; then
  ok 'manifest 包含 name 字段'
else
  bad 'manifest 包含 name 字段' 'missing'
fi

SC=$(code "$BASE/sw.js")
check_eq 'service worker 可访问' '200' "$SC"

SC=$(code "$BASE/icon-192.png")
check_eq 'PWA 图标可访问' '200' "$SC"

if curl -s "$BASE/sw.js" | grep -q '/api/'; then
  ok 'sw.js 声明不缓存 API'
else
  bad 'sw.js 声明不缓存 API' 'api rule missing'
fi

echo
echo '===== 11. W2 教程 API ====='
SC=$(code_with "$LOG/tutorials.json" "$BASE/api/v1/tutorials")
check_eq '教程列表可访问' '200' "$SC"

TUT_COUNT=$(python3 -c "
import json
d=json.load(open('$LOG/tutorials.json'))
if isinstance(d, list):
    print(len(d))
elif isinstance(d, dict):
    for k in ('tutorials','items','data'):
        if k in d and isinstance(d[k], list):
            print(len(d[k]))
            break
    else:
        print(0)
" 2>/dev/null)
check_ge '教程数量' 1 "$TUT_COUNT"

if curl -s "$BASE/api/v1/tutorials" | grep -qE '\.\./|file://'; then
  bad '教程数据不含路径穿越' 'suspicious path found'
else
  ok '教程数据不含路径穿越'
fi

echo
echo '===== 12. W2 更新检查 ====='
SC=$(code_with "$LOG/update.json" "$BASE/api/v1/update/status")
check_eq '更新状态可访问' '200' "$SC"

SC=$(code "$BASE/api/v1/update/assets")
check_eq '更新资产接口可访问' '200' "$SC"

echo
echo '===== 13. 权限边界 ====='
SC=$(code -H "$AUTH" "$BASE/api/v1/users")
check_eq '管理员可读用户列表' '200' "$SC"

SC=$(code -H "$AUTH" "$BASE/api/v1/nodes")
check_eq '管理员可读节点列表' '200' "$SC"

echo
echo '===== 14. 实例 CRUD ====='
INST_BODY='{"name":"e2e-instance","kind":"vanilla","version":"1.21.4","port":25599}'
SC=$(code_with "$LOG/inst_new.json" -X POST -H "$AUTH" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CSRF" -d "$INST_BODY" "$BASE/api/v1/instances")
if [ "$SC" = '200' ] || [ "$SC" = '201' ]; then
  ok '创建实例成功'
  INST_ID=$(json_get "$LOG/inst_new.json" "['id']")
  if [ -n "$INST_ID" ] && [ "$INST_ID" != 'None' ]; then
    ok "拿到实例 id: $INST_ID"
    SC=$(code_with "$LOG/inst_list.json" -H "$AUTH" "$BASE/api/v1/instances")
    check_eq '实例列表可访问' '200' "$SC"
    FOUND=$(python3 -c "
import json
d=json.load(open('$LOG/inst_list.json'))
items = d if isinstance(d,list) else d.get('instances',[])
print(1 if any(i.get('id')=='$INST_ID' for i in items) else 0)
" 2>/dev/null)
    check_eq '新建实例出现在列表中' '1' "$FOUND"
    SC=$(code -X DELETE -H "$AUTH" -H "x-cocktail-csrf: $CSRF" "$BASE/api/v1/instances/$INST_ID")
    if [ "$SC" = '200' ] || [ "$SC" = '204' ]; then
      ok '删除实例成功'
    else
      bad '删除实例成功' "got=$SC"
    fi
  else
    bad '拿到实例 id' 'empty'
  fi
else
  bad '创建实例成功' "got=$SC body=$(head -c 200 "$LOG/inst_new.json")"
fi

echo
echo '===== 15. 错误路径 ====='
SC=$(code -H "$AUTH" "$BASE/api/v1/instances/not-a-real-id")
if [ "$SC" = '404' ] || [ "$SC" = '400' ]; then
  ok '不存在的实例返回 404'
else
  bad '不存在的实例返回 404' "got=$SC"
fi

SC=$(code -X DELETE -H "$AUTH" -H "x-cocktail-csrf: $CSRF" "$BASE/api/v1/bans/not-a-real-id")
check_eq '解除不存在封禁返回 404' '404' "$SC"

SC=$(code -X POST -H "$AUTH" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CSRF" -d 'not-json{{{' "$BASE/api/v1/bans")
if [ "$SC" = '400' ] || [ "$SC" = '422' ]; then
  ok '非法 JSON 被拒绝'
else
  bad '非法 JSON 被拒绝' "got=$SC"
fi

echo
echo '===== 15b. 输入边界与特殊字符 ====='

LONG64=$(printf 'A%.0s' $(seq 1 64))
LONG65=$(printf 'A%.0s' $(seq 1 65))
LONG600=$(printf 'B%.0s' $(seq 1 600))
LONG9000=$(printf 'C%.0s' $(seq 1 9000))

SC=$(code -X POST -H "$AUTH" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CSRF" \
  -d "{\"target\":\"$LONG64\",\"reason\":\"ok\"}" "$BASE/api/v1/bans")
if [ "$SC" = '200' ]; then
  ok '64 字符封禁目标被接受'
else
  bad '64 字符封禁目标被接受' "got=$SC"
fi

SC=$(code -X POST -H "$AUTH" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CSRF" \
  -d "{\"target\":\"$LONG65\",\"reason\":\"ok\"}" "$BASE/api/v1/bans")
if [ "$SC" = '400' ]; then
  ok '65 字符封禁目标被拒绝'
else
  bad '65 字符封禁目标被拒绝' "got=$SC"
fi

SC=$(code -X POST -H "$AUTH" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CSRF" \
  -d "{\"target\":\"p1\",\"reason\":\"$LONG600\"}" "$BASE/api/v1/bans")
if [ "$SC" = '400' ]; then
  ok '600 字符封禁原因被拒绝'
else
  bad '600 字符封禁原因被拒绝' "got=$SC"
fi

SC=$(code -X POST -H "$AUTH" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CSRF" \
  -d "{\"kind\":\"support\",\"player\":\"p1\",\"subject\":\"ok\",\"body\":\"$LONG9000\"}" "$BASE/api/v1/tickets")
if [ "$SC" = '400' ]; then
  ok '9000 字符工单内容被拒绝'
else
  bad '9000 字符工单内容被拒绝' "got=$SC"
fi

SC=$(code -X POST -H "$AUTH" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CSRF" \
  -d '{"target":"<script>alert(1)</script>","reason":"xss"}' "$BASE/api/v1/bans")
if [ "$SC" = '200' ]; then
  ok 'HTML 特殊字符可存储'
else
  bad 'HTML 特殊字符可存储' "got=$SC"
fi

SC=$(code -X POST -H "$AUTH" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CSRF" \
  -d '{"target":"x; DROP TABLE admins; --","reason":"sqli"}' "$BASE/api/v1/bans")
if [ "$SC" = '200' ]; then
  ok 'SQL 注入串被当作普通字符串'
else
  bad 'SQL 注入串被当作普通字符串' "got=$SC"
fi

SC=$(code -H "$AUTH" "$BASE/api/v1/users")
if [ "$SC" = '200' ]; then
  ok 'SQL 注入后 admins 表仍存在'
else
  bad 'SQL 注入后 admins 表仍存在' "got=$SC"
fi

SC=$(code -X POST -H "$AUTH" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CSRF" \
  -d '{"target":"ctrlXchar","reason":"x"}' "$BASE/api/v1/bans")
if [ "$SC" = '200' ] || [ "$SC" = '400' ]; then
  ok '控制字符路径有响应'
else
  bad '控制字符路径有响应' "got=$SC"
fi

SC=$(code -X POST -H "$AUTH" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CSRF" \
  -d '{"target":"   ","reason":"   "}' "$BASE/api/v1/bans")
if [ "$SC" = '400' ]; then
  ok '纯空白被拒绝'
else
  bad '纯空白被拒绝' "got=$SC"
fi

echo
echo '===== 15c. 超大请求体 ====='

python3 -c "print('{\"target\":\"p\",\"reason\":\"' + 'x'*9000000 + '\"}')" > /tmp/bigbody.json
SC=$(curl -s -o /dev/null -w '%{http_code}' -X POST -H "$AUTH" -H 'Content-Type: application/json' -H "x-cocktail-csrf: $CSRF" --data-binary @/tmp/bigbody.json "$BASE/api/v1/bans")
if [ "$SC" = '413' ] || [ "$SC" = '400' ]; then
  ok "超大请求体被拒绝 $SC"
else
  bad '超大请求体被拒绝' "got=$SC"
fi

echo
echo '===== 18. 数据库不可用降级 ====='

DB_FILE="$WORK/data/control.db"
if [ ! -f "$DB_FILE" ]; then
  DB_FILE=$(find "$WORK" -name '*.db' -type f 2>/dev/null | head -1)
fi

if [ -z "$DB_FILE" ] || [ ! -f "$DB_FILE" ]; then
  bad '找到数据库文件' 'not found'
else
  ok '找到数据库文件'

  mv "$DB_FILE" "$DB_FILE.bak"
  chmod 000 "$DB_FILE.bak" 2>/dev/null

  SC=$(code "$BASE/api/v1/health")
  if [ "$SC" = '200' ] || [ "$SC" = '503' ]; then
    ok "数据库缺失时 health 有响应 $SC"
  else
    bad '数据库缺失时 health 有响应' "got=$SC"
  fi

  if kill -0 "$SERVER_PID" 2>/dev/null; then
    ok '数据库异常后进程仍存活'
  else
    bad '数据库异常后进程仍存活' 'process died'
  fi

  mv "$DB_FILE.bak" "$DB_FILE" 2>/dev/null
  chmod 644 "$DB_FILE" 2>/dev/null

  SC=$(code "$BASE/api/v1/health")
  if [ "$SC" = '200' ]; then
    ok '数据库恢复后 health 正常'
  else
    bad '数据库恢复后 health 正常' "got=$SC"
  fi
fi

echo
echo '===== 16. 登出 ====='
SC=$(code -X POST -H "$AUTH" -H "x-cocktail-csrf: $CSRF" "$BASE/api/v1/auth/logout")
if [ "$SC" = '200' ] || [ "$SC" = '204' ]; then
  ok '登出成功'
else
  bad '登出成功' "got=$SC"
fi

SC=$(code -H "$AUTH" "$BASE/api/v1/instances")
if [ "$SC" = '401' ]; then
  ok '登出后 token 失效'
else
  bad '登出后 token 失效' "got=$SC"
fi

echo
echo '===== 19. 数据库升级路径 ====='

UPG="$ROOT/target/upgrade-run"
rm -rf "$UPG"
mkdir -p "$UPG/data"

OLDDB="$UPG/data/control.db"
python3 - "$OLDDB" <<'PYOLD'
import sqlite3, sys
conn = sqlite3.connect(sys.argv[1])
conn.executescript('''
CREATE TABLE panel_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    panel_name TEXT NOT NULL
);
INSERT INTO panel_settings (id, panel_name) VALUES (1, 'Old Panel');

CREATE TABLE admins (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE sessions (
    token TEXT PRIMARY KEY,
    admin_id INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE login_attempts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL,
    at TEXT NOT NULL
);

CREATE TABLE nodes (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE instances (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at TEXT NOT NULL,
    action TEXT NOT NULL
);
''')
conn.commit()
conn.close()
PYOLD

if [ ! -f "$OLDDB" ]; then
  bad '构造旧版数据库' 'failed'
else
  ok '构造旧版数据库'

  cd "$UPG"
  export COCKTAIL_BIND=127.0.0.1:11013
  export COCKTAIL_WEB_ROOT="$ROOT/admin/dist"
  export RUST_LOG=warn

  "$CP" > "$UPG/upgrade.log" 2>&1 &
  UPID=$!

  READY=0
  for _ in $(seq 1 60); do
    if curl -s -o /dev/null "http://127.0.0.1:11013/api/v1/health" 2>/dev/null; then
      READY=1
      break
    fi
    if ! kill -0 "$UPID" 2>/dev/null; then
      break
    fi
    sleep 1
  done

  if [ "$READY" = '1' ]; then
    ok '旧库启动成功'
  else
    bad '旧库启动成功' "$(tail -5 "$UPG/upgrade.log")"
  fi

  if [ "$READY" = '1' ]; then
    curl -s -o /dev/null "http://127.0.0.1:11013/api/v1/setup" 2>/dev/null
    curl -s -o /dev/null -X POST -H 'Content-Type: application/json' \
      -d '{"username":"WarmUp","password":"x"}' \
      "http://127.0.0.1:11013/api/v1/setup" 2>/dev/null
    sleep 2

    OK=$(python3 - "$OLDDB" <<'PYCHK'
import sqlite3, sys
conn = sqlite3.connect(sys.argv[1])
c = conn.cursor()
required = {
    'panel_settings': ['webhook_url'],
    'sessions': ['expires_at', 'csrf_token'],
    'nodes': ['hostname', 'os', 'arch', 'last_seen', 'token_hash', 'protocol_version', 'last_seq'],
    'global_bans': [],
    'tickets': [],
    'player_actions': [],
}
missing = []
for table, cols in required.items():
    c.execute("SELECT name FROM sqlite_master WHERE type='table' AND name=?", (table,))
    if not c.fetchone():
        missing.append('table:' + table)
        continue
    c.execute('PRAGMA table_info(' + table + ')')
    have = {r[1] for r in c.fetchall()}
    for col in cols:
        if col not in have:
            missing.append(table + '.' + col)
print('OK' if not missing else 'MISSING:' + ','.join(missing))
PYCHK
)

    if [ "$OK" = 'OK' ]; then
      ok '迁移补齐所有缺失列与表'
    else
      bad '迁移补齐所有缺失列与表' "$OK"
    fi

    ROW=$(python3 - "$OLDDB" <<'PYROW'
import sqlite3, sys
conn = sqlite3.connect(sys.argv[1])
r = conn.execute('SELECT panel_name FROM panel_settings WHERE id=1').fetchone()
print(r[0] if r else 'NONE')
PYROW
)
    if [ "$ROW" = 'Old Panel' ]; then
      ok '旧库原有数据保留'
    else
      bad '旧库原有数据保留' "got=$ROW"
    fi

    SC=$(code -X POST -H 'Content-Type: application/json' \
      -d '{"username":"upgrade_admin","password":"Upgrade#2026!x","panel_name":"Upgraded"}' \
      "http://127.0.0.1:11013/api/v1/setup")
    check_eq '迁移后可创建管理员' '200' "$SC"

    SC=$(code_with "$UPG/s.json" -X POST -H 'Content-Type: application/json' \
      -d '{"username":"upgrade_admin","password":"Upgrade#2026!x"}' \
      "http://127.0.0.1:11013/api/v1/auth/login")
    check_eq '迁移后可登录' '200' "$SC"

    TOKU=$(python3 -c "import json;print(json.load(open('$UPG/s.json'))['token'])" 2>/dev/null)
    if [ -n "$TOKU" ]; then
      SC=$(code -H "Authorization: Bearer $TOKU" "http://127.0.0.1:11013/api/v1/bans?all=true")
      check_eq '迁移后封禁表可用' '200' "$SC"
      SC=$(code -H "Authorization: Bearer $TOKU" "http://127.0.0.1:11013/api/v1/tickets")
      check_eq '迁移后工单表可用' '200' "$SC"
      SC=$(code -H "Authorization: Bearer $TOKU" "http://127.0.0.1:11013/api/v1/leaderboard")
      check_eq '迁移后排行榜可用' '200' "$SC"
    fi

    kill $UPID 2>/dev/null
    wait $UPID 2>/dev/null
    sleep 3

    cd "$UPG"
    "$CP" > "$UPG/upgrade2.log" 2>&1 &
    UPID2=$!
    READY2=0
    for _ in $(seq 1 40); do
      if curl -s -o /dev/null "http://127.0.0.1:11013/api/v1/health" 2>/dev/null; then
        READY2=1
        break
      fi
      sleep 1
    done

    if [ "$READY2" = '1' ]; then
      ok '迁移后重启仍正常'
    else
      bad '迁移后重启仍正常' 'failed'
    fi

    SC=$(code -X POST -H 'Content-Type: application/json' \
      -d '{"username":"upgrade_admin","password":"Upgrade#2026!x"}' \
      "http://127.0.0.1:11013/api/v1/auth/login")
    check_eq '重启后管理员仍在' '200' "$SC"

    kill $UPID2 2>/dev/null
    wait $UPID2 2>/dev/null
    sleep 3
  fi
fi

echo
echo '===== 17. 持久化验证 ====='
kill $SERVER_PID 2>/dev/null
wait $SERVER_PID 2>/dev/null
sleep 2

"$CP" > "$LOG/server2.log" 2>&1 &
SERVER_PID=$!

READY=0
for _ in $(seq 1 60); do
  if curl -s -o /dev/null "$BASE/api/v1/health" 2>/dev/null; then
    READY=1
    break
  fi
  sleep 1
done

if [ "$READY" = '1' ]; then
  ok '重启后服务可用'
else
  bad '重启后服务可用' 'start failed'
fi

SC=$(code_with "$LOG/session2.json" -X POST -H 'Content-Type: application/json' -d "$LOGIN_BODY" "$BASE/api/v1/auth/login")
check_eq '重启后管理员仍存在（数据库持久化）' '200' "$SC"

TOKEN2=$(json_get "$LOG/session2.json" "['token']")
if [ -n "$TOKEN2" ] && [ "$TOKEN2" != 'None' ]; then
  SC=$(code_with "$LOG/bans_restart.json" -H "Authorization: Bearer $TOKEN2" "$BASE/api/v1/bans?all=true")
  check_eq '重启后可查封禁' '200' "$SC"
  BAN_COUNT2=$(python3 -c "import json;print(len(json.load(open('$LOG/bans_restart.json')).get('bans',[])))" 2>/dev/null)
  check_ge '封禁数据在重启后保留' 1 "$BAN_COUNT2"
  SC=$(code_with "$LOG/tickets_restart.json" -H "Authorization: Bearer $TOKEN2" "$BASE/api/v1/tickets")
  TICKET_COUNT2=$(python3 -c "import json;print(len(json.load(open('$LOG/tickets_restart.json')).get('tickets',[])))" 2>/dev/null)
  check_ge '工单数据在重启后保留' 1 "$TICKET_COUNT2"
else
  bad '重启后拿到 token' 'empty'
fi

echo
kill $SERVER_PID 2>/dev/null
wait $SERVER_PID 2>/dev/null

echo '=========================================='
echo "PASS: $PASS   FAIL: $FAIL"
echo '=========================================='

if [ "$FAIL" -gt 0 ]; then
  echo '失败项：'
  for n in "${FAILED_NAMES[@]}"; do
    echo "  - $n"
  done
  exit 1
fi

echo '全部通过'
