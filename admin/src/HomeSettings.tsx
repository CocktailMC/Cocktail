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
  const [qqAppId, setQqAppId] = useState('')
  const [qqSecret, setQqSecret] = useState('')
  const [qqGroup, setQqGroup] = useState('')
  const [qqUser, setQqUser] = useState('')
  const [qqSandbox, setQqSandbox] = useState(false)
  const [qqAlerts, setQqAlerts] = useState(true)
  const [qqStatusSecs, setQqStatusSecs] = useState(0)
  const [rxAlertMbps, setRxAlertMbps] = useState(80)
  const [saved, setSaved] = useState<string | null>(null)

  const load = async () => {
    const s = await api.getSettings()
    setSettings(s)
    setPanelName(s.panel_name)
    setWebhook(s.webhook_url ?? '')
    setUsername(s.admin_username)
    setQqAppId(s.qq_app_id ?? '')
    setQqSecret('')
    setQqGroup(s.qq_group_openid ?? '')
    setQqUser(s.qq_user_openid ?? '')
    setQqSandbox(!!s.qq_sandbox)
    setQqAlerts(s.qq_alerts !== false)
    setQqStatusSecs(s.qq_status_secs ?? 0)
    setRxAlertMbps(s.net_alert_rx_mbps ?? 80)
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
        qq_app_id: qqAppId.trim(),
        qq_app_secret: qqSecret.trim() || undefined,
        qq_group_openid: qqGroup.trim(),
        qq_user_openid: qqUser.trim(),
        qq_sandbox: qqSandbox,
        qq_alerts: qqAlerts,
        qq_status_secs: qqStatusSecs,
        net_alert_rx_mbps: rxAlertMbps,
      })
      setSettings(s)
      onPanelName(s.panel_name)
      onAdminName(s.admin_username)
      setSaved('服务器设置已保存')
      setQqSecret('')
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
            <p className="card-title" style={{ marginTop: '0.4rem' }}>
              <i className="fa fa-comments" /> QQ 机器人（API v2）
            </p>
            <p className="meta">
              在{' '}
              <a href="https://bot.q.qq.com/wiki/develop/api-v2/" target="_blank" rel="noreferrer">
                QQ 开放平台
              </a>{' '}
              创建机器人，拿到 AppID / AppSecret。把机器人拉进群后，从事件里复制群
              openid。主动消息有平台额度；沙箱仅调试用。
            </p>
            <label>
              AppID
              <input
                value={qqAppId}
                onChange={(e) => setQqAppId(e.target.value)}
                autoComplete="off"
              />
            </label>
            <label>
              AppSecret
              <input
                type="password"
                value={qqSecret}
                onChange={(e) => setQqSecret(e.target.value)}
                placeholder={
                  settings?.qq_app_secret_set ? '已保存，留空则不改' : 'clientSecret'
                }
                autoComplete="new-password"
              />
            </label>
            <label>
              群 openid
              <input
                value={qqGroup}
                onChange={(e) => setQqGroup(e.target.value)}
                placeholder="group_openid"
              />
            </label>
            <label>
              单聊用户 openid（可选）
              <input
                value={qqUser}
                onChange={(e) => setQqUser(e.target.value)}
                placeholder="user_openid"
              />
            </label>
            <label>
              定时状态（秒，0 关闭）
              <select
                value={qqStatusSecs}
                onChange={(e) => setQqStatusSecs(Number(e.target.value))}
              >
                <option value={0}>关闭</option>
                <option value={300}>每 5 分钟</option>
                <option value={900}>每 15 分钟</option>
                <option value={1800}>每 30 分钟</option>
                <option value={3600}>每小时</option>
                <option value={21600}>每 6 小时</option>
              </select>
            </label>
            <label>
              主机下行告警阈值（MiB/s）
              <input
                type="number"
                min={1}
                step={1}
                value={rxAlertMbps}
                onChange={(e) => setRxAlertMbps(Number(e.target.value))}
              />
            </label>
            <label className="home-check">
              <input
                type="checkbox"
                checked={qqAlerts}
                onChange={(e) => setQqAlerts(e.target.checked)}
              />
              发送崩溃 / 网络告警
            </label>
            <label className="home-check">
              <input
                type="checkbox"
                checked={qqSandbox}
                onChange={(e) => setQqSandbox(e.target.checked)}
              />
              使用沙箱环境
            </label>
            <div className="btn-row">
              <button type="submit" className="btn btn-primary">
                保存设置
              </button>
              <button
                type="button"
                className="btn btn-ghost"
                onClick={async () => {
                  onBusy(true, '发送 QQ 测试…')
                  onError(null)
                  setSaved(null)
                  try {
                    await api.updateSettings({
                      qq_app_id: qqAppId.trim(),
                      qq_app_secret: qqSecret.trim() || undefined,
                      qq_group_openid: qqGroup.trim(),
                      qq_user_openid: qqUser.trim(),
                      qq_sandbox: qqSandbox,
                    })
                    await api.testQqBot()
                    setSaved('已向 QQ 发送测试消息')
                    setQqSecret('')
                    const s = await api.getSettings()
                    setSettings(s)
                  } catch (err) {
                    onError(err instanceof Error ? err.message : String(err))
                  } finally {
                    onBusy(false)
                  }
                }}
              >
                发送测试消息
              </button>
            </div>
            {settings?.qq_ready ? (
              <p className="ok">机器人配置齐全，告警与定时状态会走 QQ。</p>
            ) : (
              <p className="meta">配置未完成时不会发 QQ。</p>
            )}
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
            <li>
              QQ 机器人按开放平台 API v2 主动发消息；群/单聊 openid 与 AppID 绑定，不能混用别的机器人的
              id。
            </li>
          </ul>
        </div>
      </div>
    </div>
  )
}
