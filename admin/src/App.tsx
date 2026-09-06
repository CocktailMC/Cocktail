import { useEffect, useMemo, useState } from 'react'
import type { FormEvent } from 'react'
import {
  api,
  eventsWsUrl,
  getToken,
  logsWsUrl,
  setToken,
  type BackupInfo,
  type FileEntry,
  type HealthInfo,
  type Instance,
  type InstanceStatus,
  type LogLine,
  type PlayerInfo,
  type PropertyEntry,
  type Schedule,
  type WorldInfo,
  type CoreVersion,
} from './api'
import CreateInstancePage from './CreateInstancePage'
import EulaPage from './EulaPage'
import BusyOverlay from './BusyOverlay'
import PropertiesPanel from './PropertiesPanel'
import PluginStore from './PluginStore'
import HomePage from './HomePage'
import HomeSettings from './HomeSettings'
import AuditPage from './AuditPage'
import NodesPage from './NodesPage'
import ExtensionsPage from './ExtensionsPage'
import SpecYamlPanel from './SpecYamlPanel'
import SetupPage from './SetupPage'
import LoginPage from './LoginPage'
import './App.css'

const STATUS_LABEL: Record<InstanceStatus, string> = {
  created: '已创建',
  starting: '启动中',
  running: '运行中',
  stopping: '停止中',
  stopped: '已停止',
  crashed: '崩溃',
}

type Tab =
  | 'dashboard'
  | 'control'
  | 'console'
  | 'files'
  | 'backups'
  | 'settings'
  | 'properties'
  | 'version'
  | 'players'
  | 'worlds'
  | 'plugins'
  | 'schedules'

const NAV_GROUPS: {
  label: string
  items: { id: Tab; label: string; icon: string }[]
}[] = [
  {
    label: '概览',
    items: [
      { id: 'dashboard', label: '仪表盘', icon: 'fa-dashboard' },
      { id: 'control', label: '服务器控制', icon: 'fa-power-off' },
      { id: 'players', label: '在线玩家', icon: 'fa-users' },
      { id: 'console', label: '控制台', icon: 'fa-terminal' },
    ],
  },
  {
    label: '内容',
    items: [
      { id: 'properties', label: '服务端配置', icon: 'fa-cog' },
      { id: 'plugins', label: '插件/模组', icon: 'fa-puzzle-piece' },
      { id: 'files', label: '文件', icon: 'fa-folder' },
      { id: 'worlds', label: '世界', icon: 'fa-globe' },
      { id: 'backups', label: '世界备份', icon: 'fa-database' },
    ],
  },
  {
    label: '运维',
    items: [
      { id: 'schedules', label: '计划任务', icon: 'fa-calendar' },
      { id: 'version', label: '版本 / jar', icon: 'fa-download' },
      { id: 'settings', label: '系统设置', icon: 'fa-desktop' },
    ],
  },
]

