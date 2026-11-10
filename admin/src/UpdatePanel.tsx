import { useEffect, useState } from 'react'
import { api, type UpdateResponse } from './api'

type Props = {
  onError: (msg: string | null) => void
}

function formatTime(iso: string): string {
  if (!iso) return '—'
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return d.toLocaleString('zh-CN', { hour12: false })
}

export default function UpdatePanel({ onError }: Props) {
  const [data, setData] = useState<UpdateResponse | null>(null)
  const [busy, setBusy] = useState(false)
  const [channel, setChannel] = useState<'stable' | 'beta'>('stable')

  const load = async (force = false) => {
    setBusy(true)
    onError(null)
    try {
      setData(await api.updateStatus({ channel, force }))
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  useEffect(() => {
    void load(false)
  }, [channel])

  const status = data?.status
  const hasUpdate = status?.update_available === true

  return (
    <div className="card-panel">
      <h3 className="card-title">版本与更新</h3>
      <p className="meta mb-1">
        当前版本 <code>{status?.current ?? '检测中…'}</code>
        {status?.checked_at ? ` · 上次检查 ${formatTime(status.checked_at)}` : ''}
      </p>

      <div className="settings">
        <label>
          更新通道
          <select
            value={channel}
            onChange={(e) => setChannel(e.target.value as 'stable' | 'beta')}
            disabled={busy}
          >
            <option value="stable">稳定版</option>
            <option value="beta">含预发布</option>
          </select>
        </label>
        <button
          type="button"
          className="btn btn-primary"
          disabled={busy}
          onClick={() => void load(true)}
        >
          {busy ? '检查中…' : '立即检查'}
        </button>
      </div>

      {status?.error && (
        <p className="field-warn" style={{ marginTop: '0.75rem' }}>
          检查失败：{status.error}
          {status.notes_url && (
            <a href={status.notes_url} target="_blank" rel="noreferrer" style={{ marginLeft: '0.5rem' }}>
              手动查看
            </a>
          )}
        </p>
      )}

      {!status?.error && status?.latest && (
        <p style={{ marginTop: '0.75rem' }}>
          {hasUpdate ? (
            <>
              <strong>有新版本可用</strong>：<code>{status.latest}</code>
              {status.latest_is_stable ? ' （稳定版）' : ' （预发布）'}
              {status.notes_url && (
                <a href={status.notes_url} target="_blank" rel="noreferrer" style={{ marginLeft: '0.5rem' }}>
                  查看发布说明
                </a>
              )}
            </>
          ) : (
            <>
              <strong>已是最新</strong>
              {status.latest ? `（${status.latest}）` : ''}
            </>
          )}
        </p>
      )}

      {data && data.releases.length > 0 && (
        <div className="table-wrap" style={{ marginTop: '1rem' }}>
          <table className="data-table">
            <thead>
              <tr>
                <th>版本</th>
                <th>发布时间</th>
                <th>类型</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {data.releases.map((r) => (
                <tr key={r.tag}>
                  <td>
                    <code>{r.tag}</code>
                  </td>
                  <td className="meta">{formatTime(r.published_at)}</td>
                  <td>
                    <span className={`badge status-${r.prerelease ? 'stopped' : 'running'}`}>
                      {r.prerelease ? '预发布' : '稳定'}
                    </span>
                  </td>
                  <td>
                    <a href={r.html_url} target="_blank" rel="noreferrer">
                      说明
                    </a>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {data && Object.keys(data.channels).length > 0 && (
        <p className="meta" style={{ marginTop: '0.75rem' }}>
          可用发布：
          {Object.entries(data.channels)
            .map(([stage, n]) => `${stage} ${n}`)
            .join(' · ')}
        </p>
      )}
    </div>
  )
}
