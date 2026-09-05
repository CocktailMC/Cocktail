import { useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { api, type HealthInfo, type PanelSettings } from './api'
import { BrandImg, BRAND } from './brandIcons'
import EnvBrandBar from './EnvBrandBar'

type Props = {
  health: string
  env: HealthInfo | null
  dockerAvailable: boolean
  dockerMessage: string
  instanceCount: number
  adminName: string
  onPanelName: (name: string) => void
  onAdminName: (name: string) => void
  onBack: () => void
  onBusy: (v: boolean, label?: string) => void
  onError: (msg: string | null) => void
}

export default function HomeSettings({
  health,
  env,
  dockerAvailable,
  dockerMessage,
  instanceCount,
  adminName,
  onPanelName,
  onAdminName,
  onBack,
  onBusy,
  onError,
}: Props) {
  const [settings, setSettings] = useState<PanelSettings | null>(null)
  const [panelName, setPanelName] = useState('')
  const [webhook, setWebhook] = useState('')
  const [username, setUsername] = useState(adminName)
  const [currentPw, setCurrentPw] = useState('')
  const [newPw, setNewPw] = useState('')
  const [confirmPw, setConfirmPw] = useState('')
  const [saved, setSaved] = useState<string | null>(null)

  const load = async () => {
    const s = await api.getSettings()
    setSettings(s)
    setPanelName(s.panel_name)
    setWebhook(s.webhook_url ?? '')
    setUsername(s.admin_username)
    onPanelName(s.panel_name)
    onAdminName(s.admin_username)
  }

  useEffect(() => {
    load().catch((e: Error) => onError(e.message))
  }, [])

  const savePanel = async (e: FormEvent) => {
    e.preventDefault()
    onBusy(true, '保存服务器设置…')
    onError(null)
    setSaved(null)
    try {
      const s = await api.updateSettings({
        panel_name: panelName.trim(),
        webhook_url: webhook.trim(),
        username: username.trim(),
      })
      setSettings(s)
      onPanelName(s.panel_name)
      onAdminName(s.admin_username)
      setSaved('服务器设置已保存')
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err))
    } finally {
      onBusy(false)
    }
  }

  const savePassword = async (e: FormEvent) => {
    e.preventDefault()
    if (newPw !== confirmPw) {
      onError('两次新密码不一致')
      return
    }
    onBusy(true, '更新密码…')
    onError(null)
    setSaved(null)
    try {
      await api.changePassword({
        current_password: currentPw,
        new_password: newPw,
      })
      setCurrentPw('')
      setNewPw('')
      setConfirmPw('')
      setSaved('最高管理员密码已更新')
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err))
    } finally {
      onBusy(false)
    }
  }

  return (
    <div className="home-page">
      <div className="page-head">
        <div>
          <p className="page-eyebrow">Cocktail Manager</p>
          <h2 className="page-title">服务器设置</h2>
        </div>
        <button type="button" className="btn btn-ghost" onClick={onBack}>
          <i className="fa fa-arrow-left" /> 返回主界面
        </button>
      </div>

      <EnvBrandBar env={env} offline={health === 'offline'} />

      {saved && <p className="ok settings-saved">{saved}</p>}

      <div className="grid-2">
        <form className="card-panel" onSubmit={savePanel}>
          <h3 className="card-title">
            <i className="fa fa-sliders" /> 控制面
          </h3>
          <div className="settings">
            <label>
              显示名称
              <input
                value={panelName}
                onChange={(e) => setPanelName(e.target.value)}
              />
            </label>
            <label>
              崩溃 Webhook URL
              <input
                value={webhook}
                onChange={(e) => setWebhook(e.target.value)}
                placeholder={
                  settings?.env_webhook_set
                    ? '留空则使用 COCKTAIL_WEBHOOK_URL'
                    : 'https://…'
                }
              />
            </label>
            <label>
              最高管理员用户名
              <input
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                autoComplete="username"
              />
            </label>
            <button type="submit" className="btn btn-primary">
              保存设置
            </button>
          </div>
        </form>

        <form className="card-panel" onSubmit={savePassword}>
          <h3 className="card-title">
            <i className="fa fa-user-secret" /> 最高管理员密码
          </h3>
          <p className="meta" style={{ marginBottom: '0.85rem' }}>
            当前账号 <strong>{settings?.admin_username ?? adminName}</strong>
            {settings?.admin_created_at
              ? ` · 创建于 ${settings.admin_created_at.slice(0, 10)}`
              : ''}
          </p>
          <div className="settings">
            <label>
              当前密码
              <input
                type="password"
                value={currentPw}
                onChange={(e) => setCurrentPw(e.target.value)}
                autoComplete="current-password"
              />
            </label>
            <label>
              新密码（至少 8 位）
              <input
                type="password"
                value={newPw}
                onChange={(e) => setNewPw(e.target.value)}
                autoComplete="new-password"
              />
            </label>
            <label>
              确认新密码
              <input
                type="password"
                value={confirmPw}
                onChange={(e) => setConfirmPw(e.target.value)}
                autoComplete="new-password"
              />
            </label>
            <button
              type="submit"
              className="btn btn-primary"
              disabled={!currentPw || newPw.length < 8}
            >
              更新密码
            </button>
          </div>
        </form>
      </div>

      <div className="grid-2 mt-6">
        <div className="card-panel">
          <h3 className="card-title">
            <BrandImg src={BRAND.openjdk} alt="" height={16} /> 控制面信息
          </h3>
          <table className="info-table">
            <tbody>
              <tr>
                <td>健康状态</td>
                <td>{health === 'offline' ? '离线' : '在线'}</td>
              </tr>
              <tr>
                <td>版本</td>
                <td>{health}</td>
              </tr>
              <tr>
                <td>监听</td>
                <td>{settings?.bind ?? '—'}</td>
              </tr>
              <tr>
                <td>SQLite</td>
                <td>
                  <code>{settings?.db_path ?? 'data/cocktail.db'}</code>
                </td>
              </tr>
              <tr>
                <td>系统</td>
                <td>
                  {[env?.distro_name || env?.os, env?.arch, env?.hostname]
                    .filter(Boolean)
                    .join(' · ') || '—'}
                </td>
              </tr>
              <tr>
                <td>实例数</td>
                <td>{instanceCount}</td>
              </tr>
              <tr>
                <td>
                  <BrandImg src={BRAND.docker} alt="" height={14} /> Docker
                </td>
                <td>
                  {dockerAvailable ? '就绪' : '不可用'}
                  {dockerMessage ? ` · ${dockerMessage}` : ''}
                </td>
              </tr>
              <tr>
                <td>环境 Token</td>
                <td>{settings?.env_api_token_set ? '已配置 COCKTAIL_API_TOKEN' : '未配置'}</td>
              </tr>
            </tbody>
          </table>
        </div>

        <div className="card-panel">
          <h3 className="card-title">
            <i className="fa fa-info-circle" /> 说明
          </h3>
          <ul className="home-help">
            <li>本页管理整台控制面，不是单个 Minecraft 实例。</li>
            <li>
              实例级设置（内存、端口、EULA、启动命令）在对应实例的「系统设置」中修改。
            </li>
            <li>
              账号与面板配置保存在 SQLite；实例列表仍在 <code>data/state.json</code>。
            </li>
          </ul>
        </div>
      </div>
    </div>
  )
}