export default function App() {
  const [instances, setInstances] = useState<Instance[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [logs, setLogs] = useState<LogLine[]>([])
  const [selectedIds, setSelectedIds] = useState<string[]>([])
  const [view, setView] = useState<'manager' | 'create' | 'eula'>('manager')
  const [homeTab, setHomeTab] = useState<
    'overview' | 'settings' | 'audit' | 'nodes' | 'extensions'
  >('overview')
  const [mkdirName, setMkdirName] = useState('')
  const [setCommand, setSetCommand] = useState('java')
  const [setArgs, setSetArgs] = useState('-jar server.jar nogui')
  const [fleet, setFleet] = useState<{
    total: number
    running: number
    stopped: number
    starting: number
    crashed: number
    docker: { available: boolean; message: string }
  } | null>(null)
  const [busy, setBusy] = useState(false)
  const [busyLabel, setBusyLabel] = useState('处理中…')
  const [error, setError] = useState<string | null>(null)
  const [health, setHealth] = useState<string>('…')
  const [envInfo, setEnvInfo] = useState<HealthInfo | null>(null)
  const [tab, setTab] = useState<Tab>('dashboard')
  const [cmd, setCmd] = useState('')

  const [filePath, setFilePath] = useState('')
  const [files, setFiles] = useState<FileEntry[]>([])
  const [editPath, setEditPath] = useState<string | null>(null)
  const [editContent, setEditContent] = useState('')

  const [backups, setBackups] = useState<BackupInfo[]>([])

  const [setNameVal, setSetNameVal] = useState('')
  const [setMem, setSetMem] = useState(1024)
  const [setPort, setSetPort] = useState(25565)
  const [setAuto, setSetAuto] = useState(false)
  const [setEula, setSetEula] = useState(false)
  const [setRuntime, setSetRuntime] = useState<'process' | 'docker'>('process')
  const [setImage, setSetImage] = useState('eclipse-temurin:21-jre')
  const [setCpu, setSetCpu] = useState(1)
  const [setGroup, setSetGroup] = useState('default')
  const [setTags, setSetTags] = useState('')
  const [propsEntries, setPropsEntries] = useState<PropertyEntry[]>([])
  const [propsEpoch, setPropsEpoch] = useState(0)
  const [authRequired, setAuthRequired] = useState(false)
  const [gate, setGate] = useState<
    'loading' | 'setup' | 'login' | 'offline' | 'app'
  >('loading')
  const [adminName, setAdminName] = useState('管理员')
  const [panelName, setPanelName] = useState('Cocktail')
  const [coreVersions, setCoreVersions] = useState<CoreVersion[]>([])
  const [installCore, setInstallCore] = useState<'paper' | 'vanilla'>('paper')
  const [installVer, setInstallVer] = useState('')
  const [players, setPlayers] = useState<PlayerInfo[]>([])
  const [worlds, setWorlds] = useState<WorldInfo[]>([])
  const [plugins, setPlugins] = useState<
    { name: string; path: string; size: number; enabled: boolean }[]
  >([])
  const [schedules, setSchedules] = useState<Schedule[]>([])
  const [schedKind, setSchedKind] = useState<'backup' | 'restart' | 'command'>(
    'backup',
  )
  const [schedSecs, setSchedSecs] = useState(3600)
  const [schedCmd, setSchedCmd] = useState('say scheduled')
  const [importWorldName, setImportWorldName] = useState('world')

  const selected = useMemo(
    () => instances.find((i) => i.id === selectedId) ?? null,
    [instances, selectedId],
  )

  const displayPlayers = useMemo(() => {
    if (players.length) return players
    return (selected?.last_players ?? []).map((n) => ({ name: n }))
  }, [players, selected?.last_players])

  const refresh = async () => {
    const [list, summary] = await Promise.all([
      api.listInstances(),
      api.fleetSummary(),
    ])
    setInstances(list)
    setFleet(summary)
    setSelectedIds((prev) => prev.filter((id) => list.some((i) => i.id === id)))
    setSelectedId((prev) => {
      if (prev && list.some((i) => i.id === prev)) return prev
      return null
    })
  }

  const boot = async () => {
    try {
      const h = await api.health()
      setEnvInfo(h)
      setHealth(`${h.name} ${h.version} · ${h.release}`)
      setAuthRequired(h.auth_required)
      if (h.panel_name) setPanelName(h.panel_name)
      if (h.admin_username) setAdminName(h.admin_username)
      if (h.setup_required) {
        setGate('setup')
        return
      }
      if (!getToken()) {
        setGate('login')
        return
      }
      try {
        const me = await api.me()
        setAdminName(me.username)
        setPanelName(me.panel_name)
        await refresh()
        setGate('app')
      } catch {
        setGate('login')
      }
    } catch {
      setEnvInfo(null)
      setHealth('offline')
      setGate('offline')
    }
  }

  useEffect(() => {
    boot().catch(() => setGate('offline'))
  }, [])

  useEffect(() => {
    if (gate !== 'app') return
    const ws = new WebSocket(eventsWsUrl())
    let statusTimer: ReturnType<typeof setTimeout> | null = null
    ws.onmessage = (ev) => {
      try {
        const msg = JSON.parse(String(ev.data)) as {
          type?: string
          instance_id?: string
          status?: InstanceStatus
          sample?: {
            ts: string
            cpu_pct: number
            memory_mib: number
            tps?: number | null
            players: number
          }
          line?: LogLine
        }
        // Logs already stream on logs WS — never re-list for them.
        if (msg.type === 'log') return
        if (msg.type === 'metric' && msg.instance_id && msg.sample) {
          const sample = msg.sample
          setInstances((prev) =>
            prev.map((inst) =>
              inst.id === msg.instance_id
                ? {
                    ...inst,
                    last_metrics: {
                      ts: sample.ts,
                      cpu_pct: sample.cpu_pct,
                      memory_mib: sample.memory_mib,
                      tps: sample.tps ?? null,
                      players: sample.players,
                    },
                  }
                : inst,
            ),
          )
          return
        }
        if (msg.type === 'status_changed') {
          if (msg.instance_id && msg.status) {
            setInstances((prev) =>
              prev.map((inst) =>
                inst.id === msg.instance_id
                  ? { ...inst, status: msg.status! }
                  : inst,
              ),
            )
          }
          // Debounce: one list+fleet after start/stop bursts, not per log line.
          if (statusTimer) clearTimeout(statusTimer)
          statusTimer = setTimeout(() => {
            refresh().catch(() => undefined)
          }, 400)
          return
        }
      } catch {
        /* ignore malformed */
      }
    }
    return () => {
      if (statusTimer) clearTimeout(statusTimer)
      ws.close()
    }
  }, [gate])

  useEffect(() => {
    if (!selectedId) {
      setLogs([])
      return
    }
    setLogs([])
    const ws = new WebSocket(logsWsUrl(selectedId))
    ws.onmessage = (ev) => {
      try {
        const line = JSON.parse(String(ev.data)) as LogLine
        setLogs((prev) => [...prev.slice(-400), line])
      } catch {
        /* ignore */
      }
    }
    return () => ws.close()
  }, [selectedId])

  useEffect(() => {
    if (!selected) return
    setSetNameVal(selected.spec.name)
    setSetMem(selected.spec.memory_mib)
    setSetPort(selected.spec.port)
    setSetAuto(selected.spec.auto_restart)
    setSetEula(selected.spec.eula_accepted)
    setSetRuntime(selected.spec.runtime ?? 'process')
    setSetImage(selected.spec.docker_image || 'eclipse-temurin:21-jre')
    setSetCpu(selected.spec.cpu_limit ?? 1)
    setSetGroup(selected.spec.group || 'default')
    setSetTags((selected.spec.tags ?? []).join(','))
    setSetCommand(selected.spec.command || 'java')
    setSetArgs(
      (selected.spec.args ?? []).length
        ? selected.spec.args.join(' ')
        : '-jar server.jar nogui',
    )
  }, [selected?.id])

  useEffect(() => {
    if (!selectedId || tab !== 'properties') return
    api
      .getProperties(selectedId)
      .then((list) => {
        setPropsEntries(list)
        setPropsEpoch((n) => n + 1)
      })
      .catch((e: Error) => setError(e.message))
  }, [selectedId, tab])

  useEffect(() => {
    if (tab !== 'version') return
    api
      .listCoreVersions(installCore)
      .then((list) => {
        setCoreVersions(list)
        setInstallVer(list.find((v) => v.latest)?.id ?? list[0]?.id ?? '')
      })
      .catch((e: Error) => setError(e.message))
  }, [tab, installCore])

  useEffect(() => {
    // Cache only — never auto-send `list` (that spams the console).
    if (!selectedId || (tab !== 'players' && tab !== 'dashboard')) return
    api
      .listPlayers(selectedId)
      .then(setPlayers)
      .catch((e: Error) => setError(e.message))
  }, [selectedId, tab])

  useEffect(() => {
    if (!selectedId || tab !== 'worlds') return
    api
      .listWorlds(selectedId)
      .then(setWorlds)
      .catch((e: Error) => setError(e.message))
  }, [selectedId, tab])

  useEffect(() => {
    if (!selectedId || tab !== 'plugins') return
    api
      .listPlugins(selectedId)
      .then(setPlugins)
      .catch((e: Error) => setError(e.message))
  }, [selectedId, tab])

  useEffect(() => {
    if (tab !== 'schedules') return
    api
      .listSchedules()
      .then(setSchedules)
      .catch((e: Error) => setError(e.message))
  }, [tab])

  useEffect(() => {
    if (!selectedId || tab !== 'files') return
    api
      .listFiles(selectedId, filePath)
      .then(setFiles)
      .catch((e: Error) => setError(e.message))
  }, [selectedId, tab, filePath])

  useEffect(() => {
    if (!selectedId || tab !== 'backups') return
    api
      .listBackups(selectedId)
      .then(setBackups)
      .catch((e: Error) => setError(e.message))
  }, [selectedId, tab])

  const beginBusy = (label = '处理中…') => {
    setBusyLabel(label)
    setBusy(true)
    setError(null)
  }

  const endBusy = () => setBusy(false)

  const setBusyState = (v: boolean, label?: string) => {
    if (v) beginBusy(label ?? '处理中…')
    else endBusy()
  }

  const run = async (fn: () => Promise<unknown>, label = '处理中…') => {
    beginBusy(label)
    try {
      await fn()
      await refresh()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      endBusy()
    }
  }

  const sendCmd = async (e: FormEvent) => {
    e.preventDefault()
    if (!selectedId || !cmd.trim()) return
    const value = cmd
    setCmd('')
    await run(() => api.sendCommand(selectedId, value), '发送命令…')
  }

  const openFile = async (path: string, isDir: boolean) => {
    if (!selectedId) return
    if (isDir) {
      setFilePath(path)
      setEditPath(null)
      return
    }
    const lower = path.toLowerCase()
    if (
      lower.endsWith('.jar') ||
      lower.endsWith('.zip') ||
      lower.endsWith('.png') ||
      lower.endsWith('.jpg')
    ) {
      setEditPath(null)
      setError(null)
      return
    }
    beginBusy('读取文件…')
    try {
      const file = await api.readFile(selectedId, path)
      setEditPath(file.path)
      setEditContent(file.content)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      endBusy()
    }
  }

  const effectiveCmd = (inst: Instance) => {
    const cmd = inst.spec.command || 'java'
    const args = inst.spec.args?.length
      ? inst.spec.args.join(' ')
      : '(未配置)'
    return `${cmd} ${args}`
  }

  const saveFile = async () => {
    if (!selectedId || !editPath) return
    await run(() => api.writeFile(selectedId, editPath, editContent))
  }

  const parentPath = filePath.includes('/')
    ? filePath.split('/').slice(0, -1).join('/')
    : ''

  const selectInstance = (id: string) => {
    setSelectedId(id)
    setTab('dashboard')
    setFilePath('')
    setEditPath(null)
    setView('manager')
    setError(null)
  }

  const goHome = (
    tab: 'overview' | 'settings' | 'audit' | 'nodes' | 'extensions' = 'overview',
  ) => {
    setSelectedId(null)
    setHomeTab(tab)
    setView('manager')
    setError(null)
  }

  const ensureEulaOrStart = (inst: Instance) => {
    if (!inst.spec.eula_accepted && inst.spec.core !== 'demo') {
      setSelectedId(inst.id)
      setView('eula')
      return
    }
    run(() => api.startInstance(inst.id), `启动 ${inst.spec.name}…`)
  }

  const memUsed = selected?.last_metrics?.memory_mib
  const memTotal = selected?.spec.memory_mib
  const tpsVal = selected?.last_metrics?.tps
  const playersCount = selected?.last_metrics?.players
  const statusOk = selected?.status === 'running'

  if (gate !== 'app') {
    return (
      <>
        <BusyOverlay
          active={busy || gate === 'loading'}
          label={gate === 'loading' ? '正在连接控制面…' : busyLabel}
        />
        {gate === 'setup' && (
          <SetupPage
            busy={busy}
            onBusy={setBusyState}
            onDone={async (session) => {
              setAdminName(session.username)
              setPanelName(session.panel_name)
              setAuthRequired(true)
              await refresh()
              setGate('app')
            }}
          />
        )}
        {(gate === 'login' || gate === 'offline') && (
          <LoginPage
            hintUsername={adminName !== '管理员' ? adminName : envInfo?.admin_username}
            panelName={panelName}
            busy={busy}
            offline={gate === 'offline'}
            onBusy={setBusyState}
            onRetryHealth={() => {
              setGate('loading')
              boot().catch(() => setGate('offline'))
            }}
            onDone={async (session) => {
              setAdminName(session.username)
              setPanelName(session.panel_name)
              setAuthRequired(true)
              await refresh()
              setGate('app')
            }}
          />
        )}
        {gate === 'loading' && (
          <div className="auth-gate">
            <header className="topbar">
              <div className="topbar-brand">
                <div className="brand-mark" aria-hidden>
                  <img src="/logo.png" alt="" className="brand-logo-img" />
                </div>
                <div>
                  <h1>Cocktail</h1>
                  <span className="brand-sub">Manager · 26Q3</span>
                </div>
              </div>
            </header>
          </div>
        )}
      </>
    )
  }

  return (
    <div className="app-shell">
      <BusyOverlay
        active={busy}
        label={busyLabel}
        statusHint={
          selected?.status === 'starting'
            ? `正在启动 ${selected.spec.name}…`
            : selected?.status === 'stopping'
              ? `正在停止 ${selected.spec.name}…`
              : null
        }
      />
      <header className="topbar">
        <div
          className="topbar-brand"
          role="button"
          tabIndex={0}
          onClick={() => goHome('overview')}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') goHome('overview')
          }}
          title="返回主界面"
          style={{ cursor: 'pointer' }}
        >
          <div className="brand-mark" aria-hidden>
            <img src="/logo.png" alt="" className="brand-logo-img" />
          </div>
          <div>
            <h1>{panelName === 'Cocktail Manager' ? 'Cocktail' : panelName}</h1>
            <span className="brand-sub">Manager · 26Q3</span>
          </div>
        </div>
        <div className="topbar-right">
          <span
            className={`health-pill${health === 'offline' ? ' offline' : ''}`}
            title={health}
          >
            <span className="dot" />
            {health}
            {authRequired ? ' · 已登录' : ''}
          </span>
          <span>
            <i className="fa fa-user-circle" /> {adminName}
          </span>
          <button
            type="button"
            onClick={() => {
              api.logout().finally(() => {
                setToken('')
                setGate('login')
              })
            }}
          >
            <i className="fa fa-sign-out" /> 退出
          </button>
        </div>
      </header>

      <div className="app-body">
        <aside className="sidebar">
          <div className="sidebar-instances">
            <h3>实例</h3>
            <button
              type="button"
              className="btn btn-primary create-entry"
              onClick={() => {
                setError(null)
                setView('create')
              }}
            >
              <i className="fa fa-plus" /> 创建实例
            </button>

            <div className="select-all">
              <label>
                <input
                  type="checkbox"
                  checked={
                    instances.length > 0 &&
                    selectedIds.length === instances.length
                  }
                  onChange={(e) => {
                    setSelectedIds(
                      e.target.checked ? instances.map((i) => i.id) : [],
                    )
                  }}
                />{' '}
                全选
              </label>
            </div>

            <ul className="instance-pick">
              {instances.map((inst) => (
                <li key={inst.id}>
                  <input
                    type="checkbox"
                    checked={selectedIds.includes(inst.id)}
                    onChange={(e) => {
                      setSelectedIds((prev) =>
                        e.target.checked
                          ? [...prev, inst.id]
                          : prev.filter((x) => x !== inst.id),
                      )
                    }}
                  />
                  <button
                    type="button"
                    className={
                      inst.id === selectedId ? 'pick-btn active' : 'pick-btn'
                    }
                    onClick={() => selectInstance(inst.id)}
                  >
                    <span className="pick-name">{inst.spec.name}</span>
                    <span className={`badge status-${inst.status}`}>
                      {STATUS_LABEL[inst.status]}
                    </span>
                  </button>
                </li>
              ))}
              {instances.length === 0 && (
                <li className="empty">还没有实例</li>
              )}
            </ul>
          </div>

          {fleet && (
            <div className="fleet-strip">
              <span>机群 {fleet.total}</span>
              <span className="ok">运行 {fleet.running}</span>
              <span>停止 {fleet.stopped}</span>
              <span>过渡 {fleet.starting}</span>
              <span className="bad">崩溃 {fleet.crashed}</span>
              <span title={fleet.docker.message}>
                Docker {fleet.docker.available ? '就绪' : '不可用'}
              </span>
              <div className="fleet-strip-actions">
                <button
                  type="button"
                  disabled={!selectedIds.length || busy}
                  onClick={() =>
                    run(async () => {
                      await api.fleetBulk('start', selectedIds)
                    }, '批量启动…')
                  }
                >
                  批量启动
                </button>
                <button
                  type="button"
                  disabled={!selectedIds.length || busy}
                  onClick={() =>
                    run(async () => {
                      await api.fleetBulk('stop', selectedIds)
                    }, '批量停止…')
                  }
                >
                  批量停止
                </button>
                <button
                  type="button"
                  disabled={!selectedIds.length || busy}
                  onClick={() =>
                    run(async () => {
                      await api.fleetBulk('restart', selectedIds)
                    }, '批量重启…')
                  }
                >
                  批量重启
                </button>
                <button
                  type="button"
                  disabled={!selectedIds.length || busy}
                  onClick={() =>
                    run(async () => {
                      await api.fleetBulk('delete', selectedIds)
                      setSelectedIds([])
                    }, '批量删除…')
                  }
                >
                  批量删除
                </button>
              </div>
            </div>
          )}

          <nav>
            <div className="nav-group">
              <p className="nav-group-label">机群</p>
              <button
                type="button"
                className={
                  !selected && homeTab === 'overview'
                    ? 'nav-item active'
                    : 'nav-item'
                }
                onClick={() => goHome('overview')}
              >
                <i className="fa fa-th-large" />
                主界面
              </button>
              <button
                type="button"
                className={
                  !selected && homeTab === 'settings'
                    ? 'nav-item active'
                    : 'nav-item'
                }
                onClick={() => goHome('settings')}
              >
                <i className="fa fa-cogs" />
                服务器设置
              </button>
              <button
                type="button"
                className={
                  !selected && homeTab === 'nodes'
                    ? 'nav-item active'
                    : 'nav-item'
                }
                onClick={() => goHome('nodes')}
              >
                <i className="fa fa-sitemap" />
                节点 / Agent
              </button>
              <button
                type="button"
                className={
                  !selected && homeTab === 'extensions'
                    ? 'nav-item active'
                    : 'nav-item'
                }
                onClick={() => goHome('extensions')}
              >
                <i className="fa fa-puzzle-piece" />
                .NET 插件
              </button>
              <button
                type="button"
                className={
                  !selected && homeTab === 'audit'
                    ? 'nav-item active'
                    : 'nav-item'
                }
                onClick={() => goHome('audit')}
              >
                <i className="fa fa-list-alt" />
                审计日志
              </button>
            </div>
            {selected &&
              NAV_GROUPS.map((group) => (
                <div key={group.label} className="nav-group">
                  <p className="nav-group-label">{group.label}</p>
                  {group.items.map((item) => (
                    <button
                      key={item.id}
                      type="button"
                      className={
                        tab === item.id ? 'nav-item active' : 'nav-item'
                      }
                      onClick={() => setTab(item.id)}
                    >
                      <i className={`fa ${item.icon}`} />
                      {item.label}
                    </button>
                  ))}
                </div>
              ))}
          </nav>
        </aside>

        <main className="main">
          {view === 'create' && (
            <CreateInstancePage
              usedPorts={instances.map((i) => i.spec.port)}
              dockerAvailable={fleet?.docker.available ?? false}
              dockerMessage={fleet?.docker.message ?? ''}
              busy={busy}
              onBusy={setBusyState}
              onError={setError}
              onCancel={() => goHome('overview')}
              onCreated={(inst, next) => {
                setSelectedId(inst.id)
                setTab(next)
                setView('manager')
                refresh().catch(() => undefined)
              }}
            />
          )}

          {view === 'eula' && selected && (
            <EulaPage
              instance={selected}
              busy={busy}
              onBusy={setBusyState}
              onError={setError}
              onCancel={() => goHome('overview')}
              onAccepted={(inst) => {
                setInstances((prev) =>
                  prev.map((i) => (i.id === inst.id ? inst : i)),
                )
                setView('manager')
                setTab('control')
                refresh().catch(() => undefined)
              }}
            />
          )}

          {view === 'manager' && !selected && (
            <>
              {error && (
                <div className="error-banner" role="alert">
                  <span>
                    <i className="fa fa-exclamation-circle" /> {error}
                  </span>
                  <button
                    type="button"
                    aria-label="关闭"
                    onClick={() => setError(null)}
                  >
                    ×
                  </button>
                </div>
              )}
              {homeTab === 'overview' ? (
                <HomePage
                  instances={instances}
                  fleet={fleet}
                  health={health}
                  env={envInfo}
                  authRequired={authRequired}
                  busy={busy}
                  selectedIds={selectedIds}
                  onToggleSelect={(id, checked) =>
                    setSelectedIds((prev) =>
                      checked ? [...prev, id] : prev.filter((x) => x !== id),
                    )
                  }
                  onSelectAll={(checked) =>
                    setSelectedIds(checked ? instances.map((i) => i.id) : [])
                  }
                  onOpenInstance={selectInstance}
                  onCreate={() => {
                    setError(null)
                    setView('create')
                  }}
                  onStart={(id) => {
                    const inst = instances.find((i) => i.id === id)
                    if (inst) ensureEulaOrStart(inst)
                  }}
                  onStop={(id) => {
                    const inst = instances.find((i) => i.id === id)
                    run(
                      () => api.stopInstance(id),
                      `停止 ${inst?.spec.name ?? id}…`,
                    )
                  }}
                  onRestart={(id) => {
                    const inst = instances.find((i) => i.id === id)
                    if (
                      inst &&
                      !inst.spec.eula_accepted &&
                      inst.spec.core !== 'demo'
                    ) {
                      setSelectedId(inst.id)
                      setView('eula')
                      return
                    }
                    run(
                      () => api.restartInstance(id),
                      `重启 ${inst?.spec.name ?? id}…`,
                    )
                  }}
                  onBulk={(action) =>
                    run(async () => {
                      await api.fleetBulk(action, selectedIds)
                      if (action === 'delete') setSelectedIds([])
                    }, `批量${action}…`)
                  }
                  onOpenSettings={() => goHome('settings')}
                />
              ) : homeTab === 'nodes' ? (
                <NodesPage
                  onBack={() => goHome('overview')}
                  onError={setError}
                />
              ) : homeTab === 'extensions' ? (
                <ExtensionsPage
                  onBack={() => goHome('overview')}
                  onError={setError}
                />
              ) : homeTab === 'audit' ? (
                <AuditPage
                  instances={instances}
                  onBack={() => goHome('overview')}
                  onOpenInstance={selectInstance}
                  onError={setError}
                />
              ) : (
                <HomeSettings
                  health={health}
                  env={envInfo}
                  dockerAvailable={fleet?.docker.available ?? false}
                  dockerMessage={fleet?.docker.message ?? ''}
                  instanceCount={instances.length}
                  adminName={adminName}
                  onPanelName={setPanelName}
                  onAdminName={setAdminName}
                  onBack={() => goHome('overview')}
                  onBusy={setBusyState}
                  onError={setError}
                />
              )}
            </>
          )}

          {view === 'manager' && selected && (
            <>
              {error && (
                <div className="error-banner" role="alert">
                  <span>
                    <i className="fa fa-exclamation-circle" /> {error}
                  </span>
                  <button type="button" aria-label="关闭" onClick={() => setError(null)}>
                    ×
                  </button>
                </div>
              )}
              {tab === 'dashboard' && (
                <>
                  <div className="page-head">
                    <div>
                      <p className="page-eyebrow">{selected.spec.name}</p>
                      <h2 className="page-title">仪表盘</h2>
                    </div>
                    <span className={`status-pill status-${selected.status}`}>
                      <span className="pulse" />
                      {STATUS_LABEL[selected.status]}
                      {selected.pid ? ` · pid ${selected.pid}` : ''}
                      {selected.reattached ? ' · 已接管' : ''}
                      {` · 节点 ${selected.node_id ?? selected.spec.node_id ?? 'local'}`}
                    </span>
                  </div>
                  <div className="stat-grid">
                    <div className="card-panel stat-card">
                      <div>
                        <p className="label">服务器状态</p>
                        <p className={`value ${statusOk ? 'success' : ''}`}>
                          <i
                            className={`fa ${statusOk ? 'fa-check-circle' : 'fa-circle'}`}
                          />{' '}
                          {STATUS_LABEL[selected.status]}
                        </p>
                      </div>
                      <i
                        className={`fa fa-play-circle icon ${statusOk ? 'success' : ''}`}
                      />
                    </div>
                    <div className="card-panel stat-card">
                      <div>
                        <p className="label">在线玩家</p>
                        <p className="value">
                          {playersCount != null ? String(playersCount) : '—'}
                        </p>
                      </div>
                      <i className="fa fa-users icon primary" />
                    </div>
                    <div className="card-panel stat-card">
                      <div>
                        <p className="label">内存占用</p>
                        <p className="value">
                          {memUsed != null && memTotal != null
                            ? `${memUsed} / ${memTotal} MiB`
                            : `— / ${memTotal ?? '—'} MiB`}
                        </p>
                      </div>
                      <i className="fa fa-microchip icon warning" />
                    </div>
                    <div className="card-panel stat-card">
                      <div>
                        <p className="label">TPS</p>
                        <p
                          className={`value ${tpsVal != null && tpsVal >= 18 ? 'success' : ''}`}
                        >
                          {tpsVal != null ? tpsVal.toFixed(2) : '—'}
                        </p>
                      </div>
                      <i className="fa fa-tachometer icon primary" />
                    </div>
                  </div>

                  <div className="grid-2">
                    <div className="card-panel">
                      <h3 className="card-title">服务器快捷控制</h3>
                      <div className="btn-row">
                        <button
                          type="button"
                          className="btn btn-primary"
                          disabled={busy}
                          onClick={() => {
                            if (
                              !selected.spec.eula_accepted &&
                              selected.spec.core !== 'demo'
                            ) {
                              setView('eula')
                              return
                            }
                            run(() => api.startInstance(selected.id), '启动服务器…')
                          }}
                        >
                          <i className="fa fa-play" /> 启动
                        </button>
                        <button
                          type="button"
                          className="btn btn-warning"
                          disabled={busy}
                          onClick={() =>
                            run(() => api.restartInstance(selected.id), '重启服务器…')
                          }
                        >
                          <i className="fa fa-refresh" /> 重启
                        </button>
                        <button
                          type="button"
                          className="btn btn-danger"
                          disabled={busy}
                          onClick={() =>
                            run(() => api.stopInstance(selected.id), '停止服务器…')
                          }
                        >
                          <i className="fa fa-stop" /> 停止
                        </button>
                        <button
                          type="button"
                          className="btn btn-danger"
                          disabled={busy}
                          onClick={() =>
                            run(async () => {
                              await api.deleteInstance(selected.id)
                              setSelectedId(null)
                            })
                          }
                        >
                          <i className="fa fa-trash" /> 删除
                        </button>
                      </div>
                    </div>
                    <div className="card-panel">
                      <h3 className="card-title">服务器基础信息</h3>
                      <table className="info-table">
                        <tbody>
                          <tr>
                            <td>核心</td>
                            <td>{selected.spec.core}</td>
                          </tr>
                          <tr>
                            <td>端口</td>
                            <td>{selected.spec.port}</td>
                          </tr>
                          <tr>
                            <td>运行时</td>
                            <td>
                              {selected.spec.runtime === 'docker'
                                ? 'Docker'
                                : '进程'}
                            </td>
                          </tr>
                          <tr>
                            <td>分组</td>
                            <td>{selected.spec.group || 'default'}</td>
                          </tr>
                          <tr>
                            <td>工作目录</td>
                            <td>
                              <code>{selected.spec.workdir}</code>
                            </td>
                          </tr>
                          <tr>
                            <td>内存</td>
                            <td>{selected.spec.memory_mib} MiB</td>
                          </tr>
                        </tbody>
                      </table>
                    </div>
                  </div>

                  <div className="card-panel mt-6">
                    <h3 className="card-title">实时控制台输出</h3>
                    <pre className="console-box">
                      {logs.length === 0
                        ? '等待日志…（启动后出现；支持历史缓冲）'
                        : logs.map((l) => l.line).join('\n')}
                    </pre>
                    <form className="cmd-row" onSubmit={sendCmd}>
                      <input
                        value={cmd}
                        onChange={(e) => setCmd(e.target.value)}
                        placeholder="输入服务器命令，例如 list / say hello"
                        disabled={busy || selected.status !== 'running'}
                      />
                      <button
                        type="submit"
                        className="btn btn-primary"
                        disabled={busy || selected.status !== 'running'}
                      >
                        发送命令
                      </button>
                    </form>
                  </div>

                  {displayPlayers.length > 0 && (
                    <div className="card-panel mt-6">
                      <h3 className="card-title">在线玩家列表</h3>
                      <table className="data-table">
                        <thead>
                          <tr>
                            <th>玩家名</th>
                            <th>操作</th>
                          </tr>
                        </thead>
                        <tbody>
                          {displayPlayers.map((p) => (
                            <tr key={p.name}>
                              <td>{p.name}</td>
                              <td>
                                {(['kick', 'ban', 'op', 'deop'] as const).map(
                                  (a) => (
                                    <button
                                      key={a}
                                      type="button"
                                      className={
                                        a === 'ban'
                                          ? 'link-btn danger'
                                          : a === 'kick'
                                            ? 'link-btn warn'
                                            : 'link-btn'
                                      }
                                      disabled={
                                        busy || selected.status !== 'running'
                                      }
                                      onClick={() =>
                                        run(() =>
                                          api.playerAction(
                                            selected.id,
                                            p.name,
                                            a,
                                          ),
                                        )
                                      }
                                    >
                                      {a}
                                    </button>
                                  ),
                                )}
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  )}
                </>
              )}

              {tab === 'control' && (
                <>
                  <div className="page-head">
                    <div>
                      <p className="page-eyebrow">{selected.spec.name}</p>
                      <h2 className="page-title">服务器控制</h2>
                    </div>
                    <span className={`status-pill status-${selected.status}`}>
                      <span className="pulse" />
                      {STATUS_LABEL[selected.status]}
                    </span>
                  </div>
                  <div className="card-panel">
                    <h3 className="card-title">{selected.spec.name}</h3>
                    <p className="meta mb-1">
                      {selected.spec.core} · :{selected.spec.port} ·{' '}
                      {STATUS_LABEL[selected.status]} ·{' '}
                      {selected.spec.runtime === 'docker' ? 'Docker' : '进程'}
                    </p>
                    <p className="meta mb-1">
                      启动：<code>{effectiveCmd(selected)}</code>
                    </p>
                    {!selected.spec.command &&
                      selected.spec.core !== 'demo' && (
                        <p className="error" style={{ marginBottom: '0.75rem' }}>
                          尚未配置 jar。请到「版本/导入jar」上传或下载核心。
                        </p>
                      )}
                    <div className="btn-row mt-4">
                      {!selected.spec.eula_accepted &&
                        selected.spec.core !== 'demo' && (
                          <button
                            type="button"
                            className="btn btn-ghost"
                            disabled={busy}
                            onClick={() => setView('eula')}
                          >
                            <i className="fa fa-file-text-o" /> 阅读并同意 EULA
                          </button>
                        )}
                      <button
                        type="button"
                        className="btn btn-primary"
                        disabled={busy}
                        onClick={() => {
                          if (
                            !selected.spec.eula_accepted &&
                            selected.spec.core !== 'demo'
                          ) {
                            setView('eula')
                            return
                          }
                          run(() => api.startInstance(selected.id), '启动服务器…')
                        }}
                      >
                        <i className="fa fa-play" /> 启动服务器
                      </button>
                      <button
                        type="button"
                        className="btn btn-warning"
                        disabled={busy}
                        onClick={() =>
                          run(() => api.restartInstance(selected.id), '重启服务器…')
                        }
                      >
                        <i className="fa fa-refresh" /> 重启服务器
                      </button>
                      <button
                        type="button"
                        className="btn btn-danger"
                        disabled={busy}
                        onClick={() =>
                          run(() => api.stopInstance(selected.id), '停止服务器…')
                        }
                      >
                        <i className="fa fa-stop" /> 停止服务器
                      </button>
                      <button
                        type="button"
                        className="btn btn-danger"
                        disabled={busy}
                        onClick={() =>
                          run(async () => {
                            await api.deleteInstance(selected.id)
                            setSelectedId(null)
                          })
                        }
                      >
                        <i className="fa fa-trash" /> 删除实例
                      </button>
                    </div>
                  </div>
                </>
              )}

              {tab === 'console' && (
                <>
                  <div className="page-head">
                    <div>
                      <p className="page-eyebrow">{selected.spec.name}</p>
                      <h2 className="page-title">控制台</h2>
                    </div>
                  </div>
                  <div className="card-panel">
                    <pre className="console-box tall">
                      {logs.length === 0
                        ? '等待日志…（启动后出现；支持历史缓冲）'
                        : logs.map((l) => l.line).join('\n')}
                    </pre>
                    <form className="cmd-row" onSubmit={sendCmd}>
                      <input
                        value={cmd}
                        onChange={(e) => setCmd(e.target.value)}
                        placeholder="输入命令，如 list / say hello"
                        disabled={busy || selected.status !== 'running'}
                      />
                      <button
                        type="submit"
                        className="btn btn-primary"
                        disabled={busy || selected.status !== 'running'}
                      >
                        发送
                      </button>
                    </form>
                  </div>
                </>
              )}

              {tab === 'version' && (
                <>
                  <div className="page-head">
                    <div>
                      <p className="page-eyebrow">{selected.spec.name}</p>
                      <h2 className="page-title">版本 / 导入 jar</h2>
                    </div>
                  </div>
                  <div className="card-panel">
                    <p className="meta" style={{ marginBottom: '1rem' }}>
                      当前核心：{selected.spec.core} · 启动命令：{' '}
                      <code>{effectiveCmd(selected)}</code>
                      {selected.spec.runtime === 'docker'
                        ? ` · 容器镜像 ${selected.spec.docker_image || 'eclipse-temurin:21-jre'}`
                        : ' · 本机进程'}
                    </p>
                    <div className="settings">
                      <label className="upload-btn btn btn-primary">
                        <i className="fa fa-upload" /> 导入自定义 server.jar
                        <input
                          type="file"
                          accept=".jar"
                          hidden
                          disabled={
                            busy ||
                            selected.status === 'running' ||
                            selected.status === 'starting'
                          }
                          onChange={(e) => {
                            const file = e.target.files?.[0]
                            if (!file) return
                            run(async () => {
                              await api.installJar(selected.id, file, {
                                path: 'server.jar',
                                core: 'custom',
                                accept_eula: selected.spec.eula_accepted,
                              })
                            }, `导入 jar：${file.name}`)
                            e.target.value = ''
                          }}
                        />
                      </label>
                      <p className="meta">
                        导入后自动配置：java -jar server.jar nogui，并按设置注入
                        -Xmx/-Xms；容器运行时映射 主机端口→容器 25565。
                      </p>
                      <hr style={{ margin: '1.25rem 0', borderColor: '#e5eaf0' }} />
                      <label>
                        在线核心
                        <select
                          value={installCore}
                          onChange={(e) =>
                            setInstallCore(
                              e.target.value as 'paper' | 'vanilla',
                            )
                          }
                        >
                          <option value="paper">Paper</option>
                          <option value="vanilla">Vanilla</option>
                        </select>
                      </label>
                      <label>
                        版本
                        <select
                          value={installVer}
                          onChange={(e) => setInstallVer(e.target.value)}
                        >
                          {coreVersions.map((v) => (
                            <option key={v.id} value={v.id}>
                              {v.id}
                              {v.latest ? ' (latest)' : ''}
                            </option>
                          ))}
                        </select>
                      </label>
                      <button
                        type="button"
                        className="btn btn-primary"
                        disabled={
                          busy ||
                          !installVer ||
                          selected.status === 'running' ||
                          selected.status === 'starting'
                        }
                        onClick={() =>
                          run(
                            () =>
                              api.installCore(
                                selected.id,
                                installCore,
                                installVer,
                              ),
                            `下载安装 ${installCore} ${installVer}…`,
                          )
                        }
                      >
                        <i className="fa fa-download" /> 下载并安装（自动配置启动命令）
                      </button>
                    </div>
                  </div>
                </>
              )}

              {tab === 'players' && (
                <>
                  <div className="page-head">
                    <div>
                      <p className="page-eyebrow">{selected.spec.name}</p>
                      <h2 className="page-title">在线玩家</h2>
                    </div>
                  </div>
                  <div className="card-panel">
                    <div className="files-toolbar">
                      <button
                        type="button"
                        className="btn btn-ghost"
                        disabled={busy || selected.status !== 'running'}
                        onClick={() =>
                          run(async () => {
                            setPlayers(
                              await api.listPlayers(selected.id, {
                                probe: true,
                              }),
                            )
                          })
                        }
                      >
                        <i className="fa fa-refresh" /> 刷新 list
                      </button>
                    </div>
                    {displayPlayers.length === 0 ? (
                      <p className="empty">暂无在线玩家（启动后点刷新）</p>
                    ) : (
                      <table className="data-table">
                        <thead>
                          <tr>
                            <th>玩家名</th>
                            <th>操作</th>
                          </tr>
                        </thead>
                        <tbody>
                          {displayPlayers.map((p) => (
                            <tr key={p.name}>
                              <td>
                                <strong>{p.name}</strong>
                              </td>
                              <td>
                                {(['kick', 'ban', 'op', 'deop'] as const).map(
                                  (a) => (
                                    <button
                                      key={a}
                                      type="button"
                                      className={
                                        a === 'ban'
                                          ? 'link-btn danger'
                                          : a === 'kick'
                                            ? 'link-btn warn'
                                            : 'link-btn'
                                      }
                                      disabled={
                                        busy || selected.status !== 'running'
                                      }
                                      onClick={() =>
                                        run(() =>
                                          api.playerAction(
                                            selected.id,
                                            p.name,
                                            a,
                                          ),
                                        )
                                      }
                                    >
                                      {a}
                                    </button>
                                  ),
                                )}
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    )}
                  </div>
                </>
              )}

              {tab === 'worlds' && (
                <>
                  <div className="page-head">
                    <div>
                      <p className="page-eyebrow">{selected.spec.name}</p>
                      <h2 className="page-title">世界</h2>
                    </div>
                  </div>
                  <div className="card-panel">
                    <div className="files-toolbar">
                      <input
                        className="form-input"
                        value={importWorldName}
                        onChange={(e) => setImportWorldName(e.target.value)}
                        placeholder="导入世界名"
                        style={{ maxWidth: 140 }}
                      />
                      <label className="upload-btn">
                        导入 zip
                        <input
                          type="file"
                          accept=".zip"
                          hidden
                          onChange={(e) => {
                            const file = e.target.files?.[0]
                            if (!file) return
                            run(async () => {
                              await api.importWorld(
                                selected.id,
                                importWorldName,
                                file,
                              )
                              setWorlds(await api.listWorlds(selected.id))
                            })
                            e.target.value = ''
                          }}
                        />
                      </label>
                    </div>
                    <ul className="backup-list">
                      {worlds.map((w) => (
                        <li key={w.name} className="backup-row">
                          <div>
                            <strong>{w.name}</strong>
                            <span className="meta">
                              {' '}
                              {(w.size_bytes / 1024 / 1024).toFixed(2)} MiB
                            </span>
                          </div>
                          <div className="actions">
                            <button
                              type="button"
                              className="btn btn-ghost"
                              disabled={busy}
                              onClick={() =>
                                run(() => api.exportWorld(selected.id, w.name))
                              }
                            >
                              导出
                            </button>
                            <button
                              type="button"
                              className="btn btn-danger"
                              disabled={busy || selected.status === 'running'}
                              onClick={() =>
                                run(async () => {
                                  await api.resetWorld(selected.id, w.name)
                                  setWorlds(await api.listWorlds(selected.id))
                                })
                              }
                            >
                              重置
                            </button>
                          </div>
                        </li>
                      ))}
                      {worlds.length === 0 && (
                        <li className="empty">未检测到世界目录</li>
                      )}
                    </ul>
                  </div>
                </>
              )}

              {tab === 'plugins' && (
                <>
                  <div className="page-head">
                    <div>
                      <p className="page-eyebrow">{selected.spec.name}</p>
                      <h2 className="page-title">插件 / 模组</h2>
                    </div>
                  </div>
                  <div className="card-panel" style={{ marginBottom: '1rem' }}>
                    <PluginStore
                      instanceId={selected.id}
                      busy={busy}
                      defaultLoader={
                        selected.spec.core === 'fabric' ? 'fabric' : 'paper'
                      }
                      onInstalled={() =>
                        api.listPlugins(selected.id).then(setPlugins)
                      }
                      run={run}
                    />
                  </div>
                  <div className="card-panel">
                    <div className="store-head" style={{ marginBottom: '0.5rem' }}>
                      <div>
                        <h3 className="card-title">已安装</h3>
                        <p className="store-sub">
                          {plugins.length
                            ? `${plugins.length} 个 jar`
                            : 'plugins/ 与 mods/ 目前为空'}
                        </p>
                      </div>
                      <div className="files-toolbar">
                        <label className="upload-btn">
                          上传插件
                          <input
                            type="file"
                            accept=".jar"
                            hidden
                            onChange={(e) => {
                              const file = e.target.files?.[0]
                              if (!file) return
                              run(async () => {
                                await api.uploadFile(
                                  selected.id,
                                  `plugins/${file.name}`,
                                  file,
                                )
                                setPlugins(await api.listPlugins(selected.id))
                              }, `上传插件 ${file.name}…`)
                              e.target.value = ''
                            }}
                          />
                        </label>
                        <label className="upload-btn">
                          上传模组
                          <input
                            type="file"
                            accept=".jar"
                            hidden
                            onChange={(e) => {
                              const file = e.target.files?.[0]
                              if (!file) return
                              run(async () => {
                                await api.uploadFile(
                                  selected.id,
                                  `mods/${file.name}`,
                                  file,
                                )
                                setPlugins(await api.listPlugins(selected.id))
                              }, `上传模组 ${file.name}…`)
                              e.target.value = ''
                            }}
                          />
                        </label>
                      </div>
                    </div>
                    <ul className="plugin-installed">
                      {plugins.map((p) => {
                        const isMod = p.path.startsWith('mods/')
                        return (
                          <li key={p.path} className="plugin-row">
                            <div className="plugin-row-main">
                              <span
                                className={`plugin-kind${isMod ? ' mods' : ''}`}
                              >
                                {isMod ? 'mods' : 'plugins'}
                              </span>
                              <div>
                                <strong>{p.name}</strong>
                                <span className="meta">
                                  {p.enabled ? '启用' : '禁用'} ·{' '}
                                  {(p.size / 1024).toFixed(1)} KiB
                                </span>
                              </div>
                            </div>
                            <button
                              type="button"
                              className="btn btn-ghost"
                              disabled={busy}
                              onClick={() =>
                                run(async () => {
                                  if (p.enabled) {
                                    await api.disablePlugin(selected.id, p.name)
                                  } else {
                                    await api.enablePlugin(selected.id, p.name)
                                  }
                                  setPlugins(await api.listPlugins(selected.id))
                                })
                              }
                            >
                              {p.enabled ? '禁用' : '启用'}
                            </button>
                          </li>
                        )
                      })}
                      {plugins.length === 0 && (
                        <li className="store-empty">
                          <i className="fa fa-puzzle-piece" />
                          <span>还没有安装插件或模组</span>
                        </li>
                      )}
                    </ul>
                  </div>
                </>
              )}

              {tab === 'schedules' && (
                <>
                  <div className="page-head">
                    <div>
                      <p className="page-eyebrow">{selected.spec.name}</p>
                      <h2 className="page-title">计划任务</h2>
                    </div>
                  </div>
                  <div className="card-panel">
                    <div className="settings">
                      <label>
                        类型
                        <select
                          value={schedKind}
                          onChange={(e) =>
                            setSchedKind(
                              e.target.value as
                                | 'backup'
                                | 'restart'
                                | 'command',
                            )
                          }
                        >
                          <option value="backup">定时备份</option>
                          <option value="restart">定时重启</option>
                          <option value="command">定时命令</option>
                        </select>
                      </label>
                      <label>
                        间隔（秒，≥30）
                        <input
                          type="number"
                          min={30}
                          value={schedSecs}
                          onChange={(e) =>
                            setSchedSecs(Number(e.target.value))
                          }
                        />
                      </label>
                      {schedKind === 'command' && (
                        <label>
                          命令
                          <input
                            value={schedCmd}
                            onChange={(e) => setSchedCmd(e.target.value)}
                          />
                        </label>
                      )}
                      <button
                        type="button"
                        className="btn btn-primary"
                        disabled={busy}
                        onClick={() =>
                          run(async () => {
                            await api.createSchedule({
                              instance_id: selected.id,
                              kind: schedKind,
                              every_secs: schedSecs,
                              command:
                                schedKind === 'command' ? schedCmd : undefined,
                            })
                            setSchedules(await api.listSchedules())
                          })
                        }
                      >
                        <i className="fa fa-plus" /> 添加定时任务
                      </button>
                    </div>
                    <ul className="backup-list mt-4">
                      {schedules
                        .filter((s) => s.instance_id === selected.id)
                        .map((s) => (
                          <li key={s.id} className="backup-row">
                            <div>
                              <strong>
                                {s.kind} / {s.every_secs}s
                              </strong>
                              <span className="meta">
                                {' '}
                                next {new Date(s.next_run_at).toLocaleString()}
                              </span>
                            </div>
                            <button
                              type="button"
                              className="btn btn-danger"
                              disabled={busy}
                              onClick={() =>
                                run(async () => {
                                  await api.deleteSchedule(s.id)
                                  setSchedules(await api.listSchedules())
                                })
                              }
                            >
                              删除
                            </button>
                          </li>
                        ))}
                    </ul>
                  </div>
                </>
              )}

              {tab === 'files' && (
                <>
                  <div className="page-head">
                    <div>
                      <p className="page-eyebrow">{selected.spec.name}</p>
                      <h2 className="page-title">文件</h2>
                    </div>
                  </div>
                  <div className="card-panel">
                    <div className="files-toolbar">
                      <button
                        type="button"
                        className="btn btn-ghost"
                        disabled={!filePath}
                        onClick={() => openFile(parentPath, true)}
                      >
                        上级
                      </button>
                      <label className="upload-btn">
                        上传文件
                        <input
                          type="file"
                          hidden
                          onChange={(e) => {
                            const file = e.target.files?.[0]
                            if (!file || !selectedId) return
                            const dest = filePath
                              ? `${filePath}/${file.name}`
                              : file.name
                            run(async () => {
                              await api.uploadFile(selectedId, dest, file)
                              if (
                                file.name.toLowerCase().endsWith('.jar') &&
                                (!filePath || filePath === '.')
                              ) {
                                const useAsServer =
                                  file.name.toLowerCase() === 'server.jar' ||
                                  window.confirm(
                                    `已上传 ${file.name}。设为启动 jar（java -jar … nogui）？`,
                                  )
                                if (useAsServer) {
                                  await api.setStartupJar(selectedId, dest)
                                }
                              }
                              setFiles(
                                await api.listFiles(selectedId, filePath),
                              )
                            }, `上传 ${file.name}…`)
                            e.target.value = ''
                          }}
                        />
                      </label>
                      <input
                        className="form-input"
                        style={{ maxWidth: 140 }}
                        value={mkdirName}
                        onChange={(e) => setMkdirName(e.target.value)}
                        placeholder="新文件夹名"
                      />
                      <button
                        type="button"
                        className="btn btn-ghost"
                        disabled={busy || !mkdirName.trim()}
                        onClick={() => {
                          if (!selectedId || !mkdirName.trim()) return
                          const dest = filePath
                            ? `${filePath}/${mkdirName.trim()}`
                            : mkdirName.trim()
                          run(async () => {
                            await api.mkdir(selectedId, dest)
                            setMkdirName('')
                            setFiles(await api.listFiles(selectedId, filePath))
                          })
                        }}
                      >
                        新建文件夹
                      </button>
                      <span className="path-label">/{filePath || ''}</span>
                    </div>
                    <div className="files-grid">
                      <ul className="file-list">
                        {files.map((f) => (
                          <li key={f.path} className="file-item">
                            <button
                              type="button"
                              className="file-row"
                              onClick={() => openFile(f.path, f.is_dir)}
                            >
                              <span>
                                <span className="file-kind">
                                  {f.is_dir ? 'DIR' : 'FILE'}
                                </span>{' '}
                                {f.name}
                              </span>
                              {!f.is_dir && (
                                <span className="file-size">
                                  {f.size > 1024 * 1024
                                    ? `${(f.size / 1024 / 1024).toFixed(1)} MiB`
                                    : `${Math.max(1, Math.round(f.size / 1024))} KiB`}
                                </span>
                              )}
                            </button>
                            <div className="file-actions">
                              {!f.is_dir && (
                                <a
                                  className="link-btn"
                                  href={api.downloadUrl(selected.id, f.path)}
                                >
                                  下载
                                </a>
                              )}
                              {!f.is_dir &&
                                f.name.toLowerCase().endsWith('.jar') && (
                                  <button
                                    type="button"
                                    className="link-btn"
                                    disabled={
                                      busy ||
                                      selected.status === 'running' ||
                                      selected.status === 'starting'
                                    }
                                    onClick={() =>
                                      run(() =>
                                        api.setStartupJar(selected.id, f.path),
                                      )
                                    }
                                  >
                                    设为启动
                                  </button>
                                )}
                              <button
                                type="button"
                                className="link-btn danger"
                                disabled={busy}
                                onClick={() => {
                                  if (
                                    !window.confirm(
                                      `删除 ${f.path}？${f.is_dir ? '（目录将递归删除）' : ''}`,
                                    )
                                  )
                                    return
                                  run(async () => {
                                    await api.deleteFile(selected.id, f.path)
                                    if (editPath === f.path) setEditPath(null)
                                    setFiles(
                                      await api.listFiles(
                                        selected.id,
                                        filePath,
                                      ),
                                    )
                                  })
                                }}
                              >
                                删除
                              </button>
                            </div>
                          </li>
                        ))}
                      </ul>
                      <div className="editor">
                        {editPath ? (
                          <>
                            <div className="editor-head">
                              <span>{editPath}</span>
                              <div className="actions">
                                <a
                                  className="link-btn"
                                  href={api.downloadUrl(selected.id, editPath)}
                                >
                                  下载
                                </a>
                                <button
                                  type="button"
                                  className="btn btn-primary"
                                  disabled={busy}
                                  onClick={saveFile}
                                >
                                  保存
                                </button>
                              </div>
                            </div>
                            <textarea
                              value={editContent}
                              onChange={(e) => setEditContent(e.target.value)}
                              spellCheck={false}
                            />
                          </>
                        ) : (
                          <p className="empty" style={{ padding: '1rem' }}>
                            选择文本文件编辑；jar/zip 请用下载或「设为启动」
                          </p>
                        )}
                      </div>
                    </div>
                  </div>
                </>
              )}

              {tab === 'backups' && (
                <>
                  <div className="page-head">
                    <div>
                      <p className="page-eyebrow">{selected.spec.name}</p>
                      <h2 className="page-title">世界备份</h2>
                    </div>
                  </div>
                  <div className="card-panel">
                    <div className="files-toolbar">
                      <button
                        type="button"
                        className="btn btn-primary"
                        disabled={busy}
                        onClick={() =>
                          run(async () => {
                            await api.createBackup(selected.id)
                            setBackups(await api.listBackups(selected.id))
                          })
                        }
                      >
                        <i className="fa fa-database" /> 立即备份
                      </button>
                    </div>
                    <ul className="backup-list">
                      {backups.map((b) => (
                        <li key={b.id} className="backup-row">
                          <div>
                            <strong>{b.id}</strong>
                            <span className="meta">
                              {' '}
                              {(b.size_bytes / 1024).toFixed(1)} KiB · {b.path}
                            </span>
                          </div>
                          <div className="actions">
                            <button
                              type="button"
                              className="btn btn-ghost"
                              disabled={busy || selected.status === 'running'}
                              onClick={() =>
                                run(async () => {
                                  await api.restoreBackup(selected.id, b.id)
                                })
                              }
                            >
                              恢复
                            </button>
                            <button
                              type="button"
                              className="btn btn-danger"
                              disabled={busy}
                              onClick={() =>
                                run(async () => {
                                  await api.deleteBackup(selected.id, b.id)
                                  setBackups(
                                    await api.listBackups(selected.id),
                                  )
                                })
                              }
                            >
                              删除
                            </button>
                          </div>
                        </li>
                      ))}
                      {backups.length === 0 && (
                        <li className="empty">暂无备份</li>
                      )}
                    </ul>
                  </div>
                </>
              )}

              {tab === 'properties' && (
                <>
                  <div className="page-head">
                    <div>
                      <p className="page-eyebrow">{selected.spec.name}</p>
                      <h2 className="page-title">服务端配置</h2>
                    </div>
                  </div>
                  <p className="meta" style={{ marginBottom: '0.75rem' }}>
                    编辑 <code>server.properties</code>
                    。常用项已分组；也可切换「全部键值」或添加自定义项。
                  </p>
                  <PropertiesPanel
                    entries={propsEntries}
                    onChange={setPropsEntries}
                    busy={busy}
                    running={selected.status === 'running'}
                    cleanEpoch={propsEpoch}
                    onReload={() =>
                      run(async () => {
                        const list = await api.getProperties(selected.id)
                        setPropsEntries(list)
                        setPropsEpoch((n) => n + 1)
                      }, '加载配置…')
                    }
                    onSave={() =>
                      run(async () => {
                        const list = await api.setProperties(
                          selected.id,
                          propsEntries,
                        )
                        setPropsEntries(list)
                        setPropsEpoch((n) => n + 1)
                      }, '保存 server.properties…')
                    }
                  />
                </>
              )}

              {tab === 'settings' && (
                <>
                  <div className="page-head">
                    <div>
                      <p className="page-eyebrow">{selected.spec.name}</p>
                      <h2 className="page-title">系统设置</h2>
                    </div>
                  </div>
                  <div className="card-panel">
                    <form
                      className="settings"
                      onSubmit={(e) => {
                        e.preventDefault()
                        run(() =>
                          api.updateInstance(selected.id, {
                            name: setNameVal,
                            memory_mib: setMem,
                            port: setPort,
                            auto_restart: setAuto,
                            eula_accepted: setEula,
                            runtime: setRuntime,
                            docker_image: setImage,
                            cpu_limit: setCpu,
                            command: setCommand.trim() || 'java',
                            args: setArgs
                              .trim()
                              .split(/\s+/)
                              .filter(Boolean),
                            group: setGroup,
                            tags: setTags
                              .split(',')
                              .map((t) => t.trim())
                              .filter(Boolean),
                          }),
                        )
                      }}
                    >
                      <label>
                        名称
                        <input
                          value={setNameVal}
                          onChange={(e) => setSetNameVal(e.target.value)}
                        />
                      </label>
                      <label>
                        运行时
                        <select
                          value={setRuntime}
                          onChange={(e) =>
                            setSetRuntime(
                              e.target.value as 'process' | 'docker',
                            )
                          }
                        >
                          <option value="process">本机进程</option>
                          <option value="docker">Docker 容器</option>
                        </select>
                      </label>
                      {setRuntime === 'docker' && (
                        <>
                          <label>
                            镜像
                            <input
                              value={setImage}
                              onChange={(e) => setSetImage(e.target.value)}
                              placeholder="eclipse-temurin:21-jre"
                            />
                          </label>
                          <label>
                            CPU 限制
                            <input
                              type="number"
                              min={0.1}
                              step={0.1}
                              value={setCpu}
                              onChange={(e) =>
                                setSetCpu(Number(e.target.value))
                              }
                            />
                          </label>
                        </>
                      )}
                      <label>
                        启动命令
                        <input
                          value={setCommand}
                          onChange={(e) => setSetCommand(e.target.value)}
                          placeholder="java"
                        />
                      </label>
                      <label>
                        启动参数（空格分隔；内存会自动注入 -Xmx/-Xms）
                        <input
                          value={setArgs}
                          onChange={(e) => setSetArgs(e.target.value)}
                          placeholder="-jar server.jar nogui"
                        />
                      </label>
                      <p className="meta">
                        预览：{setCommand} {setArgs}
                      </p>
                      <label>
                        分组
                        <input
                          value={setGroup}
                          onChange={(e) => setSetGroup(e.target.value)}
                        />
                      </label>
                      <label>
                        标签（逗号分隔）
                        <input
                          value={setTags}
                          onChange={(e) => setSetTags(e.target.value)}
                        />
                      </label>
                      <label>
                        内存 (MiB) — 进程注入 -Xmx；容器同时 --memory
                        <input
                          type="number"
                          min={256}
                          value={setMem}
                          onChange={(e) => setSetMem(Number(e.target.value))}
                        />
                      </label>
                      <label>
                        端口 — 写入 server.properties；容器映射 host:25565
                        <input
                          type="number"
                          min={1}
                          max={65535}
                          value={setPort}
                          onChange={(e) => setSetPort(Number(e.target.value))}
                        />
                      </label>
                      <label className="check">
                        <input
                          type="checkbox"
                          checked={setAuto}
                          onChange={(e) => setSetAuto(e.target.checked)}
                        />
                        崩溃自动重启
                      </label>
                      <label className="check">
                        <input
                          type="checkbox"
                          checked={setEula}
                          onChange={(e) => {
                            if (e.target.checked && !selected.spec.eula_accepted) {
                              setView('eula')
                              return
                            }
                            setSetEula(e.target.checked)
                          }}
                        />
                        已同意 Mojang EULA
                        {!selected.spec.eula_accepted && (
                          <button
                            type="button"
                            className="link-btn"
                            onClick={() => setView('eula')}
                          >
                            打开协议页
                          </button>
                        )}
                      </label>
                      <p className="meta">工作目录：{selected.spec.workdir}</p>
                      <p className="meta">
                        节点：{selected.node_id ?? selected.spec.node_id ?? 'local'}
                        {selected.desired_running ?? selected.spec.desired_running
                          ? ' · 期望运行'
                          : ' · 期望停止'}
                        {selected.generation != null
                          ? ` · gen ${selected.generation}`
                          : ''}
                      </p>
                      <button
                        type="submit"
                        className="btn btn-primary"
                        disabled={busy}
                      >
                        保存设置
                      </button>
                    </form>
                  </div>
                  <SpecYamlPanel
                    instanceId={selected.id}
                    busy={busy}
                    onBusy={setBusyState}
                    onError={setError}
                  />
                </>
              )}
            </>
          )}
          {error && !(view === 'manager' && selected) && (
            <div className="error-banner" role="alert">
              <span>
                <i className="fa fa-exclamation-circle" /> {error}
              </span>
              <button type="button" aria-label="关闭" onClick={() => setError(null)}>
                ×
              </button>
            </div>
          )}
        </main>
      </div>
    </div>
  )
}
