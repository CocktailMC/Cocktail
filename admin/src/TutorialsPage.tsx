import { useEffect, useRef, useState } from 'react'
import { api, type TutorialView } from './api'

type Props = {
  onBack: () => void
  onError: (msg: string | null) => void
}

function fmtDuration(secs: number): string {
  const m = Math.floor(secs / 60)
  const s = secs % 60
  return `${m}:${String(s).padStart(2, '0')}`
}

export default function TutorialsPage({ onBack, onError }: Props) {
  const [items, setItems] = useState<TutorialView[]>([])
  const [active, setActive] = useState<TutorialView | null>(null)
  const [query, setQuery] = useState('')
  const [loading, setLoading] = useState(true)
  const videoRef = useRef<HTMLVideoElement | null>(null)
  const lastReport = useRef(0)

  const load = async (q?: string) => {
    setLoading(true)
    onError(null)
    try {
      const res = await api.listTutorials(q)
      setItems(res.tutorials)
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void load()
  }, [])

  const chapters = Array.from(new Set(items.map((i) => i.chapter)))

  const report = async (item: TutorialView, position: number, duration: number) => {
    if (!Number.isFinite(position) || position < 0) return
    try {
      const res = await api.reportTutorialProgress(item.id, Math.floor(position), Math.floor(duration))
      setItems((prev) =>
        prev.map((it) =>
          it.id === item.id
            ? { ...it, percent: res.percent, completed: res.completed }
            : it,
        ),
      )
    } catch {
      return
    }
  }

  const onTimeUpdate = () => {
    const v = videoRef.current
    const cur = active
    if (!v || !cur) return
    const now = Date.now()
    if (now - lastReport.current < 10000) return
    lastReport.current = now
    void report(cur, v.currentTime, v.duration || cur.duration_secs)
  }

  const onEnded = () => {
    const v = videoRef.current
    const cur = active
    if (!v || !cur) return
    void report(cur, v.duration || cur.duration_secs, v.duration || cur.duration_secs)
  }

  return (
    <div className="page-flow">
      <div className="page-head">
        <div>
          <p className="page-eyebrow">教程</p>
          <h2 className="page-title">视频教程</h2>
        </div>
        <button type="button" className="btn btn-ghost" onClick={onBack}>
          返回主界面
        </button>
      </div>

      <div className="card-panel">
        <div className="settings">
          <label>
            搜索
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') void load(query)
              }}
              placeholder="安装、备份、权限…"
            />
          </label>
          <button type="button" className="btn btn-primary" disabled={loading} onClick={() => void load(query)}>
            {loading ? '搜索中…' : '搜索'}
          </button>
        </div>
      </div>

      {active && (
        <div className="card-panel" style={{ marginTop: '1rem' }}>
          <div className="page-head" style={{ marginBottom: '0.5rem' }}>
            <div>
              <p className="page-eyebrow">{active.chapter}</p>
              <h3 className="card-title">{active.title}</h3>
            </div>
            <button type="button" className="btn btn-ghost" onClick={() => setActive(null)}>
              关闭
            </button>
          </div>
          {active.embedded ? (
            <iframe
              title={active.title}
              src={active.url ?? ''}
              allow="accelerometer; clipboard-write; encrypted-media; picture-in-picture"
              allowFullScreen
              style={{ width: '100%', aspectRatio: '16 / 9', border: 'none' }}
            />
          ) : active.url ? (
            <video
              ref={videoRef}
              src={active.url}
              controls
              preload="metadata"
              onTimeUpdate={onTimeUpdate}
              onEnded={onEnded}
              style={{ width: '100%', borderRadius: '0.5rem', background: '#000' }}
            />
          ) : (
            <p className="meta">该教程暂无可用视频源</p>
          )}
          <p className="meta" style={{ marginTop: '0.5rem' }}>
            {active.summary} · 时长 {fmtDuration(active.duration_secs)} · 已看 {active.percent}%
          </p>
        </div>
      )}

      <div className="card-panel" style={{ marginTop: '1rem' }}>
        <h3 className="card-title">播放列表</h3>
        {items.length === 0 ? (
          <p className="meta">{loading ? '加载中…' : '没有匹配的教程'}</p>
        ) : (
          chapters.map((ch) => (
            <div key={ch} style={{ marginTop: '0.75rem' }}>
              <p className="page-eyebrow">{ch}</p>
              <div className="table-wrap">
                <table className="data-table">
                  <thead>
                    <tr>
                      <th>标题</th>
                      <th>时长</th>
                      <th>进度</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {items
                      .filter((i) => i.chapter === ch)
                      .sort((a, b) => a.order - b.order)
                      .map((it) => (
                        <tr key={it.id}>
                          <td>
                            <strong>{it.title}</strong>
                            <div className="meta">{it.summary}</div>
                          </td>
                          <td className="meta">{fmtDuration(it.duration_secs)}</td>
                          <td>
                            {it.completed ? (
                              <span className="badge status-running">已看完</span>
                            ) : (
                              <span className="meta">{it.percent}%</span>
                            )}
                          </td>
                          <td>
                            <button
                              type="button"
                              className="btn btn-ghost"
                              onClick={() => {
                                setActive(it)
                                lastReport.current = 0
                              }}
                            >
                              播放
                            </button>
                          </td>
                        </tr>
                      ))}
                  </tbody>
                </table>
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  )
}
