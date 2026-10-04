import { useState } from 'react'
import type { FormEvent } from 'react'
import { api, setSession, type AuthSession } from './api'

type Props = {
  hintUsername?: string | null
  panelName?: string
  busy: boolean
  offline?: boolean
  onBusy: (v: boolean, label?: string) => void
  onDone: (session: AuthSession) => void
  onRetryHealth?: () => void
}

export default function LoginPage({
  hintUsername,
  panelName,
  busy,
  offline,
  onBusy,
  onDone,
  onRetryHealth,
}: Props) {
  const [username, setUsername] = useState(hintUsername ?? '')
  const [password, setPassword] = useState('')
  const [totp, setTotp] = useState('')
  const [needTotp, setNeedTotp] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    onBusy(true, '正在登录…')
    setError(null)
    try {
      const code = totp.trim() ? Number(totp.trim()) : undefined
      const session = await api.login({
        username: username.trim(),
        password,
        totp_code: code,
      })
      setSession(session)
      onDone(session)
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      if (msg.includes('2FA')) {
        setNeedTotp(true)
      }
      setError(msg)
    } finally {
      onBusy(false)
    }
  }

  return (
    <div className="auth-gate">
      <header className="topbar">
        <div className="topbar-brand">
          <div className="brand-mark" aria-hidden>
            <img src="/logo.png" alt="" className="brand-logo-img" />
          </div>
          <div>
            <h1>Cocktail</h1>
            <span className="brand-sub">{panelName || 'Manager'}</span>
          </div>
        </div>
      </header>
      <main className="auth-gate-main">
        <form className="auth-card" onSubmit={submit}>
          <p className="page-eyebrow">控制面登录</p>
          <h2>最高管理员</h2>
          <p className="meta">使用初始化时创建的超级管理员账号进入服务器设置与实例管理。</p>

          {offline && (
            <div className="error-banner" role="alert">
              <span>控制面离线，请确认 cocktail-control 已启动。</span>
              {onRetryHealth && (
                <button type="button" className="link-btn" onClick={onRetryHealth}>
                  重试
                </button>
              )}
            </div>
          )}

          <div className="settings">
            <label>
              用户名
              <input
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                autoComplete="username"
                autoFocus
                disabled={offline}
              />
            </label>
            <label>
              密码
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                autoComplete="current-password"
                disabled={offline}
              />
            </label>
            <label>
              动态码 (2FA)
              <input
                value={totp}
                onChange={(e) => setTotp(e.target.value)}
                placeholder={needTotp ? '请输入 6 位验证码' : '未开启 2FA 可留空'}
                inputMode="numeric"
                autoComplete="one-time-code"
                disabled={offline}
              />
            </label>
          </div>

          {error && (
            <div className="error-banner" role="alert">
              <span>{error}</span>
            </div>
          )}

          <div className="wizard-actions">
            <span className="spacer" />
            <button
              type="submit"
              className="btn btn-primary"
              disabled={busy || offline || !username.trim() || !password}
            >
              登录
            </button>
          </div>
        </form>
      </main>
    </div>
  )
}
