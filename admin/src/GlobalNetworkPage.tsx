import { useEffect, useState } from 'react'
import {
  api,
  formatBps,
  formatBytes,
  type HostNetworkResponse,
  type HostNetSample,
} from './api'
import NetOpsPanel from './NetOpsPanel'

type Props = {
  onBack: () => void
  onOpenSettings: () => void
  onOpenInstance: (id: string) => void
  onError: (msg: string | null) => void
}

function Sparkline({ values, color }: { values: number[]; color: string }) {
  const w = 280
  const h = 56
  if (values.length < 2) {
    return <div className="spark-empty">采集中…</div>
  }
  const max = Math.max(...values, 0.001)
  const pts = values
    .map((v, i) => {
      const x = (i / (values.length - 1)) * w
      const y = h - (v / max) * (h - 4) - 2
      return `${x.toFixed(1)},${y.toFixed(1)}`
    })
    .join(' ')
  return (
    <svg className="sparkline" viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none">
      <polyline fill="none" stroke={color} strokeWidth="2" points={pts} />
    </svg>
  )
}

export default function GlobalNetworkPage({
  onBack,
  onOpenSettings,
  onOpenInstance,
  onError,
}: Props) {
  const [data, setData] = useState<HostNetworkResponse | null>(null)

  const load = async () => {
    try {
      setData(await api.hostNetwork())
      onError(null)
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    }
  }

  useEffect(() => {
    void load()
    const t = setInterval(() => void load(), 5000)
    return () => clearInterval(t)
  }, [])

  const live: HostNetSample | null = data?.live ?? null
  const history = data?.history ?? []
  const rx = history.map((s) => s.rx_bps ?? 0)
  const tx = history.map((s) => s.tx_bps ?? 0)
  const alerts = live?.alerts ?? []

  return (
    <div className="home-page">
      <div className="page-head">
        <div>
          <p className="page-eyebrow">Cocktail Manager</p>
          <h2 className="page-title">全局网络</h2>
          <p className="meta">主机全部网卡（不含 lo）· 约 15 分钟曲线 · 告警可推送到 QQ 机器人</p>
        </div>
        <div className="btn-row">
          <button type="button" className="btn btn-ghost" onClick={onOpenSettings}>
            <i className="fa fa-qq" /> QQ 机器人
          </button>
          <button type="button" className="btn btn-ghost" onClick={onBack}>
            <i className="fa fa-arrow-left" /> 返回主界面
          </button>
        </div>
      </div>

      {alerts.length > 0 && (
        <div className="net-alerts">
          {alerts.map((a) => (
            <p key={a}>
              <i className="fa fa-exclamation-triangle" /> {a}
            </p>
          ))}
        </div>
      )}

      <div className="stat-grid net-kpis">
        <div className="card-panel stat-card">
          <div>
            <p className="label">主机下行</p>
            <p className="value">{formatBps(live?.rx_bps)}</p>
            <p className="net-sub">峰值 {formatBps(live?.peak_rx_bps)}</p>
          </div>
          <i className="fa fa-arrow-down icon primary" />
        </div>
        <div className="card-panel stat-card">
          <div>
            <p className="label">主机上行</p>
            <p className="value">{formatBps(live?.tx_bps)}</p>
            <p className="net-sub">峰值 {formatBps(live?.peak_tx_bps)}</p>
          </div>
          <i className="fa fa-arrow-up icon primary" />
        </div>
        <div className="card-panel stat-card">
          <div>
            <p className="label">TCP 已建立</p>
            <p className="value">{live?.tcp_estab ?? '—'}</p>
            <p className="net-sub">TIME-WAIT {live?.time_wait ?? 0}</p>
          </div>
          <i className="fa fa-plug icon warning" />
        </div>
        <div className="card-panel stat-card">
          <div>
            <p className="label">SYN-RECV</p>
            <p className="value">{live?.syn_recv ?? '—'}</p>
            <p className="net-sub">
              丢包 {live?.drops ?? 0} · 错误 {live?.errors ?? 0}
            </p>
          </div>
          <i className="fa fa-shield icon warning" />
        </div>
      </div>

      <div className="grid-2">
        <div className="card-panel">
          <h3 className="card-title">主机流量</h3>
          <Sparkline values={rx} color="#2563eb" />
          <p className="net-legend">
            <span className="dot rx" /> 下行 {formatBps(live?.rx_bps)}
            <span className="dot tx" /> 上行 {formatBps(live?.tx_bps)}
          </p>
          <Sparkline values={tx} color="#059669" />
          <dl className="net-dl">
            <div>
              <dt>累计收</dt>
              <dd>{formatBytes(live?.rx_bytes)}</dd>
            </div>
            <div>
              <dt>累计发</dt>
              <dd>{formatBytes(live?.tx_bytes)}</dd>
            </div>
            <div>
              <dt>包速率</dt>
              <dd>
                {(live?.rx_pps ?? 0).toFixed(0)} / {(live?.tx_pps ?? 0).toFixed(0)} pps
              </dd>
            </div>
          </dl>
        </div>
        <div className="card-panel">
          <h3 className="card-title">网卡</h3>
          {(live?.nics ?? []).length === 0 ? (
            <p className="meta">暂无 /proc/net/dev 数据。</p>
          ) : (
            <table className="net-table">
              <thead>
                <tr>
                  <th>接口</th>
                  <th>↓</th>
                  <th>↑</th>
                  <th>丢包</th>
                </tr>
              </thead>
              <tbody>
                {(live?.nics ?? []).map((n) => (
                  <tr key={n.name}>
                    <td>
                      <code>{n.name}</code>
                    </td>
                    <td>{formatBps(n.rx_bps)}</td>
                    <td>{formatBps(n.tx_bps)}</td>
                    <td>{n.drops}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>

      <div className="card-panel net-panel">
        <h3 className="card-title">实例贡献</h3>
        {(live?.instances ?? []).length === 0 ? (
          <p className="meta">还没有实例。游戏端口流量会显示在这里。</p>
        ) : (
          <table className="net-table">
            <thead>
              <tr>
                <th>实例</th>
                <th>状态</th>
                <th>端口</th>
                <th>↓ / ↑</th>
                <th>连接</th>
              </tr>
            </thead>
            <tbody>
              {(live?.instances ?? []).map((row) => (
                <tr key={row.id}>
                  <td>
                    <button
                      type="button"
                      className="linkish"
                      onClick={() => onOpenInstance(row.id)}
                    >
                      {row.name}
                    </button>
                  </td>
                  <td>{row.status}</td>
                  <td>:{row.port}</td>
                  <td>
                    {formatBps(row.rx_bps)} / {formatBps(row.tx_bps)}
                  </td>
                  <td>
                    {row.connections} · {row.unique_ips} IP
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <NetOpsPanel onError={onError} />
    </div>
  )
}
