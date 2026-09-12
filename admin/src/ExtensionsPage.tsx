import { useEffect, useMemo, useState } from 'react'
import { api, type ExtensionInfo, type PluginUiView } from './api'

type Props = {
  onBack: () => void
  onError: (msg: string | null) => void
  focusId?: string | null
  onOpenPlugin?: (id: string) => void
}

function pick(obj: unknown, path: string | undefined): unknown {
  if (!path) return obj
  let cur: unknown = obj
  for (const part of path.split('.')) {
    if (cur && typeof cur === 'object' && part in (cur as object)) {
      cur = (cur as Record<string, unknown>)[part]
    } else {
      return undefined
    }
  }
  return cur
}

function cell(row: unknown, key: string): string {
  const v = pick(row, key)
  if (v == null) return '—'
  if (typeof v === 'boolean') return v ? '是' : '否'
  if (typeof v === 'object') return JSON.stringify(v)
  return String(v)
}

export default function ExtensionsPage({ onBack, onError, focusId, onOpenPlugin }: Props) {
  const [online, setOnline] = useState(false)
  const [host, setHost] = useState('')
  const [hostError, setHostError] = useState<string | null>(null)
  const [items, setItems] = useState<ExtensionInfo[]>([])
  const [busy, setBusy] = useState(false)
  const [active, setActive] = useState<string | null>(focusId ?? null)
  const [payload, setPayload] = useState<unknown>(null)
  const [rawText, setRawText] = useState('')
  const [codeDraft, setCodeDraft] = useState('')
  const [formState, setFormState] = useState<Record<string, unknown>>({})
  const [hint, setHint] = useState<string | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)

  const load = async () => {
    try {
      const list = await api.listExtensions()
      setOnline(list.online)
      setHost(list.host)
      setHostError(list.error ?? null)
      setItems(list.items ?? [])
      const next =
        focusId ||
        active ||
        list.items?.find((i) => i.enabled)?.id ||
        list.items?.[0]?.id ||
        null
      if (next && next !== active) {
        setActive(next)
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
    if (focusId) setActive(focusId)
  }, [focusId])

  const ext = items.find((i) => i.id === active) ?? null
  const views: PluginUiView[] = useMemo(() => {
    if (ext?.ui?.views?.length) return ext.ui.views
    const path = ext?.ui?.path || '/summary'
    return [{ id: 'raw', type: 'json', path, title: '数据' }]
  }, [ext])

  const loadViewData = async (pluginId: string) => {
    const dataPath = ext?.ui?.path || views.find((v) => v.type !== 'code' && v.type !== 'actions')?.path || '/summary'
    setLoadError(null)
    const data = await api.extensionGet(pluginId, dataPath)
    setPayload(data)
    if (typeof data === 'string') {
      setRawText(data)
    } else {
      setRawText(JSON.stringify(data, null, 2))
    }
    const obj = data && typeof data === 'object' ? (data as Record<string, unknown>) : {}
    const nextForm: Record<string, unknown> = {}
    for (const view of views) {
      for (const field of view.fields ?? []) {
        const from = field.from || field.name
        if (from in obj) nextForm[field.name] = obj[from]
      }
    }
    setFormState((prev) => ({ ...nextForm, ...prev }))
    const codeView = views.find((v) => v.type === 'code')
    if (codeView?.path) {
      try {
        const text = await api.extensionText(pluginId, codeView.path)
        setCodeDraft(text)
      } catch (e) {
        setCodeDraft(e instanceof Error ? e.message : String(e))
      }
    }
  }

  useEffect(() => {
    if (!active || !online) {
      setPayload(null)
      setRawText('')
      setLoadError(null)
      return
    }
    loadViewData(active).catch((e) => {
      const msg = e instanceof Error ? e.message : String(e)
      setRawText(msg)
      setLoadError(msg)
    })
  }, [active, online, ext?.id])

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

  const post = async (path: string, body: unknown) => {
    if (!active) return
    setBusy(true)
    onError(null)
    setHint(null)
    try {
      const result = await api.extensionPost(active, path, body)
      if (result && typeof result === 'object' && 'hint' in result) {
        const h = (result as { hint?: string; panelPassword?: string }).hint
        const pw = (result as { panelPassword?: string }).panelPassword
        setHint(pw ? `${h ?? ''} 密码：${pw}` : (h ?? null))
      }
      await loadViewData(active)
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const renderView = (view: PluginUiView) => {
    const type = view.type || 'json'
    if (type === 'form') {
      return (
        <div className="plugin-view" key={view.id || view.title}>
          {view.title ? <h4 className="card-title">{view.title}</h4> : null}
          {(view.fields ?? []).map((field) => (
            <label key={field.name} className="mb-1">
              {field.type === 'boolean' ? (
                <>
                  <input
                    type="checkbox"
                    checked={Boolean(formState[field.name])}
                    onChange={(e) =>
                      setFormState((s) => ({ ...s, [field.name]: e.target.checked }))
                    }
                  />{' '}
                  {field.label || field.name}
                </>
              ) : (
                <>
                  <span className="meta">{field.label || field.name}</span>
                  <input
                    value={String(formState[field.name] ?? '')}
                    onChange={(e) =>
                      setFormState((s) => ({ ...s, [field.name]: e.target.value }))
                    }
                  />
                </>
              )}
            </label>
          ))}
          <button
            type="button"
            className="btn btn-primary"
            disabled={busy}
            onClick={() => void post(view.path || '/config', formState)}
          >
            保存
          </button>
        </div>
      )
    }
    if (type === 'code') {
      return (
        <div className="plugin-view" key={view.id || 'code'}>
          {view.title ? <h4 className="card-title">{view.title}</h4> : null}
          <textarea
            value={codeDraft}
            onChange={(e) => setCodeDraft(e.target.value)}
            rows={16}
            spellCheck={false}
            style={{ fontFamily: 'var(--font-mono)', width: '100%' }}
          />
          {view.submit ? (
            <div className="btn-row mt-4">
              <button
                type="button"
                className="btn btn-primary"
                disabled={busy}
                onClick={() =>
                  void post(view.submit!.path, {
                    [view.submit!.bodyKey || 'yaml']: codeDraft,
                  })
                }
              >
                {view.submit.label || '提交'}
              </button>
            </div>
          ) : null}
        </div>
      )
    }
    if (type === 'actions') {
      return (
        <div className="btn-row plugin-view" key={view.id || 'actions'}>
          {(view.actions ?? []).map((a) => (
            <button
              key={a.path}
              type="button"
              className="btn btn-ghost"
              disabled={busy}
              onClick={() => void post(a.path, {})}
            >
              {a.label}
            </button>
          ))}
        </div>
      )
    }
    if (type === 'table') {
      const rows = (pick(payload, view.rows) as unknown[]) || []
      const list = Array.isArray(rows) ? rows : []
      return (
        <div className="plugin-view" key={view.id || 'table'}>
          {view.title ? <h4 className="card-title">{view.title}</h4> : null}
          {list.length === 0 ? (
            <p className="meta">{loadError ? loadError : '没有数据。'}</p>
          ) : (
            <div className="table-wrap">
              <table className="data-table">
                <thead>
                  <tr>
                    {(view.columns ?? []).map((c) => (
                      <th key={c.key}>{c.label}</th>
                    ))}
                    {view.rowActions?.length ? <th /> : null}
                  </tr>
                </thead>
                <tbody>
                  {list.map((row, idx) => (
                    <tr key={idx}>
                      {(view.columns ?? []).map((c) => (
                        <td key={c.key}>{cell(row, c.key)}</td>
                      ))}
                      {view.rowActions?.length ? (
                        <td>
                          <div className="btn-row">
                            {view.rowActions.map((a) => (
                              <button
                                key={a.label}
                                type="button"
                                className={a.primary ? 'btn btn-primary' : 'btn btn-ghost'}
                                disabled={busy}
                                onClick={() => {
                                  const id =
                                    row && typeof row === 'object' && 'id' in row
                                      ? String((row as { id: unknown }).id)
                                      : ''
                                  void post(a.path, { [a.bodyKey || 'instanceId']: id })
                                }}
                              >
                                {a.label}
                              </button>
                            ))}
                          </div>
                        </td>
                      ) : null}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )
    }
    return (
      <pre
        key={view.id || 'json'}
        className="console"
        style={{ whiteSpace: 'pre-wrap', maxHeight: 420, overflow: 'auto' }}
      >
        {rawText || '…'}
      </pre>
    )
  }

  return (
    <div className="page-flow">
      <div className="page-head">
        <div>
          <p className="page-eyebrow">生态 · WASM</p>
          <h2 className="page-title">
            {focusId && ext ? ext.ui?.label || ext.name : '扩展中心'}
          </h2>
        </div>
        <div className="btn-row">
          <button type="button" className="btn btn-ghost" onClick={() => void reload()} disabled={busy}>
            重载插件
          </button>
          <button type="button" className="btn btn-ghost" onClick={onBack}>
            返回主界面
          </button>
        </div>
      </div>

      {!focusId && (
        <div className="stat-grid" style={{ marginBottom: '1rem' }}>
          <div className="card-panel">
            <h3 className="card-title">WASM 宿主</h3>
            <p className="meta">插件在控制面进程内运行，不再依赖 .NET PluginHost。</p>
          </div>
          <div className="card-panel">
            <h3 className="card-title">安装位置</h3>
            <p className="meta">
              <code>data/extensions/&lt;id&gt;/plugin.wasm</code> + <code>plugin.json</code>
            </p>
          </div>
          <div className="card-panel">
            <h3 className="card-title">能力</h3>
            <p className="meta">权限声明在清单里；宿主按能力放开控制面 / 事件 / 出网。</p>
          </div>
          <div className="card-panel">
            <h3 className="card-title">界面</h3>
            <p className="meta">插件用 <code>ui.views</code> 描述面板，管理端按清单渲染。</p>
          </div>
        </div>
      )}

      <div className="card-panel">
        <p className="meta mb-1">
          宿主 {host || 'wasm://in-process'} ·{' '}
          <span className={`badge status-${online ? 'running' : 'stopped'}`}>
            {online ? '在线' : '离线'}
          </span>
        </p>
        {hostError && <p className="error">{hostError}</p>}
      </div>

      {!focusId && (
        <div className="card-panel" style={{ marginTop: '1rem' }}>
          {items.length === 0 ? (
            <p className="meta">
              还没有 WASM 插件。运行 <code>scripts/build-wasm-plugins.ps1</code> 后重载。
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
                          onClick={() => {
                            setActive(p.id)
                            onOpenPlugin?.(p.id)
                          }}
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
      )}

      {active && online && (
        <div className="card-panel" style={{ marginTop: '1rem' }}>
          <h3 className="card-title">{ext?.name ?? active}</h3>
          {ext?.description ? <p className="meta">{ext.description}</p> : null}
          {hint ? <p className="meta">{hint}</p> : null}
          {loadError ? <p className="error">{loadError}</p> : null}
          {views.map((v) => renderView(v))}
        </div>
      )}
    </div>
  )
}
