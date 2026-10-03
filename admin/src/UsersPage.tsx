import { useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { api, getPermissions, type PanelUser } from './api'

const ROLES = [
  { id: 'superadmin', label: 'Owner' },
  { id: 'admin', label: '管理员' },
  { id: 'support', label: '客服' },
  { id: 'developer', label: '开发' },
  { id: 'observer', label: '观察员' },
]

const ROLE_DESC: Record<string, string> = {
  superadmin: '全部权限，含用户与 Owner 管理',
  admin: '启停、控制台、文件、插件、备份、网络、节点，不含用户管理',
  support: '启停、控制台、玩家、备份、网络，不能改文件与配置',
  developer: '控制台、文件、插件、启停，不能碰玩家与备份',
  observer: '只读，所有写操作被拒绝',
}

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
  const [notice, setNotice] = useState<string | null>(null)
  const [resetFor, setResetFor] = useState<PanelUser | null>(null)
  const [resetPw, setResetPw] = useState('')
  const mine = getPermissions()

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
    setNotice(null)
    try {
      await api.createUser({ username: username.trim(), password, role })
      setNotice(`已创建 ${username.trim()}（${ROLES.find((r) => r.id === role)?.label}）`)
      setUsername('')
      setPassword('')
      await load()
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(false)
    }
  }

  const changeRole = async (u: PanelUser, next: string) => {
    setBusy(true)
    onError(null)
    setNotice(null)
    try {
      await api.updateUser(u.id, { role: next })
      setNotice(`${u.username} 的角色已改为 ${ROLES.find((r) => r.id === next)?.label}`)
      await load()
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(false)
    }
  }

  const remove = async (u: PanelUser) => {
    if (!window.confirm(`删除用户 ${u.username}？该用户的登录会话会立即失效。`)) return
    setBusy(true)
    onError(null)
    try {
      await api.deleteUser(u.id)
      await load()
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(false)
    }
  }

  const submitReset = async (e: FormEvent) => {
    e.preventDefault()
    if (!resetFor) return
    setBusy(true)
    onError(null)
    try {
      await api.updateUser(resetFor.id, { password: resetPw })
      setNotice(`${resetFor.username} 的密码已重置，其旧会话已全部下线`)
      setResetFor(null)
      setResetPw('')
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
        <h3 className="card-title">角色能力</h3>
        <table className="data-table">
          <thead>
            <tr>
              <th>角色</th>
              <th>能力</th>
            </tr>
          </thead>
          <tbody>
            {ROLES.map((r) => (
              <tr key={r.id}>
                <td>
                  <strong>{r.label}</strong>
                </td>
                <td className="meta">{ROLE_DESC[r.id]}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <p className="meta mb-1">
          当前账号权限：{mine.length ? mine.join('、') : '（未记录，请重新登录）'}
        </p>
      </div>

      <div className="card-panel" style={{ marginTop: '1rem' }}>
        <h3 className="card-title">新增用户</h3>
        <form className="settings" onSubmit={create}>
          <label>
            用户名
            <input
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              required
              minLength={3}
              maxLength={32}
              pattern="[A-Za-z0-9_\-]+"
              placeholder="3–32 位字母数字下划线短横线"
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
              placeholder="至少 8 位"
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
        {notice && <p className="meta mb-1">{notice}</p>}
      </div>

      {resetFor && (
        <div className="card-panel" style={{ marginTop: '1rem' }}>
          <h3 className="card-title">重置 {resetFor.username} 的密码</h3>
          <form className="settings" onSubmit={submitReset}>
            <label>
              新密码
              <input
                type="password"
                value={resetPw}
                onChange={(e) => setResetPw(e.target.value)}
                required
                minLength={8}
              />
            </label>
            <button type="submit" className="btn btn-primary" disabled={busy}>
              确认重置
            </button>
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => {
                setResetFor(null)
                setResetPw('')
              }}
            >
              取消
            </button>
          </form>
        </div>
      )}

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
                <td>
                  <select
                    value={u.role}
                    disabled={busy}
                    onChange={(e) => void changeRole(u, e.target.value)}
                  >
                    {ROLES.map((r) => (
                      <option key={r.id} value={r.id}>
                        {r.label}
                      </option>
                    ))}
                  </select>
                </td>
                <td className="meta">
                  {new Date(u.created_at).toLocaleString('zh-CN', {
                    hour12: false,
                  })}
                </td>
                <td>
                  <button
                    type="button"
                    className="link-btn"
                    disabled={busy}
                    onClick={() => {
                      setResetFor(u)
                      setResetPw('')
                    }}
                  >
                    重置密码
                  </button>
                  <button
                    type="button"
                    className="link-btn danger"
                    disabled={busy}
                    onClick={() => void remove(u)}
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