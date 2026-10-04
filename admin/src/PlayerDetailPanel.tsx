import { useEffect, useState } from 'react'
import { api } from './api'

type Detail = Awaited<ReturnType<typeof api.playerDetail>>

type Props = {
  instanceId: string
  player: string
  running: boolean
  onClose: () => void
  onError: (msg: string | null) => void
}

const ACTIONS: [string, string][] = [
  ['kick', '踢出'],
  ['ban', '封禁'],
  ['pardon', '解封'],
  ['op', '给 op'],
  ['deop', '撤 op'],
  ['whitelist', '加白名单'],
  ['unwhitelist', '移出白名单'],
  ['gamemode', '切换模式'],
  ['give', '给物品'],
  ['teleport', '传送'],
  ['effect', '给药水效果'],
  ['kill', '击杀'],
  ['clear', '清空背包'],
]

function buildCommand(action: string, player: string, arg: string): string {
  const extra = arg.trim()
  switch (action) {
    case 'kick':
      return `kick ${player}${extra ? ` ${extra}` : ''}`
    case 'ban':
      return `ban ${player}${extra ? ` ${extra}` : ''}`
    case 'pardon':
      return `pardon ${player}`
    case 'op':
      return `op ${player}`
    case 'deop':
      return `deop ${player}`
    case 'whitelist':
      return `whitelist add ${player}`
    case 'unwhitelist':
      return `whitelist remove ${player}`
    case 'gamemode':
      return `gamemode ${extra || 'survival'} ${player}`
    case 'give':
      return `give ${player} ${extra || 'minecraft:stone 1'}`
    case 'teleport':
      return `tp ${player} ${extra || '~ ~ ~'}`
    case 'effect':
      return `effect give ${player} ${extra || 'minecraft:speed 30 1'}`
    case 'kill':
      return `kill ${player}`
    case 'clear':
      return `clear ${player}`
    default:
      return ''
  }
}

export default function PlayerDetailPanel({
  instanceId,
  player,
  running,
  onClose,
  onError,
}: Props) {
  const [detail, setDetail] = useState<Detail | null>(null)
  const [busy, setBusy] = useState(false)
  const [action, setAction] = useState('kick')
  const [arg, setArg] = useState('')
  const [raw, setRaw] = useState('')
  const [output, setOutput] = useState<string[]>([])

  const load = async () => {
    try {
      setDetail(await api.playerDetail(instanceId, player))
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    }
  }

  useEffect(() => {
    void load()
  }, [instanceId, player])

  const exec = async (command: string) => {
    if (!command) return
    setBusy(true)
    onError(null)
    try {
      const res = await api.playerRconAction(instanceId, player, command)
      setOutput((prev) => [
        `$ ${command}`,
        res.response || '(空响应)',
        ...prev,
      ].slice(0, 40))
      await load()
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const caps = detail?.rcon_capabilities ?? []

  return (
    <div className="card-panel" style={{ marginTop: '1rem' }}>
      <div className="page-head" style={{ marginBottom: '0.5rem' }}>
        <div>
          <p className="page-eyebrow">玩家详情</p>
          <h3 className="card-title">{player}</h3>
        </div>
        <button type="button" className="btn btn-ghost" onClick={onClose}>
          关闭
        </button>
      </div>
      {detail ? (
        <>
          <div className="kv-grid">
            <div>
              <span className="meta">UUID</span>
              <div className="mono">{detail.info.uuid ?? '—'}</div>
            </div>
            <div>
              <span className="meta">在线</span>
              <div>{detail.info.online ? '是' : '否'}</div>
            </div>
            <div>
              <span className="meta">本次会话</span>
              <div>{detail.info.session_secs ?? 0} 秒</div>
            </div>
            <div>
              <span className="meta">累计时长</span>
              <div>{detail.info.total_secs ?? 0} 秒</div>
            </div>
            <div>
              <span className="meta">OP</span>
              <div>{detail.ops.includes(player) ? '是' : '否'}</div>
            </div>
            <div>
              <span className="meta">白名单</span>
              <div>{detail.whitelist.includes(player) ? '在' : '不在'}</div>
            </div>
            <div>
              <span className="meta">封禁</span>
              <div>{detail.banned_players.includes(player) ? '已封禁' : '正常'}</div>
            </div>
            <div>
              <span className="meta">IP 封禁条目</span>
              <div>{detail.banned_ips.length}</div>
            </div>
          </div>
          <p className="meta" style={{ marginTop: '0.75rem' }}>
            可执行动作：{caps.length ? caps.join(' / ') : '（服务器未运行，无法探测）'}
          </p>
          <div className="settings" style={{ marginTop: '0.75rem' }}>
            <label>
              动作
              <select
                value={action}
                onChange={(e) => setAction(e.target.value)}
                disabled={busy || !running}
              >
                {ACTIONS.filter(([a]) => !caps.length || caps.includes(a)).map(
                  ([a, label]) => (
                    <option key={a} value={a}>
                      {label}
                    </option>
                  ),
                )}
              </select>
            </label>
            <label>
              参数
              <input
                value={arg}
                onChange={(e) => setArg(e.target.value)}
                placeholder="可选，如 survival / minecraft:stone 64"
                disabled={busy || !running}
              />
            </label>
            <button
              type="button"
              className="btn btn-primary"
              disabled={busy || !running}
              onClick={() => void exec(buildCommand(action, player, arg))}
            >
              执行
            </button>
          </div>
          <div className="settings" style={{ marginTop: '0.5rem' }}>
            <label>
              原始 RCON 命令
              <input
                value={raw}
                onChange={(e) => setRaw(e.target.value)}
                placeholder={`list`}
                disabled={busy || !running}
              />
            </label>
            <button
              type="button"
              className="btn btn-ghost"
              disabled={busy || !running || !raw.trim()}
              onClick={() => void exec(raw.trim())}
            >
              发送
            </button>
          </div>
          {output.length > 0 && (
            <pre className="console" style={{ whiteSpace: 'pre-wrap', marginTop: '0.75rem' }}>
              {output.join('\n')}
            </pre>
          )}
        </>
      ) : (
        <p className="meta">加载中…</p>
      )}
    </div>
  )
}
