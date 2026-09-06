import { useEffect, useState } from 'react'
import {
  api,
  type NetopsRule,
  type NetopsStatus,
} from './api'

type Props = {
  instanceId?: string
  defaultPort?: number
  peerIps?: string[]
  onBusy?: (v: boolean, label?: string) => void
  onError?: (msg: string | null) => void
}

export default function NetOpsPanel({
  instanceId,
  defaultPort,
  peerIps = [],
  onBusy,
  onError,
}: Props) {
  const [status, setStatus] = useState<NetopsStatus | null>(null)
  const [cidr, setCidr] = useState('')
  const [verdict, setVerdict] = useState<'drop' | 'reject'>('drop')
  const [proto, setProto] = useState<'both' | 'tcp' | 'udp'>('both')
  const [ttl, setTtl] = useState(0)
  const [comment, setComment] = useState('')
  const [firewall, setFirewall] = useState(true)
  const [dropConns, setDropConns] = useState(true)
  const [gameBan, setGameBan] = useState(true)
  const [busy, setBusy] = useState(false)

  const load = async () => {
    try {
      setStatus(await api.netops())
      onError?.(null)
    } catch (e) {
      onError?.(e instanceof Error ? e.message : String(e))
    }
  }

  useEffect(() => {
    void load()
  }, [])

  const run = async (label: string, fn: () => Promise<unknown>) => {
    setBusy(true)
    onBusy?.(true, label)
    try {
      await fn()
      await load()
    } catch (e) {
      onError?.(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
      onBusy?.(false)
    }
  }

  const block = (ip: string) =>
    run(`拉黑 ${ip}…`, () =>
      api.createNetop({
        cidr: ip,
        verdict,
        proto,
        port: defaultPort,
        instance_id: instanceId,
        ttl_secs: ttl,
        comment: comment.trim() || undefined,
        firewall,
        drop_conns: dropConns,
        game_ban: gameBan,
      }),
    )

  const rules: NetopsRule[] = (status?.rules ?? []).filter((r) =>
    instanceId ? r.instance_id === instanceId || !r.instance_id : true,
  )

  return (
    <div className="card-panel net-panel">
      <div className="store-head" style={{ marginBottom: '0.6rem' }}>
        <div>
          <h3 className="card-title">
            <i className="fa fa-fire" /> 网络操作 / 防火墙
          </h3>
          <p className="meta">
            {status?.hint || '加载中…'}
            {status?.game_ports?.length
              ? ` · 游戏端口 ${status.game_ports.map((p) => `:${p}`).join(' ')}`
              : ''}
          </p>
        </div>
        <button
          type="button"
          className="btn btn-ghost"
          disabled={busy}
          onClick={() => run('同步防火墙…', () => api.resyncNetops())}
        >
          同步规则
        </button>
      </div>

      <div className="settings netops-form">
        <label>
          IP / CIDR
          <input
            value={cidr}
            onChange={(e) => setCidr(e.target.value)}
            placeholder="203.0.113.8 或 203.0.113.0/24"
          />
        </label>
        <label>
          动作
          <select
            value={verdict}
            onChange={(e) => setVerdict(e.target.value as 'drop' | 'reject')}
          >
            <option value="drop">丢弃（静默 DROP）</option>
            <option value="reject">拒绝（ICMP / 更快断开）</option>
          </select>
        </label>
        <label>
          协议
          <select
            value={proto}
            onChange={(e) => setProto(e.target.value as 'both' | 'tcp' | 'udp')}
          >
            <option value="both">TCP + UDP</option>
            <option value="tcp">仅 TCP</option>
            <option value="udp">仅 UDP</option>
          </select>
        </label>
        <label>
          有效期
          <select value={ttl} onChange={(e) => setTtl(Number(e.target.value))}>
            <option value={0}>永久（直到手动解除）</option>
            <option value={600}>10 分钟</option>
            <option value={3600}>1 小时</option>
            <option value={21600}>6 小时</option>
            <option value={86400}>24 小时</option>
          </select>
        </label>
        <label>
          备注
          <input
            value={comment}
            onChange={(e) => setComment(e.target.value)}
            placeholder="扫服 / 恶意流量…"
          />
        </label>
      </div>
      <div className="btn-row" style={{ margin: '0.65rem 0' }}>
        <label className="home-check">
          <input
            type="checkbox"
            checked={firewall}
            onChange={(e) => setFirewall(e.target.checked)}
          />
          防火墙
        </label>
        <label className="home-check">
          <input
            type="checkbox"
            checked={dropConns}
            onChange={(e) => setDropConns(e.target.checked)}
          />
          踢掉已建立连接
        </label>
        <label className="home-check">
          <input
            type="checkbox"
            checked={gameBan}
            onChange={(e) => setGameBan(e.target.checked)}
          />
          游戏 ban-ip
        </label>
      </div>
      <div className="btn-row">
        <button
          type="button"
          className="btn btn-primary"
          disabled={busy || !cidr.trim()}
          onClick={() => void block(cidr.trim())}
        >
          执行拉黑
        </button>
        <button
          type="button"
          className="btn btn-ghost"
          disabled={busy || !cidr.trim()}
          onClick={() =>
            run(`踢连接 ${cidr}…`, () =>
              api.kickNetops({
                cidr: cidr.trim(),
                port: defaultPort,
              }),
            )
          }
        >
          只踢连接
        </button>
      </div>

      {peerIps.length > 0 && (
        <p className="meta" style={{ marginTop: '0.75rem' }}>
          当前对端可一键拉黑：{' '}
          {peerIps.slice(0, 8).map((ip) => (
            <button
              key={ip}
              type="button"
              className="btn btn-ghost"
              disabled={busy}
              style={{ margin: '0.15rem' }}
              onClick={() => void block(ip)}
            >
              {ip}
            </button>
          ))}
        </p>
      )}

      {rules.length === 0 ? (
        <p className="meta" style={{ marginTop: '0.85rem' }}>
          还没有 Cocktail 防火墙规则。IPv4 最宽 /16，不会改 SSH 默认策略。
        </p>
      ) : (
        <table className="net-table" style={{ marginTop: '0.85rem' }}>
          <thead>
            <tr>
              <th>目标</th>
              <th>动作</th>
              <th>端口</th>
              <th>到期</th>
              <th>状态</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {rules.map((r) => (
              <tr key={r.id}>
                <td>
                  <code>{r.cidr}</code>
                  {r.comment ? <span className="net-sub"> {r.comment}</span> : null}
                </td>
                <td>
                  {r.verdict}/{r.proto}
                  {r.game_ban ? ' · 游戏' : ''}
                </td>
                <td>{r.port != null ? `:${r.port}` : '全部游戏端口'}</td>
                <td>
                  {r.expires_at
                    ? new Date(r.expires_at).toLocaleString()
                    : '永久'}
                </td>
                <td>
                  {r.applied ? (
                    <span className="ok-text">已生效</span>
                  ) : (
                    <span className="bad-text">{r.apply_error || '未写入内核'}</span>
                  )}
                </td>
                <td>
                  <button
                    type="button"
                    className="btn btn-danger"
                    disabled={busy}
                    onClick={() =>
                      run('解除拉黑…', () => api.deleteNetop(r.id))
                    }
                  >
                    解除
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  )
}
