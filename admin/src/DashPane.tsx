import type { Instance, PanelEvent } from './api'
import { formatBps } from './api'
import EventFeed from './EventFeed'

const STATUS: Record<string, string> = {
  created: '已创建',
  starting: '启动中',
  running: '运行中',
  stopping: '停止中',
  stopped: '已停止',
  crashed: '崩溃',
}

type Props = {
  selected: Instance
  events: PanelEvent[]
  onOpenNetwork: () => void
}

function cell(k: string, v: string, kind?: 'live' | 'dead' | 'bad') {
  return (
    <div className="sheet-cell">
      <p className="k">{k}</p>
      <p className={kind ? `v ${kind}` : 'v'}>{v}</p>
    </div>
  )
}

export default function DashPane({ selected, events, onOpenNetwork }: Props) {
  const live = selected.status === 'running'
  const m = selected.last_metrics
  const reasons = (selected.health_reasons ?? []).join(' · ') || '规则评分（非 AI）'

  if (selected.status === 'crashed') {
    return (
      <div className="spilled">
        <h2>洒了</h2>
        <p>
          {selected.spec.core}
          {selected.spec.mc_version ? ` ${selected.spec.mc_version}` : ''} 已停下。
          健康 {selected.health_score ?? '—'}%。
          {reasons ? ` ${reasons}。` : ''}
          这里没有假的活图——酒已经不在杯里。
        </p>
      </div>
    )
  }

  if (!live && selected.status !== 'starting') {
    return (
      <div className="empty-cup">
        <h2>空杯</h2>
        <p>
          {selected.spec.runtime === 'docker' ? 'docker' : 'process'}
          {' · '}
          {selected.spec.memory_mib} MiB · {STATUS[selected.status]}。
          液面不在，数字是破折号。
        </p>
      </div>
    )
  }

  return (
    <>
      <div className="sheet">
        {cell('TPS', m?.tps != null ? m.tps.toFixed(2) : '—', live && m?.tps != null ? 'live' : 'dead')}
        {cell(
          'MSPT',
          m?.mspt != null ? `${m.mspt.toFixed(1)} ms` : '—',
          m?.mspt == null ? 'dead' : undefined,
        )}
        {cell(
          '玩家',
          m?.players != null
            ? `${m.players}${m.players_max != null ? `/${m.players_max}` : ''}`
            : '—',
        )}
        {cell(
          'CPU',
          m?.cpu_pct != null ? `${m.cpu_pct.toFixed(1)}%` : '—',
          m?.cpu_pct == null ? 'dead' : undefined,
        )}
        {cell(
          '内存',
          m?.memory_mib != null
            ? `${m.memory_mib}/${selected.spec.memory_mib}`
            : `—/${selected.spec.memory_mib}`,
        )}
        {cell(
          '下行 / 上行',
          live ? `${formatBps(m?.net_rx_bps)} / ${formatBps(m?.net_tx_bps)}` : '—',
          live ? undefined : 'dead',
        )}
        {cell(
          '实体 / 区块',
          `${m?.entities ?? '—'} · ${m?.chunks ?? '—'}`,
        )}
        {cell(
          '堆 / GC',
          m?.heap_used_mib != null
            ? `${m.heap_used_mib.toFixed(0)}${m.heap_max_mib != null ? `/${m.heap_max_mib.toFixed(0)}` : ''} · ${m.gc_count ?? '—'}`
            : '—',
        )}
        {cell(
          'TCP / ping',
          live
            ? `${m?.net_connections ?? 0} · ${m?.net_rtt_ms != null ? `${m.net_rtt_ms.toFixed(0)} ms` : '—'}`
            : '—',
          live ? undefined : 'dead',
        )}
        {cell(
          '健康',
          selected.health_score != null ? `${selected.health_score}% · 规则评分` : '—',
        )}
        {cell(
          '热接管',
          selected.reattached && live
            ? `已接管${selected.pid ? ` · pid ${selected.pid}` : ''}`
            : '—',
          selected.reattached && live ? 'live' : 'dead',
        )}
        {cell('节点', selected.node_id ?? selected.spec.node_id ?? 'local')}
      </div>
      <p className="net-lead" style={{ marginTop: 14 }}>
        监听{' '}
        <strong className="mono">
          {m?.net_listen || `0.0.0.0:${selected.spec.port}`}
        </strong>
        {m?.net_unique_ips != null ? ` · ${m.net_unique_ips} IP` : ''}
        {' · '}
        <button type="button" className="link-btn" onClick={onOpenNetwork}>
          网络
        </button>
      </p>
      <div style={{ marginTop: 18 }}>
        <h3 className="card-title">事件</h3>
        <EventFeed
          events={events}
          names={new Map([[selected.id, selected.spec.name]])}
        />
      </div>
    </>
  )
}
