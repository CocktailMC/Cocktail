import { formatBps, formatBytes, type Instance, type MetricSample } from './api'
import NetOpsPanel from './NetOpsPanel'

type Props = {
  instance: Instance
  history: MetricSample[]
  running: boolean
  onBusy?: (v: boolean, label?: string) => void
  onError?: (msg: string | null) => void
}

const SCOPE_LABEL: Record<string, string> = {
  loopback: '本机',
  private: '内网',
  public: '公网',
}

function Sparkline({
  values,
  color,
}: {
  values: number[]
  color: string
}) {
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

export default function NetworkPage({
  instance,
  history,
  running,
  onBusy,
  onError,
}: Props) {
  const live = instance.last_metrics
  const series = history.length ? history : live ? [live] : []
  const rx = series.map((s) => s.net_rx_bps ?? 0)
  const tx = series.map((s) => s.net_tx_bps ?? 0)
  const conns = series.map((s) => s.net_connections ?? 0)
  const peers = live?.net_peers ?? []
  const alerts = live?.net_alerts ?? []
  const publicPeers = peers.filter((p) => p.scope === 'public').length

  return (
    <>
      <div className="page-head">
        <div>
          <p className="page-eyebrow">{instance.spec.name}</p>
          <h2 className="page-title">网络分析</h2>
          <p className="meta">
            端口 :{instance.spec.port}
            {live?.net_listen ? ` · 监听 ${live.net_listen}` : ''}
            {live?.net_source === 'container' ? ' · 容器网卡流量' : ' · 按游戏端口套接字统计'}
            {live?.net_ping_version ? ` · ${live.net_ping_version}` : ''}
          </p>
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
            <p className="label">下行</p>
            <p className="value">{running ? formatBps(live?.net_rx_bps) : '—'}</p>
            <p className="net-sub">峰值 {formatBps(live?.net_peak_rx_bps)}</p>
          </div>
          <i className="fa fa-arrow-down icon primary" />
        </div>
        <div className="card-panel stat-card">
          <div>
            <p className="label">上行</p>
            <p className="value">{running ? formatBps(live?.net_tx_bps) : '—'}</p>
            <p className="net-sub">峰值 {formatBps(live?.net_peak_tx_bps)}</p>
          </div>
          <i className="fa fa-arrow-up icon primary" />
        </div>
        <div className="card-panel stat-card">
          <div>
            <p className="label">TCP 已建立</p>
            <p className="value">{running ? String(live?.net_connections ?? 0) : '—'}</p>
            <p className="net-sub">{live?.net_unique_ips ?? 0} 个远程 IP</p>
          </div>
          <i className="fa fa-plug icon warning" />
        </div>
        <div className="card-panel stat-card">
          <div>
            <p className="label">Status ping</p>
            <p className="value">
              {live?.net_rtt_ms != null ? `${live.net_rtt_ms.toFixed(0)} ms` : '—'}
            </p>
            <p className="net-sub">
              {live?.net_ping_online != null
                ? `${live.net_ping_online}/${live.net_ping_max ?? '—'} 人（列表协议）`
                : '本机握手未通'}
            </p>
          </div>
          <i className="fa fa-heartbeat icon success" />
        </div>
      </div>

      <div className="grid-2">
        <div className="card-panel">
          <h3 className="card-title">流量</h3>
          <Sparkline values={rx} color="#2563eb" />
          <p className="net-legend">
            <span className="dot rx" /> 下行 {formatBps(live?.net_rx_bps)}
            <span className="dot tx" /> 上行 {formatBps(live?.net_tx_bps)}
          </p>
          <Sparkline values={tx} color="#059669" />
          <dl className="net-dl">
            <div>
              <dt>本次接管累计 ↓</dt>
              <dd>{formatBytes(live?.net_session_rx)}</dd>
            </div>
            <div>
              <dt>本次接管累计 ↑</dt>
              <dd>{formatBytes(live?.net_session_tx)}</dd>
            </div>
            <div>
              <dt>包速率</dt>
              <dd>
                {(live?.net_rx_pps ?? 0).toFixed(0)} / {(live?.net_tx_pps ?? 0).toFixed(0)} pps
              </dd>
            </div>
            <div>
              <dt>丢包 / 错误</dt>
              <dd>
                {live?.net_drops ?? 0} / {live?.net_errors ?? 0}
              </dd>
            </div>
          </dl>
        </div>
        <div className="card-panel">
          <h3 className="card-title">连接状态</h3>
          <Sparkline values={conns} color="#d97706" />
          <div className="net-states">
            <span>
              ESTAB <strong>{live?.net_connections ?? 0}</strong>
            </span>
            <span>
              SYN-RECV <strong>{live?.net_syn_recv ?? 0}</strong>
            </span>
            <span>
              TIME-WAIT <strong>{live?.net_time_wait ?? 0}</strong>
            </span>
            <span>
              FIN-WAIT <strong>{live?.net_fin_wait ?? 0}</strong>
            </span>
            <span>
              UDP :{instance.spec.port} <strong>{live?.net_udp ?? 0}</strong>
            </span>
            <span>
              公网 IP <strong>{publicPeers}</strong>
            </span>
          </div>
          <p className="meta">
            SYN-RECV 偏高通常是扫端口；UDP 计数包含 query / 基岩相关套接字。曲线约 6 分钟窗口。
          </p>
        </div>
      </div>

      <div className="card-panel net-panel">
        <h3 className="card-title">远程对端</h3>
        {peers.length === 0 ? (
          <p className="meta">
            {running
              ? '没有 ESTABLISHED TCP。服务器列表 ping 走短连接，不一定出现在此表。'
              : '服务器未运行。'}
          </p>
        ) : (
          <table className="net-table">
            <thead>
              <tr>
                <th>IP</th>
                <th>范围</th>
                <th>连接</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {peers.map((p) => (
                <tr key={p.ip}>
                  <td>
                    <code>{p.ip}</code>
                    {p.ipv6 ? <span className="net-badge">v6</span> : null}
                  </td>
                  <td
                    className={
                      p.scope === 'public' ? 'net-scope-public' : undefined
                    }
                  >
                    {SCOPE_LABEL[p.scope || ''] || p.scope || '—'}
                  </td>
                  <td>{p.connections}</td>
                  <td>
                    <button
                      type="button"
                      className="btn btn-ghost"
                      onClick={() => {
                        void navigator.clipboard.writeText(p.ip)
                      }}
                    >
                      复制
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <NetOpsPanel
        instanceId={instance.id}
        defaultPort={instance.spec.port}
        peerIps={peers.filter((p) => p.scope !== 'loopback').map((p) => p.ip)}
        onError={onError}
        onBusy={onBusy}
      />
    </>
  )
}
