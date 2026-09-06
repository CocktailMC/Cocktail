import type { PanelEvent } from './api'

type Props = {
  events: PanelEvent[]
  names?: Map<string, string>
  onOpenInstance?: (id: string) => void
}

export default function EventFeed({ events, names, onOpenInstance }: Props) {
  if (events.length === 0) {
    return <p className="empty">暂无事件</p>
  }
  return (
    <ul className="event-feed">
      {events.map((ev) => (
        <li key={ev.id} className={`event-item event-${ev.level}`}>
          <span className="event-mark" aria-hidden>
            {ev.level === 'ok' ? '✓' : '⚠'}
          </span>
          <div>
            <strong>{ev.title}</strong>
            {ev.detail ? <span className="meta"> {ev.detail}</span> : null}
            <div className="meta">
              {new Date(ev.at).toLocaleString('zh-CN', { hour12: false })}
              {ev.instance_id
                ? onOpenInstance
                  ? (
                      <>
                        {' · '}
                        <button
                          type="button"
                          className="link-btn"
                          onClick={() => onOpenInstance(ev.instance_id!)}
                        >
                          {names?.get(ev.instance_id) ?? ev.instance_id.slice(0, 8)}
                        </button>
                      </>
                    )
                  : ` · ${ev.instance_id.slice(0, 8)}`
                : null}
            </div>
          </div>
        </li>
      ))}
    </ul>
  )
}
