import { useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { api, type ActionRow, type BanRow, type TicketRow } from './api'

type Props = {
  onBack: () => void
  onError: (msg: string | null) => void
}

const KIND_LABEL: Record<string, string> = {
  appeal: '封禁申诉',
  whitelist: '白名单申请',
  report: '玩家举报',
  support: '求助',
}

const STATE_LABEL: Record<string, string> = {
  open: '待处理',
  claimed: '处理中',
  approved: '已通过',
  rejected: '已驳回',
  closed: '已关闭',
}

function fmt(iso: string): string {
  if (!iso) return '—'
  const d = new Date(iso)
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString('zh-CN', { hour12: false })
}

export default function CommunityPage({ onBack, onError }: Props) {
  const [tab, setTab] = useState<'bans' | 'tickets' | 'actions'>('bans')
  const [bans, setBans] = useState<BanRow[]>([])
  const [tickets, setTickets] = useState<TicketRow[]>([])
  const [actions, setActions] = useState<ActionRow[]>([])
  const [busy, setBusy] = useState(false)
  const [openOnly, setOpenOnly] = useState(true)
  const [stateFilter, setStateFilter] = useState<string>('all')
  const visibleTickets =
    stateFilter === 'all' ? tickets : tickets.filter((t) => t.state === stateFilter)
  const [target, setTarget] = useState('')
  const [reason, setReason] = useState('')
  const [hours, setHours] = useState('')
  const [expiresAt, setExpiresAt] = useState('')
  const [banKind, setBanKind] = useState('player')
  const [searchTarget, setSearchTarget] = useState('')

  const loadBans = async () => {
    try {
      const res = await api.listBans({ all: true })
      setBans(res.bans)
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    }
  }

  const loadTickets = async () => {
    try {
      const res = await api.listTickets({ open: openOnly })
      setTickets(res.tickets)
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    }
  }

  const loadActions = async () => {
    try {
      setActions(await api.listPlayerActions({ target: searchTarget || undefined, limit: 200 }))
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    }
  }

  useEffect(() => {
    void loadBans()
    void loadTickets()
  }, [])

  useEffect(() => {
    if (tab === 'actions') void loadActions()
  }, [tab])

  const submitBan = async (e: FormEvent) => {
    e.preventDefault()
    if (!target.trim() || !reason.trim()) return
    setBusy(true)
    onError(null)
    try {
      await api.createBan({
        target: target.trim(),
        reason: reason.trim(),
        kind: banKind,
        hours: hours.trim() ? Number(hours.trim()) : undefined,
        expires_at: expiresAt ? new Date(expiresAt).toISOString() : undefined,
      })
      setTarget('')
      setReason('')
      setHours('')
      await loadBans()
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(false)
    }
  }

  const lift = async (id: string) => {
    if (!window.confirm('确定解除这条封禁？')) return
    setBusy(true)
    try {
      await api.liftBan(id)
      await loadBans()
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(false)
    }
  }

  const act = async (fn: () => Promise<void>) => {
    setBusy(true)
    onError(null)
    try {
      await fn()
      await loadTickets()
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="page-flow">
      <div className="page-head">
        <div>
          <p className="page-eyebrow">玩家社区</p>
          <h2 className="page-title">封禁与工单</h2>
        </div>
        <button type="button" className="btn btn-ghost" onClick={onBack}>
          返回主界面
        </button>
      </div>

      <div className="chip-row">
        <button
          type="button"
          className={tab === 'bans' ? 'btn btn-primary' : 'btn btn-ghost'}
          onClick={() => setTab('bans')}
        >
          全局封禁 ({bans.filter((b) => b.active).length})
        </button>
        <button
          type="button"
          className={tab === 'tickets' ? 'btn btn-primary' : 'btn btn-ghost'}
          onClick={() => setTab('tickets')}
        >
          工单 ({tickets.filter((t) => t.state === 'open' || t.state === 'claimed').length})
        </button>
        <button
          type="button"
          className={tab === 'actions' ? 'btn btn-primary' : 'btn btn-ghost'}
          onClick={() => setTab('actions')}
        >
          行为记录
        </button>
      </div>

      {tab === 'bans' && (
        <>
          <div className="card-panel" style={{ marginTop: '1rem' }}>
            <h3 className="card-title">新增封禁</h3>
            <form className="settings" onSubmit={submitBan}>
              <label>
                目标
                <input
                  value={target}
                  onChange={(e) => setTarget(e.target.value)}
                  placeholder="玩家名 / IP / UUID"
                  disabled={busy}
                />
              </label>
              <label>
                类型
                <select value={banKind} onChange={(e) => setBanKind(e.target.value)} disabled={busy}>
                  <option value="player">玩家</option>
                  <option value="ip">IP</option>
                  <option value="uuid">UUID</option>
                </select>
              </label>
              <label>
                时长（小时，留空永久）
                <input
                  value={hours}
                  onChange={(e) => setHours(e.target.value)}
                  placeholder="24"
                  inputMode="numeric"
                  disabled={busy}
                />
              </label>
              <label>
                到期时间（可选，优先于时长）
                <input
                  type="datetime-local"
                  value={expiresAt}
                  onChange={(e) => setExpiresAt(e.target.value)}
                  disabled={busy}
                />
              </label>
              <label>
                原因
                <input
                  value={reason}
                  onChange={(e) => setReason(e.target.value)}
                  placeholder="破坏规则"
                  disabled={busy}
                />
              </label>
              <button type="submit" className="btn btn-primary" disabled={busy}>
                封禁
              </button>
            </form>
          </div>

          <div className="card-panel" style={{ marginTop: '1rem' }}>
            <h3 className="card-title">封禁列表</h3>
            {bans.length === 0 ? (
              <p className="meta">还没有封禁记录</p>
            ) : (
              <div className="table-wrap">
                <table className="data-table">
                  <thead>
                    <tr>
                      <th>目标</th>
                      <th>类型</th>
                      <th>原因</th>
                      <th>操作人</th>
                      <th>时间</th>
                      <th>到期</th>
                      <th>状态</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {bans.map((b) => (
                      <tr key={b.id}>
                        <td>
                          <strong>{b.target}</strong>
                        </td>
                        <td>{b.target_kind}</td>
                        <td>{b.reason}</td>
                        <td>{b.banned_by}</td>
                        <td className="meta">{fmt(b.banned_at)}</td>
                        <td className="meta">{b.expires_at ? fmt(b.expires_at) : '永久'}</td>
                        <td>
                          <span className={`badge status-${b.active ? 'running' : 'stopped'}`}>
                            {b.active ? '生效中' : '已解除'}
                          </span>
                        </td>
                        <td>
                          {b.active && (
                            <button
                              type="button"
                              className="link-btn"
                              disabled={busy}
                              onClick={() => void lift(b.id)}
                            >
                              解除
                            </button>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </div>
        </>
      )}

      {tab === 'tickets' && (
        <div className="card-panel" style={{ marginTop: '1rem' }}>
          <div className="page-head" style={{ marginBottom: '0.5rem' }}>
            <h3 className="card-title">工单</h3>
            <label className="meta">
              <input
                type="checkbox"
                checked={openOnly}
                onChange={(e) => {
                  setOpenOnly(e.target.checked)
                  setStateFilter('all')
                  void loadTickets()
                }}
              />{' '}
              只看未结束
            </label>
          </div>
          <div
            className="row-actions"
            style={{ flexWrap: 'wrap', gap: '0.35rem', marginBottom: '0.5rem' }}
          >
            {(['all', 'open', 'claimed', 'approved', 'rejected'] as const).map((k) => {
              const label = k === 'all' ? '全部' : STATE_LABEL[k] ?? k
              const count =
                k === 'all' ? tickets.length : tickets.filter((t) => t.state === k).length
              return (
                <button
                  key={k}
                  type="button"
                  className={stateFilter === k ? 'btn btn-primary' : 'btn btn-ghost'}
                  onClick={() => setStateFilter(k)}
                >
                  {label} ({count})
                </button>
              )
            })}
          </div>
          {visibleTickets.length === 0 ? (
            <p className="meta">没有工单</p>
          ) : (
            <div className="table-wrap">
              <table className="data-table">
                <thead>
                  <tr>
                    <th>类型</th>
                    <th>玩家</th>
                    <th>主题</th>
                    <th>状态</th>
                    <th>提交时间</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {visibleTickets.map((t) => (
                    <tr key={t.id}>
                      <td>{KIND_LABEL[t.kind] ?? t.kind}</td>
                      <td>
                        <strong>{t.player}</strong>
                      </td>
                      <td>
                        {t.subject}
                        <div className="meta">{t.body}</div>
                      </td>
                      <td>
                        <span className={`badge status-${t.state === 'open' || t.state === 'claimed' ? 'running' : 'stopped'}`}>
                          {STATE_LABEL[t.state] ?? t.state}
                        </span>
                      </td>
                      <td className="meta">{fmt(t.created_at)}</td>
                      <td>
                        {(t.state === 'open' || t.state === 'claimed') && (
                          <>
                            {t.state === 'open' && (
                              <button
                                type="button"
                                className="link-btn"
                                disabled={busy}
                                onClick={() => void act(() => api.claimTicket(t.id))}
                              >
                                认领
                              </button>
                            )}
                            <button
                              type="button"
                              className="link-btn"
                              disabled={busy}
                              onClick={() => void act(() => api.approveTicket(t.id, '已处理'))}
                            >
                              通过
                            </button>
                            <button
                              type="button"
                              className="link-btn"
                              disabled={busy}
                              onClick={() => void act(() => api.rejectTicket(t.id, '已驳回'))}
                            >
                              驳回
                            </button>
                          </>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      {tab === 'actions' && (
        <div className="card-panel" style={{ marginTop: '1rem' }}>
          <div className="settings">
            <label>
              按玩家筛选
              <input
                value={searchTarget}
                onChange={(e) => setSearchTarget(e.target.value)}
                placeholder="留空查看全部"
              />
            </label>
            <button type="button" className="btn btn-primary" onClick={() => void loadActions()}>
              查询
            </button>
          </div>
          {actions.length === 0 ? (
            <p className="meta" style={{ marginTop: '0.75rem' }}>
              没有行为记录
            </p>
          ) : (
            <div className="table-wrap" style={{ marginTop: '0.75rem' }}>
              <table className="data-table">
                <thead>
                  <tr>
                    <th>时间</th>
                    <th>操作人</th>
                    <th>动作</th>
                    <th>对象</th>
                    <th>详情</th>
                  </tr>
                </thead>
                <tbody>
                  {actions.map((a) => (
                    <tr key={a.id}>
                      <td className="meta">{fmt(a.at)}</td>
                      <td>{a.actor}</td>
                      <td>{a.action}</td>
                      <td>
                        <strong>{a.target}</strong>
                      </td>
                      <td className="meta">{a.detail}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
