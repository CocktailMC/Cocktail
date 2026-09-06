import { useEffect, useState } from 'react'
import { api, type ExtensionInfo } from './api'

type Props = {
  onBack: () => void
  onError: (msg: string | null) => void
}

type EsplusRow = {
  id: string
  name?: string
  status?: string
  core?: string
  installed?: boolean
  jar?: string | null
  panelUrl?: string
  panelReachable?: boolean | null
  passwordIsDefault?: boolean
}

function parseEsplusSummary(payload: string): EsplusRow[] {
  try {
    const data = JSON.parse(payload) as { instances?: EsplusRow[] }
    return Array.isArray(data.instances) ? data.instances : []
  } catch {
    return []
  }
}

export default function ExtensionsPage({ onBack, onError }: Props) {
  const [online, setOnline] = useState(false)
  const [host, setHost] = useState('')
  const [hostError, setHostError] = useState<string | null>(null)
  const [items, setItems] = useState<ExtensionInfo[]>([])
  const [busy, setBusy] = useState(false)
  const [active, setActive] = useState<string | null>(null)
  const [payload, setPayload] = useState<string>('')
  const [watchdogAuto, setWatchdogAuto] = useState(false)
  const [gameYaml, setGameYaml] = useState('')
  const [esplusHint, setEsplusHint] = useState<string | null>(null)

  const load = async () => {
    try {
      const list = await api.listExtensions()
      setOnline(list.online)
      setHost(list.host)
      setHostError(list.error ?? null)
      setItems(list.items ?? [])
      if (!active && list.items?.[0]) {
        setActive(list.items[0].id)
      }
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    }
  }

  useEffect(() => {
    void load()
    const t = setInterval(() => void load(), 8000)
    return () => clearInterval(t)
  }, [])

  useEffect(() => {
    if (!active || !online) {
      setPayload('')
      return
    }
    const ext = items.find((i) => i.id === active)
    const path = ext?.ui?.path || '/summary'
    api
      .extensionGet(active, path)
      .then((data) => {
        setPayload(JSON.stringify(data, null, 2))
        if (active === 'watchdog' && data && typeof data === 'object' && 'autoStartOnCrash' in data) {
          setWatchdogAuto(Boolean((data as { autoStartOnCrash?: boolean }).autoStartOnCrash))
        }
        if (active === 'gameops' && !gameYaml) {
          api.extensionText('gameops', '/example').then(setGameYaml).catch(() => undefined)
        }
        if (active === 'esplus') {
          setEsplusHint(null)
        }
      })
      .catch((e) => setPayload(e instanceof Error ? e.message : String(e)))
  }, [active, online, items])

  const reload = async () => {
    setBusy(true)
    onError(null)
    try {
      await api.reloadExtensions()
      await load()
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const toggle = async (id: string, enabled: boolean) => {
    setBusy(true)
    try {
      await api.setExtensionEnabled(id, enabled)
      await load()
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const applyGameOps = async () => {
    setBusy(true)
    onError(null)
    try {
      await api.extensionPost('gameops', '/apply', { yaml: gameYaml })
      const data = await api.extensionGet('gameops', '/summary')
      setPayload(JSON.stringify(data, null, 2))
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const reconcileGameOps = async () => {
    setBusy(true)
    try {
      await api.extensionPost('gameops', '/reconcile', {})
      const data = await api.extensionGet('gameops', '/summary')
      setPayload(JSON.stringify(data, null, 2))
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const esplusAction = async (path: string, instanceId: string) => {
    setBusy(true)
    onError(null)
    setEsplusHint(null)
    try {
      const result = await api.extensionPost('esplus', path, { instanceId })
      if (result && typeof result === 'object' && 'hint' in result) {
        const hint = (result as { hint?: string; panelPassword?: string }).hint
        const pw = (result as { panelPassword?: string }).panelPassword
        setEsplusHint(pw ? `${hint ?? ''} 密码：${pw}` : (hint ?? JSON.stringify(result)))
      }
      const data = await api.extensionGet('esplus', '/summary')
      setPayload(JSON.stringify(data, null, 2))
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const saveWatchdog = async () => {
    setBusy(true)
    try {
      await api.extensionPost('watchdog', '/config', {
        autoStartOnCrash: watchdogAuto,
      })
      const data = await api.extensionGet('watchdog', '/summary')
      setPayload(JSON.stringify(data, null, 2))
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="page-flow">
      <div className="page-head">
        <div>
          <p className="page-eyebrow">GameOps</p>
          <h2 className="page-title">.NET 插件</h2>
        </div>
        <div className="btn-row">
          <button type="button" className="btn btn-ghost" onClick={() => void reload()} disabled={busy}>
            重载宿主
          </button>
          <button type="button" className="btn btn-ghost" onClick={onBack}>
            返回主界面
          </button>
        </div>
      </div>

      <div className="card-panel">
        <p className="meta mb-1">
          宿主 {host || 'http://127.0.0.1:11012'} ·{' '}
          <span className={`badge status-${online ? 'running' : 'stopped'}`}>
            {online ? '在线' : '离线'}
          </span>
        </p>
        {!online && (
          <pre className="console" style={{ whiteSpace: 'pre-wrap' }}>
            {`# 在仓库根目录
dotnet build dotnet/Cocktail.sln
dotnet run --project dotnet/Cocktail.PluginHost
# 或让控制面自动拉起：安装 .NET 8 后重启 cocktail-control`}
          </pre>
        )}
        {hostError && <p className="error">{hostError}</p>}
      </div>

      <div className="card-panel" style={{ marginTop: '1rem' }}>
        {items.length === 0 ? (
          <p className="meta">
            还没有插件。把带 plugin.json 的目录放到{' '}
            <code>dotnet/dist/plugins/&lt;id&gt;</code> 后重载。
          </p>
        ) : (
          <div className="table-wrap">
            <table className="data-table">
              <thead>
                <tr>
                  <th>插件</th>
                  <th>版本</th>
                  <th>权限</th>
                  <th>状态</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {items.map((p) => (
                  <tr key={p.id}>
                    <td>
                      <button
                        type="button"
                        className="link-btn"
                        onClick={() => setActive(p.id)}
                      >
                        <strong>{p.name}</strong>
                      </button>
                      <div className="meta">{p.description}</div>
                    </td>
                    <td>{p.version}</td>
                    <td className="meta">{(p.permissions ?? []).join(', ')}</td>
                    <td>
                      <span className={`badge status-${p.running ? 'running' : 'stopped'}`}>
                        {p.enabled ? (p.running ? '运行中' : '已启用') : '已停用'}
                      </span>
                      {p.error ? <div className="error">{p.error}</div> : null}
                    </td>
                    <td>
                      <button
                        type="button"
                        className="btn btn-ghost"
                        disabled={busy}
                        onClick={() => void toggle(p.id, !p.enabled)}
                      >
                        {p.enabled ? '停用' : '启用'}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      {active && online && (
        <div className="card-panel" style={{ marginTop: '1rem' }}>
          <h3 className="card-title">
            {items.find((i) => i.id === active)?.name ?? active}
          </h3>
          {active === 'gameops' && (
            <div className="mb-1">
              <p className="meta">
                Node / Instance 由控制面观察；World / PluginSet / Proxy / Network 在此 apply 后由插件协调。
              </p>
              <textarea
                value={gameYaml}
                onChange={(e) => setGameYaml(e.target.value)}
                rows={16}
                spellCheck={false}
                style={{ fontFamily: 'ui-monospace, monospace', width: '100%' }}
              />
              <div className="btn-row mt-4">
                <button type="button" className="btn btn-primary" disabled={busy} onClick={() => void applyGameOps()}>
                  Apply 清单
                </button>
                <button type="button" className="btn btn-ghost" disabled={busy} onClick={() => void reconcileGameOps()}>
                  立即协调
                </button>
              </div>
            </div>
          )}
          {active === 'esplus' && (
            <div className="mb-1">
              <p className="meta">
                适配{' '}
                <a href="https://github.com/FORGE24/ESPlus" target="_blank" rel="noreferrer">
                  FORGE24/ESPlus
                </a>
                ：扫描实例 <code>mods/esplus*.jar</code> 与{' '}
                <code>config/esplus-common.toml</code>
                。安装会拉取 GitHub Release 的 fat jar（或 <code>COCKTAIL_ESPLUS_JAR</code>
                ），并写入 loopback 面板配置。服务端与客户端都需要同一模组。
              </p>
              {esplusHint ? <p className="meta">{esplusHint}</p> : null}
              {(() => {
                const parsed = parseEsplusSummary(payload)
                if (!parsed.length) {
                  return <p className="meta">当前没有实例，或摘要尚未加载。</p>
                }
                return (
                  <div className="table-wrap">
                    <table>
                      <thead>
                        <tr>
                          <th>实例</th>
                          <th>模组</th>
                          <th>面板</th>
                          <th />
                        </tr>
                      </thead>
                      <tbody>
                        {parsed.map((row) => (
                          <tr key={row.id}>
                            <td>
                              {row.name || row.id}
                              <div className="meta">
                                {row.status} · {row.core || '—'}
                              </div>
                            </td>
                            <td>{row.installed ? row.jar : '未安装'}</td>
                            <td>
                              {row.panelUrl}
                              <div className="meta">
                                {row.panelReachable === true
                                  ? '可达'
                                  : row.panelReachable === false
                                    ? '未监听'
                                    : '未探测'}
                                {row.passwordIsDefault ? ' · 仍是默认密码' : ''}
                              </div>
                            </td>
                            <td>
                              <div className="btn-row">
                                <button
                                  type="button"
                                  className="btn btn-ghost"
                                  disabled={busy}
                                  onClick={() => void esplusAction('/ensure-config', row.id)}
                                >
                                  写入配置
                                </button>
                                <button
                                  type="button"
                                  className="btn btn-primary"
                                  disabled={busy}
                                  onClick={() => void esplusAction('/install', row.id)}
                                >
                                  安装模组
                                </button>
                              </div>
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )
              })()}
            </div>
          )}
          {active === 'watchdog' && (
            <label className="mb-1">
              <input
                type="checkbox"
                checked={watchdogAuto}
                onChange={(e) => setWatchdogAuto(e.target.checked)}
              />{' '}
              崩溃后请求控制面再次启动
              <button
                type="button"
                className="btn btn-primary"
                style={{ marginLeft: '0.75rem' }}
                disabled={busy}
                onClick={() => void saveWatchdog()}
              >
                保存策略
              </button>
            </label>
          )}
          <pre className="console" style={{ whiteSpace: 'pre-wrap', maxHeight: 420, overflow: 'auto' }}>
            {payload || '…'}
          </pre>
        </div>
      )}
    </div>
  )
}
