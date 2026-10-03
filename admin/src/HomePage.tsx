import type { HealthInfo, Instance, InstanceStatus, PanelEvent } from './api'
import { formatBps } from './api'
import EventFeed from './EventFeed'

const STATUS_LABEL: Record<InstanceStatus, string> = {
  created: '已创建',
  starting: '启动中',
  running: '运行中',
  stopping: '停止中',
  stopped: '已停止',
  crashed: '崩溃',
}

type Fleet = {
  total: number
  running: number
  stopped: number
  starting: number
  crashed: number
  docker: { available: boolean; message: string }
}

type Props = {
  instances: Instance[]
  fleet: Fleet | null
  health: string
  env: HealthInfo | null
  authRequired: boolean
  busy: boolean
  selectedIds: string[]
  events: PanelEvent[]
  onToggleSelect: (id: string, checked: boolean) => void
  onSelectAll: (checked: boolean) => void
  onOpenInstance: (id: string) => void
  onCreate: () => void
  onStart: (id: string) => void
  onStop: (id: string) => void
  onRestart: (id: string) => void
  onBulk: (action: 'start' | 'stop' | 'restart' | 'delete') => void
  onOpenSettings: () => void
}

export default function HomePage(props: Props) {
  const {
    instances,
    fleet,
    health,
    env,
    authRequired,
    busy,
    selectedIds,
    events,
    onToggleSelect,
    onSelectAll,
    onOpenInstance,
    onCreate,
    onStart,
    onStop,
    onRestart,
    onBulk,
  } = props
  const allSelected =
    instances.length > 0 && selectedIds.length === instances.length
  const crashed = fleet?.crashed ?? 0

  return (
    <div className="home-page">
      <div className="page-head">
        <div>
          <p className="page-eyebrow">控制面</p>
          <h2 className="page-title">机群总览</h2>
        </div>
      </div>

      <p className="plane-facts">
        {health === 'offline' ? '离线' : '在线'}
        {env?.version ? ` · v${env.version}` : ''}
        {env?.release ? ` · ${env.release}` : ''}
        {' · '}
        {authRequired ? '鉴权已启用' : '鉴权关闭'}
        {' · '}
        Docker {fleet?.docker.available ? '就绪' : '不可用'}
        {env?.hostname ? ` · ${env.hostname}` : ''}
        {' · '}
        插件源 Modrinth · Hangar · Spiget
      </p>
      {!fleet?.docker.available && fleet?.docker.message ? (
        <p className="meta">{fleet.docker.message}</p>
      ) : null}

      <div className="sheet" style={{ marginBottom: '1.25rem' }}>
        <div className="sheet-cell">
          <p className="k">运行</p>
          <p className={fleet?.running ? 'v live' : 'v'}>
            {fleet?.running ?? 0}
          </p>
        </div>
        <div className="sheet-cell">
          <p className="k">停止</p>
          <p className="v">{fleet?.stopped ?? 0}</p>
        </div>
        <div className="sheet-cell">
          <p className="k">崩溃</p>
          <p className={crashed > 0 ? 'v bad' : 'v'}>{crashed}</p>
        </div>
        <div className="sheet-cell">
          <p className="k">总计</p>
          <p className="v">{fleet?.total ?? instances.length}</p>
        </div>
      </div>

      <div className="card-panel" style={{ marginBottom: '1.25rem' }}>
        <h3 className="card-title">事件</h3>
        <EventFeed
          events={events}
          names={new Map(instances.map((i) => [i.id, i.spec.name]))}
          onOpenInstance={onOpenInstance}
        />
      </div>

      <div className="card-panel">
        <div className="home-servers-head">
          <h3 className="card-title" style={{ margin: 0 }}>
            杯子
          </h3>
          <div className="home-servers-tools">
            <label className="home-check">
              <input
                type="checkbox"
                checked={allSelected}
                onChange={(e) => onSelectAll(e.target.checked)}
                disabled={!instances.length}
              />
              全选
            </label>
            <button
              type="button"
              className="btn btn-ghost"
              disabled={!selectedIds.length || busy}
              onClick={() => onBulk('start')}
            >
              批量启动
            </button>
            <button
              type="button"
              className="btn btn-ghost"
              disabled={!selectedIds.length || busy}
              onClick={() => onBulk('stop')}
            >
              批量停止
            </button>
            <button
              type="button"
              className="btn btn-ghost"
              disabled={!selectedIds.length || busy}
              onClick={() => onBulk('restart')}
            >
              批量重启
            </button>
            <button
              type="button"
              className="btn btn-danger"
              disabled={!selectedIds.length || busy}
              onClick={() => onBulk('delete')}
            >
              批量删除
            </button>
          </div>
        </div>

        {instances.length === 0 ? (
          <div className="empty-cup" style={{ marginTop: '0.75rem' }}>
            <h2>还没有杯子</h2>
            <p>创建后会出现在左边那一轨。批量启停也在这一页。</p>
            <div className="power-row" style={{ marginTop: 16, marginBottom: 0 }}>
              <button type="button" className="power primary" onClick={onCreate}>
                创建实例
              </button>
            </div>
          </div>
        ) : (
          <table className="data-table" style={{ marginTop: '0.75rem' }}>
            <thead>
              <tr>
                <th />
                <th>名称</th>
                <th>状态</th>
                <th>TPS</th>
                <th>玩家</th>
                <th>内存</th>
                <th>网络</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {instances.map((inst) => {
                const running = inst.status === 'running'
                const mem = inst.last_metrics?.memory_mib
                const players = inst.last_metrics?.players
                const tps = inst.last_metrics?.tps
                return (
                  <tr key={inst.id}>
                    <td>
                      <input
                        type="checkbox"
                        checked={selectedIds.includes(inst.id)}
                        onChange={(e) =>
                          onToggleSelect(inst.id, e.target.checked)
                        }
                      />
                    </td>
                    <td>
                      <button
                        type="button"
                        className="link-btn"
                        onClick={() => onOpenInstance(inst.id)}
                      >
                        {inst.spec.name}
                      </button>
                      <div className="meta">
                        {inst.spec.core}
                        {' · '}
                        <span className="mono">:{inst.spec.port}</span>
                        {' · '}
                        {inst.spec.runtime === 'docker' ? 'docker' : 'process'}
                        {inst.reattached && running ? ' · 已接管' : ''}
                      </div>
                    </td>
                    <td className={inst.status === 'crashed' ? 'v bad' : undefined}>
                      {STATUS_LABEL[inst.status]}
                    </td>
                    <td className="mono">
                      {running && tps != null ? tps.toFixed(2) : '—'}
                    </td>
                    <td className="mono">{running ? (players ?? 0) : '—'}</td>
                    <td className="mono">
                      {running && mem != null
                        ? `${mem}/${inst.spec.memory_mib}`
                        : `—/${inst.spec.memory_mib}`}
                    </td>
                    <td className="mono">
                      {running
                        ? `${formatBps(inst.last_metrics?.net_rx_bps)} ↓`
                        : '—'}
                    </td>
                    <td className="actions">
                      <button
                        type="button"
                        className="link-btn"
                        disabled={busy || running}
                        onClick={() => onStart(inst.id)}
                      >
                        启动
                      </button>
                      <button
                        type="button"
                        className="link-btn"
                        disabled={busy || !running}
                        onClick={() => onStop(inst.id)}
                      >
                        停止
                      </button>
                      <button
                        type="button"
                        className="link-btn"
                        disabled={busy}
                        onClick={() => onRestart(inst.id)}
                      >
                        重启
                      </button>
                      <button
                        type="button"
                        className="link-btn"
                        onClick={() => onOpenInstance(inst.id)}
                      >
                        进入
                      </button>
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        )}
      </div>
    </div>
  )
}
