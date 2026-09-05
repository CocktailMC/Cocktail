import { useState } from 'react'
import type { FormEvent } from 'react'
import { api, setToken, type AuthSession } from './api'

type Props = {
  busy: boolean
  onBusy: (v: boolean, label?: string) => void
  onDone: (session: AuthSession) => void
}

const STEPS = ['欢迎', '面板名称', '最高管理员'] as const

export default function SetupPage({ busy, onBusy, onDone }: Props) {
  const [step, setStep] = useState(0)
  const [panelName, setPanelName] = useState('Cocktail Manager')
  const [username, setUsername] = useState('admin')
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [error, setError] = useState<string | null>(null)

  const canNext = () => {
    if (step === 0) return true
    if (step === 1) return panelName.trim().length > 0
    if (username.trim().length < 3) return false
    if (password.length < 8) return false
    if (password !== confirm) return false
    return true
  }

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    if (step < STEPS.length - 1) {
      if (canNext()) setStep((s) => s + 1)
      return
    }
    if (!canNext()) return
    onBusy(true, '正在创建最高管理员…')
    setError(null)
    try {
      const session = await api.setup({
        username: username.trim(),
        password,
        panel_name: panelName.trim(),
      })
      setToken(session.token)
      onDone(session)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
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
            <span className="brand-sub">首次初始化</span>
          </div>
        </div>
      </header>
      <main className="auth-gate-main">
        <form className="auth-card" onSubmit={submit}>
          <p className="page-eyebrow">第一次打开控制面</p>
          <h2>设置最高管理员</h2>
          <p className="meta">
            这是控制面的唯一超级管理员，将写入本机 SQLite（
            <code>data/cocktail.db</code>
            ）。完成后每次进入都需要登录。
          </p>

          <ol className="wizard-steps">
            {STEPS.map((label, i) => (
              <li
                key={label}
                className={
                  i === step ? 'active' : i < step ? 'done' : undefined
                }
              >
                <span className="step-num">{i + 1}</span>
                {label}
              </li>
            ))}
          </ol>

          {step === 0 && (
            <div className="setup-intro">
              <p>
                Cocktail Manager 会在这台机器上管理 Minecraft 实例。在创建服务器之前，请先指定最高管理员账号。
              </p>
              <ul className="home-help">
                <li>最高管理员可改面板名称、Webhook、以及所有实例。</li>
                <li>密码使用 Argon2id 哈希后存入 SQLite，不会明文保存。</li>
                <li>可选环境变量 <code>COCKTAIL_API_TOKEN</code> 仍可用于脚本访问。</li>
              </ul>
            </div>
          )}

          {step === 1 && (
            <div className="settings">
              <label>
                控制面显示名称
                <input
                  value={panelName}
                  onChange={(e) => setPanelName(e.target.value)}
                  autoFocus
                />
              </label>
            </div>
          )}

          {step === 2 && (
            <div className="settings">
              <label>
                用户名
                <input
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  autoComplete="username"
                  autoFocus
                />
              </label>
              <label>
                密码（至少 8 位）
                <input
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  autoComplete="new-password"
                />
              </label>
              <label>
                确认密码
                <input
                  type="password"
                  value={confirm}
                  onChange={(e) => setConfirm(e.target.value)}
                  autoComplete="new-password"
                />
              </label>
              {password && confirm && password !== confirm && (
                <p className="field-warn">两次密码不一致</p>
              )}
            </div>
          )}

          {error && (
            <div className="error-banner" role="alert">
              <span>
                <i className="fa fa-exclamation-circle" /> {error}
              </span>
            </div>
          )}

          <div className="wizard-actions">
            {step > 0 ? (
              <button
                type="button"
                className="btn btn-ghost"
                onClick={() => setStep((s) => s - 1)}
              >
                上一步
              </button>
            ) : (
              <span />
            )}
            <span className="spacer" />
            <button
              type="submit"
              className="btn btn-primary"
              disabled={busy || !canNext()}
            >
              {step < STEPS.length - 1 ? '继续' : '创建并进入'}
            </button>
          </div>
        </form>
      </main>
    </div>
  )
}
