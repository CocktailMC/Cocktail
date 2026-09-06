import { useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { api, type PanelUser } from './api'

const ROLES = [
  { id: 'superadmin', label: 'Owner' },
  { id: 'admin', label: '管理员' },
  { id: 'support', label: '客服' },
  { id: 'developer', label: '开发' },
  { id: 'observer', label: '观察员' },
]

type Props = {
  onBack: () => void
  onError: (msg: string | null) => void
}

export default function UsersPage({ onBack, onError }: Props) {
  const [rows, setRows] = useState<PanelUser[]>([])
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [role, setRole] = useState('admin')
  const [busy, setBusy] = useState(false)

  const load = async () => {
    try {
      setRows(await api.listUsers())
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    }
  }

  useEffect(() => {
    void load()
  }, [])

  const create = async (e: FormEvent) => {
    e.preventDefault()
    setBusy(true)
    onError(null)
    try {
      await api.createUser({ username: username.trim(), password, role })
      setUsername('')
      setPassword('')
      await load()
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
          <p className="page-eyebrow">控制面</p>
          <h2 className="page-title">用户与权限</h2>
        </div>
        <button type="button" className="btn btn-ghost" onClick={onBack}>
          返回主界面
        </button>
      </div>
      <div className="card-panel">
        <p className="meta mb-1">
          Owner 可启动服务器、查看日志、改文件、管理玩家、备份与用户。观察员仅查看。
        </p>
        <form className="settings" onSubmit={create}>
          <label>
            用户名
            <input
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              required
              minLength={3}
            />
          </label>
          <label>
            密码
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
              minLength={8}
            />
          </label>
          <label>
            角色
            <select value={role} onChange={(e) => setRole(e.target.value)}>
              {ROLES.map((r) => (
                <option key={r.id} value={r.id}>
                  {r.label}
                </option>
              ))}
            </select>
          </label>
          <button type="submit" className="btn btn-primary" disabled={busy}>
            添加用户
          </button>
        </form>
      </div>
      <div className="card-panel" style={{ marginTop: '1rem' }}>
        <table className="data-table">
          <thead>
            <tr>
              <th>用户</th>
              <th>角色</th>
              <th>创建时间</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {rows.map((u) => (
              <tr key={u.id}>
                <td>
                  <strong>{u.username}</strong>
                </td>
                <td>{ROLES.find((r) => r.id === u.role)?.label ?? u.role}</td>
                <td className="meta">
                  {new Date(u.created_at).toLocaleString('zh-CN', {
                    hour12: false,
                  })}
                </td>
                <td>
                  <button
                    type="button"
                    className="link-btn danger"
                    disabled={busy}
                    onClick={() => {
                      if (!window.confirm(`删除用户 ${u.username}？`)) return
                      setBusy(true)
                      api
                        .deleteUser(u.id)
                        .then(load)
                        .catch((e: Error) => onError(e.message))
                        .finally(() => setBusy(false))
                    }}
                  >
                    删除
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}
