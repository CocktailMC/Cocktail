import { useEffect, useState } from 'react'
import { api, type BoardEntry, type ProfileView } from './api'

type Props = {
  onBack: () => void
  onError: (msg: string | null) => void
}

function fmtHours(h: number): string {
  if (h < 1) return `${Math.round(h * 60)} 分钟`
  return `${h.toFixed(1)} 小时`
}

function fmtDate(iso?: string | null): string {
  if (!iso) return '—'
  const d = new Date(iso)
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleDateString('zh-CN')
}

export default function LeaderboardPage({ onBack, onError }: Props) {
  const [entries, setEntries] = useState<BoardEntry[]>([])
  const [limit, setLimit] = useState(20)
  const [loading, setLoading] = useState(true)
  const [profile, setProfile] = useState<ProfileView | null>(null)
  const [search, setSearch] = useState('')

  const load = async () => {
    setLoading(true)
    onError(null)
    try {
      const res = await api.leaderboard({ limit })
      setEntries(res.entries)
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void load()
  }, [limit])

  const openProfile = async (name: string) => {
    onError(null)
    try {
      setProfile(await api.playerProfile(name))
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    }
  }

  return (
    <div className="page-flow">
      <div className="page-head">
        <div>
          <p className="page-eyebrow">玩家社区</p>
          <h2 className="page-title">游玩排行榜</h2>
        </div>
        <button type="button" className="btn btn-ghost" onClick={onBack}>
          返回主界面
        </button>
      </div>

      <div className="card-panel">
        <div className="settings">
          <label>
            显示数量
            <select value={limit} onChange={(e) => setLimit(Number(e.target.value))}>
              <option value={10}>前 10</option>
              <option value={20}>前 20</option>
              <option value={50}>前 50</option>
              <option value={100}>前 100</option>
            </select>
          </label>
          <label>
            查玩家
            <input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="输入玩家名查看档案"
              onKeyDown={(e) => {
                if (e.key === 'Enter' && search.trim()) void openProfile(search.trim())
              }}
            />
          </label>
          <button
            type="button"
            className="btn btn-primary"
            disabled={!search.trim()}
            onClick={() => void openProfile(search.trim())}
          >
            查档案
          </button>
        </div>
      </div>

      {profile && (
        <div className="card-panel" style={{ marginTop: '1rem' }}>
          <div className="page-head" style={{ marginBottom: '0.5rem' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: '0.75rem' }}>
              <img
                src={profile.avatar}
                alt=""
                width={48}
                height={48}
                style={{ borderRadius: '0.5rem' }}
                onError={(e) => {
                  ;(e.currentTarget as HTMLImageElement).style.display = 'none'
                }}
              />
              <div>
                <p className="page-eyebrow">玩家档案</p>
                <h3 className="card-title">{profile.name}</h3>
              </div>
            </div>
            <button type="button" className="btn btn-ghost" onClick={() => setProfile(null)}>
              关闭
            </button>
          </div>
          <div className="kv-grid">
            <div>
              <span className="meta">累计游玩</span>
              <div>{fmtHours(profile.hours)}</div>
            </div>
            <div>
              <span className="meta">会话数</span>
              <div>{profile.sessions}</div>
            </div>
            <div>
              <span className="meta">首次加入</span>
              <div>{fmtDate(profile.first_seen)}</div>
            </div>
            <div>
              <span className="meta">最后在线</span>
              <div>{fmtDate(profile.last_seen)}</div>
            </div>
            <div>
              <span className="meta">风险评分</span>
              <div>{profile.risk_score} / 10</div>
            </div>
            <div>
              <span className="meta">封禁记录</span>
              <div>{profile.ban_count}</div>
            </div>
            <div>
              <span className="meta">管理操作</span>
              <div>{profile.action_count}</div>
            </div>
            <div>
              <span className="meta">UUID</span>
              <div className="mono">{profile.uuid ?? '—'}</div>
            </div>
          </div>
          {profile.instances.length > 0 && (
            <p className="meta" style={{ marginTop: '0.75rem' }}>
              所在实例：{profile.instances.join(' · ')}
            </p>
          )}
          {profile.geo.length > 0 && (
            <div style={{ marginTop: '0.75rem' }}>
              <p className="page-eyebrow">来源 IP</p>
              <ul className="home-help">
                {profile.geo.map((g) => (
                  <li key={g.ip}>
                    <span className="mono">{g.ip}</span> · {g.country}
                    {g.city && g.city !== g.country ? ` / ${g.city}` : ''}
                    {g.asn ? ` · ${g.asn}` : ''}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}

      <div className="card-panel" style={{ marginTop: '1rem' }}>
        <h3 className="card-title">游玩时长排行</h3>
        {loading ? (
          <p className="meta">加载中…</p>
        ) : entries.length === 0 ? (
          <p className="meta">还没有玩家数据，服务器记录到玩家后会自动出现</p>
        ) : (
          <div className="table-wrap">
            <table className="data-table">
              <thead>
                <tr>
                  <th>#</th>
                  <th>玩家</th>
                  <th>时长</th>
                  <th>首次加入</th>
                  <th>最后在线</th>
                  <th>覆盖实例</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {entries.map((e) => (
                  <tr key={e.name}>
                    <td>{e.rank}</td>
                    <td>
                      <div style={{ display: 'flex', alignItems: 'center', gap: '0.5rem' }}>
                        <img
                          src={e.avatar}
                          alt=""
                          width={24}
                          height={24}
                          style={{ borderRadius: '0.25rem' }}
                          onError={(ev) => {
                            ;(ev.currentTarget as HTMLImageElement).style.display = 'none'
                          }}
                        />
                        <strong>{e.name}</strong>
                      </div>
                    </td>
                    <td>{fmtHours(e.hours)}</td>
                    <td className="meta">{fmtDate(e.first_seen)}</td>
                    <td className="meta">{fmtDate(e.last_seen)}</td>
                    <td>{e.instances}</td>
                    <td>
                      <button type="button" className="link-btn" onClick={() => void openProfile(e.name)}>
                        档案
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  )
}
