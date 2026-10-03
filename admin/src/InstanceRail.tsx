import type { Instance, InstanceStatus, PanelEvent } from './api'

const STATUS_LABEL: Record<InstanceStatus, string> = {
  created: '已创建',
  starting: '启动中',
  running: '运行中',
  stopping: '停止中',
  stopped: '已停止',
  crashed: '崩溃',
}

type Props = {
  instances: Instance[]
  selectedId: string | null
  fleet: {
    running: number
    stopped: number
    crashed: number
    starting: number
  } | null
  events: PanelEvent[]
  names: Map<string, string>
  onSelect: (id: string) => void
  onCreate: () => void
}

function fillWidth(inst: Instance): number {
  if (inst.status === 'stopped' || inst.status === 'created') return 0
  const used = inst.last_metrics?.memory_mib
  const total = inst.spec.memory_mib
  if (used == null || !total) {
    return inst.status === 'running' ? 40 : 12
  }
  return Math.max(4, Math.min(100, (used / total) * 100))
}

export default function InstanceRail({
  instances,
  selectedId,
  fleet,
  events,
  names,
  onSelect,
  onCreate,
}: Props) {
  const crashed = fleet?.crashed ?? instances.filter((i) => i.status === 'crashed').length
  const running = fleet?.running ?? instances.filter((i) => i.status === 'running').length
  const stopped = fleet?.stopped ?? instances.filter((i) => i.status === 'stopped' || i.status === 'created').length

  return (
    <aside className="rail" aria-label="机群">
      <div className="fleet-head">
        <div className="fleet-head-row">
          <h2 className="fleet-title">机群</h2>
          <button type="button" className="create-btn" onClick={onCreate}>
            创建实例
          </button>
        </div>
        <p className="fleet-counts">
          运行 <strong>{running}</strong>
          {' · '}停止 <strong>{stopped}</strong>
          {' · '}崩溃{' '}
          <strong className={crashed > 0 ? 'crash-n' : undefined}>{crashed}</strong>
        </p>
      </div>

      <div className="rail-list">
        {instances.length === 0 ? (
          <p className="rail-empty">还没有杯子。创建后会出现在这一轨。</p>
        ) : (
          instances.map((inst) => {
            const tps = inst.last_metrics?.tps
            const live = inst.status === 'running'
            const crash = inst.status === 'crashed'
            return (
              <button
                key={inst.id}
                type="button"
                className={inst.id === selectedId ? 'inst is-current' : 'inst'}
                data-status={inst.status}
                aria-current={inst.id === selectedId ? 'true' : undefined}
                onClick={() => onSelect(inst.id)}
              >
                {crash ? (
                  <span className="crash-mark" aria-hidden />
                ) : (
                  <span className="inst-dot" aria-hidden />
                )}
                <span className="inst-name">{inst.spec.name}</span>
                <span className="inst-spark">
                  {live && tps != null ? (
                    <>
                      <span className="spark-bars" aria-hidden>
                        <i style={{ height: 10 }} />
                        <i style={{ height: 12 }} />
                        <i style={{ height: 14 }} />
                        <i style={{ height: 16 }} />
                        <i style={{ height: 13 }} />
                      </span>
                      {tps.toFixed(2)}
                    </>
                  ) : crash ? (
                    '!'
                  ) : (
                    '—'
                  )}
                </span>
                <span className="inst-sub">
                  :{inst.spec.port} · {inst.spec.core}
                  {crash ? ` · ${STATUS_LABEL.crashed}` : ''}
                  {inst.reattached && live ? ' · 已接管' : ''}
                </span>
                <span className="inst-fill" aria-hidden>
                  <span style={{ width: `${fillWidth(inst)}%` }} />
                </span>
              </button>
            )
          })
        )}
      </div>

      <div className="events-rail">
        <p>事件</p>
        {events.length === 0 ? (
          <p className="rail-empty">暂无事件</p>
        ) : (
          <ul>
            {events.slice(0, 4).map((ev) => (
              <li key={ev.id} className={ev.level === 'ok' ? undefined : 'bad'}>
                <b>
                  {ev.instance_id
                    ? names.get(ev.instance_id) ?? ev.instance_id.slice(0, 8)
                    : '控制面'}
                </b>{' '}
                {ev.title}
              </li>
            ))}
          </ul>
        )}
      </div>
    </aside>
  )
}
