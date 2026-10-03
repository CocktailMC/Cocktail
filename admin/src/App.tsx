import { useEffect, useMemo, useRef, useState } from 'react'
import type { FormEvent } from 'react'
import {
  api,
  eventsWsUrl,
  getToken,
  logsWsUrl,
  setToken,
  formatBps,
  formatDuration,
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
  type CoreLoader,
  type MetricSample,
  type PanelEvent,
  type DownloadProgress,
} from './api'
import CreateInstancePage from './CreateInstancePage'
import EulaPage from './EulaPage'
import BusyOverlay from './BusyOverlay'
import PropertiesPanel from './PropertiesPanel'
import PluginStore from './PluginStore'
import HomePage from './HomePage'
import HomeSettings from './HomeSettings'
import AuditPage from './AuditPage'
import NetworkPage from './NetworkPage'
import GlobalNetworkPage from './GlobalNetworkPage'
import NodesPage from './NodesPage'
import ExtensionsPage from './ExtensionsPage'
import { parseHash, writeHash, type HomeTab } from './hashRoute'
import AutomationsPage from './AutomationsPage'
import EventFeed from './EventFeed'
import UsersPage from './UsersPage'
import SpecYamlPanel from './SpecYamlPanel'
import SetupPage from './SetupPage'
import LoginPage from './LoginPage'
import {
  INSTALLABLE_CORE_GROUPS,
  JAVA_MAJORS,
  coreHasLoaders,
  defaultModrinthLoader,
  defaultModrinthProjectType,
  isInstallableCore,
  recommendedJavaMajor,
} from './cores'
import './App.css'
import './shell.css'
import './pages.css'
import InstanceRail from './InstanceRail'
import DashPane from './DashPane'

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
  | 'automations'
  | 'network'
  | 'worlds'
  | 'plugins'
  | 'schedules'

const PRIMARY_TABS: { id: Tab; label: string }[] = [
  { id: 'dashboard', label: '仪表盘' },
  { id: 'console', label: '控制台' },
  { id: 'players', label: '玩家' },
  { id: 'network', label: '网络' },
  { id: 'control', label: '控制' },
]

const MORE_TABS: { id: Tab; label: string }[] = [
  { id: 'automations', label: '自动化' },
  { id: 'properties', label: '服务端配置' },
  { id: 'plugins', label: '插件/模组' },
  { id: 'files', label: '文件' },
  { id: 'worlds', label: '世界' },
  { id: 'backups', label: '备份' },
  { id: 'schedules', label: '计划任务' },
  { id: 'version', label: '版本 / 导入' },
  { id: 'settings', label: '系统设置' },
]

const PLANE_LINKS: { tab: HomeTab; label: string }[] = [
  { tab: 'overview', label: '机群总览' },
  { tab: 'network', label: '全局网络' },
  { tab: 'events', label: '事件中心' },
  { tab: 'users', label: '用户权限' },
  { tab: 'settings', label: '服务器设置' },
  { tab: 'nodes', label: '节点 / Agent' },
  { tab: 'extensions', label: '扩展中心' },
  { tab: 'audit', label: '审计日志' },
]

function closeDetails(el: HTMLElement) {
  el.closest('details')?.removeAttribute('open')
}

export default function App() {
  const [instances, setInstances] = useState<Instance[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const selectedIdRef = useRef<string | null>(null)
  selectedIdRef.current = selectedId
  const [logs, setLogs] = useState<LogLine[]>([])
  const [selectedIds, setSelectedIds] = useState<string[]>([])
  const [view, setView] = useState<'manager' | 'create' | 'eula'>('manager')
  const [homeTab, setHomeTab] = useState<HomeTab>('overview')
  const [pluginFocus, setPluginFocus] = useState<string | null>(null)
  const [wasmPlugins, setWasmPlugins] = useState<
    { id: string; label: string; icon: string }[]
  >([])
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
  const [dlProgress, setDlProgress] = useState<DownloadProgress | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [health, setHealth] = useState<string>('…')
  const [envInfo, setEnvInfo] = useState<HealthInfo | null>(null)
  const [tab, setTab] = useState<Tab>('console')
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
  const [setBackupKeep, setSetBackupKeep] = useState(7)
  const [setBackupHour, setSetBackupHour] = useState<number | ''>('')
  const [setJavaMajor, setSetJavaMajor] = useState(0)
  const [propsEntries, setPropsEntries] = useState<PropertyEntry[]>([])
  const [propsEpoch, setPropsEpoch] = useState(0)
  const [authRequired, setAuthRequired] = useState(false)
  const [gate, setGate] = useState<
    'loading' | 'setup' | 'login' | 'offline' | 'app'
  >('loading')
  const [adminName, setAdminName] = useState('管理员')
  const [panelName, setPanelName] = useState('Cocktail')
  const [coreVersions, setCoreVersions] = useState<CoreVersion[]>([])
  const [installCore, setInstallCore] = useState('paper')
  const [installVer, setInstallVer] = useState('')
  const [installLoaders, setInstallLoaders] = useState<CoreLoader[]>([])
  const [installLoader, setInstallLoader] = useState('')
  const [players, setPlayers] = useState<PlayerInfo[]>([])
  const [playerHistory, setPlayerHistory] = useState<PlayerInfo[]>([])
  const [panelEvents, setPanelEvents] = useState<PanelEvent[]>([])
  const [metricHistory, setMetricHistory] = useState<MetricSample[]>([])
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
  const instanceNames = useMemo(
    () => new Map(instances.map((i) => [i.id, i.spec.name])),
    [instances],
  )

  const displayPlayers = useMemo((): PlayerInfo[] => {
    if (players.length) return players
    return (selected?.last_players ?? []).map((n) => ({
      name: n,
      online: true,
    }))
  }, [players, selected?.last_players])

  const refresh = async () => {
    const [list, summary, events] = await Promise.all([
      api.listInstances(),
      api.fleetSummary(),
      api.listEvents().catch(() => [] as PanelEvent[]),
    ])
    setInstances(list)
    setFleet(summary)
    setPanelEvents(events)
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
          id?: string
          label?: string
          phase?: string
          received?: number
          total?: number | null
          pct?: number | null
          sample?: {
            ts: string
            cpu_pct: number
            memory_mib: number
            tps?: number | null
            players: number
            net_rx_bps?: number
            net_tx_bps?: number
            net_connections?: number
            net_unique_ips?: number
            net_listen?: string | null
            net_peers?: { ip: string; connections: number }[]
          }
          line?: LogLine
        }
        // Logs already stream on logs WS — never re-list for them.
        if (msg.type === 'log') return
        if (msg.type === 'download_progress') {
          const next: DownloadProgress = {
            id: String(msg.id ?? ''),
            label: String(msg.label ?? ''),
            phase: String(msg.phase ?? 'download'),
            received: Number(msg.received ?? 0),
            total: msg.total ?? null,
            pct: msg.pct ?? null,
          }
          setDlProgress(next)
          return
        }
        if (msg.type === 'metric' && msg.instance_id && msg.sample) {
          const sample = msg.sample
          setInstances((prev) =>
            prev.map((inst) =>
              inst.id === msg.instance_id
                ? {
                    ...inst,
                    last_metrics: {
                      ...(inst.last_metrics ?? {
                        ts: sample.ts,
                        cpu_pct: sample.cpu_pct,
                        memory_mib: sample.memory_mib,
                        tps: null,
                        players: 0,
                      }),
                      ...sample,
                      tps: sample.tps ?? null,
                    },
                  }
                : inst,
            ),
          )
          if (msg.instance_id === selectedIdRef.current) {
            setMetricHistory((prev) => [...prev, sample as MetricSample].slice(-120))
          }
          return
        }
        if (msg.type === 'status_changed') {
          if (msg.instance_id && msg.status) {
            setInstances((prev) =>
              prev.map((inst) => {
                if (inst.id !== msg.instance_id) return inst
                const next = msg.status as InstanceStatus
                const cur = inst.status
                if (
                  (cur === 'stopped' ||
                    cur === 'crashed' ||
                    cur === 'created') &&
                  next === 'stopping'
                ) {
                  return inst
                }
                if (cur === 'running' && next === 'starting') return inst
                if (
                  cur === 'stopping' &&
                  (next === 'running' || next === 'starting')
                ) {
                  return inst
                }
                return { ...inst, status: next }
              }),
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
    if (gate !== 'app') return
    const apply = () => {
      const loc = parseHash()
      if (loc.kind === 'home') {
        setSelectedId(null)
        setPluginFocus(null)
        setHomeTab(loc.tab)
        setView('manager')
      } else if (loc.kind === 'plugin') {
        setSelectedId(null)
        setPluginFocus(loc.id)
        setHomeTab('extensions')
        setView('manager')
      } else if (loc.kind === 'instance') {
        setPluginFocus(null)
        setSelectedId(loc.id)
        setTab(loc.tab as Tab)
        setView('manager')
      } else if (loc.kind === 'create') {
        setView('create')
      } else if (loc.kind === 'eula') {
        setSelectedId(loc.id)
        setView('eula')
      }
    }
    apply()
    window.addEventListener('hashchange', apply)
    return () => window.removeEventListener('hashchange', apply)
  }, [gate])

  useEffect(() => {
    if (gate !== 'app') return
    api
      .listExtensions()
      .then((list) => {
        setWasmPlugins(
          (list.items ?? [])
            .filter((p) => p.ui?.nav !== false && p.enabled)
            .map((p) => ({
              id: p.id,
              label: p.ui?.label || p.name,
              icon: p.ui?.icon || 'fa-puzzle-piece',
            })),
        )
      })
      .catch(() => setWasmPlugins([]))
  }, [gate, homeTab])

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
    setSetBackupKeep(selected.spec.backup_keep ?? 7)
    setSetBackupHour(
      selected.spec.backup_hour == null ? '' : selected.spec.backup_hour,
    )
    setSetJavaMajor(selected.spec.java_major ?? 0)
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
    if (tab !== 'version' || !installVer || !coreHasLoaders(installCore)) {
      setInstallLoaders([])
      setInstallLoader('')
      return
    }
    let cancelled = false
    api
      .listCoreLoaders(installCore, installVer)
      .then((list) => {
        if (cancelled) return
        setInstallLoaders(list)
        setInstallLoader('')
      })
      .catch(() => {
        if (cancelled) return
        setInstallLoaders([])
        setInstallLoader('')
      })
    return () => {
      cancelled = true
    }
  }, [tab, installCore, installVer])

  useEffect(() => {
    if (!selectedId) return
    const core = instances.find((i) => i.id === selectedId)?.spec.core
    if (core && isInstallableCore(core)) {
      setInstallCore(core)
    }
  }, [selectedId])

  useEffect(() => {
    if (!selectedId || (tab !== 'network' && tab !== 'dashboard')) return
    api
      .listMetrics(selectedId)
      .then(setMetricHistory)
      .catch(() => setMetricHistory([]))
  }, [selectedId, tab])

  useEffect(() => {
    if (!selectedId || (tab !== 'players' && tab !== 'dashboard')) return
    api
      .listPlayers(selectedId)
      .then(setPlayers)
      .catch((e: Error) => setError(e.message))
    if (tab === 'players') {
      api
        .listPlayerHistory(selectedId)
        .then(setPlayerHistory)
        .catch((e: Error) => setError(e.message))
    }
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

  const endBusy = () => {
    setBusy(false)
    setDlProgress(null)
  }

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
    setTab('console')
    setFilePath('')
    setEditPath(null)
    setView('manager')
    setPluginFocus(null)
    setError(null)
    writeHash({ kind: 'instance', id, tab: 'console' })
  }

  const goTab = (next: Tab) => {
    setTab(next)
    if (selectedId) {
      writeHash({ kind: 'instance', id: selectedId, tab: next })
    }
  }

  const goHome = (tab: HomeTab = 'overview') => {
    setSelectedId(null)
    setHomeTab(tab)
    setView('manager')
    setPluginFocus(null)
    setError(null)
    writeHash({ kind: 'home', tab })
  }

  const openPlugin = (id: string) => {
    setSelectedId(null)
    setHomeTab('extensions')
    setPluginFocus(id)
    setView('manager')
    writeHash({ kind: 'plugin', id })
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
  const cpuVal = selected?.last_metrics?.cpu_pct
  const playersCount = selected?.last_metrics?.players
  const playersMax = selected?.last_metrics?.players_max
  const netRx = selected?.last_metrics?.net_rx_bps
  const statusOk = selected?.status === 'running'
  const meterKind =
    selected?.status === 'crashed'
      ? 'is-spill'
      : statusOk
        ? 'is-live'
        : 'is-empty'
  const memPct =
    memUsed != null && memTotal
      ? Math.max(0, Math.min(100, (memUsed / memTotal) * 100))
      : 0
  const cpuBar = Math.max(0, Math.min(100, cpuVal ?? 0))
  const netBar =
    netRx != null
      ? Math.max(6, Math.min(100, (netRx / (2 * 1024 * 1024)) * 100))
      : 0
  const moreTabOpen = MORE_TABS.some((item) => item.id === tab)

  if (gate !== 'app') {
    return (
      <>
        <BusyOverlay
          active={busy || gate === 'loading'}
          label={gate === 'loading' ? '正在连接控制面…' : busyLabel}
          progress={dlProgress}
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
                <div className="brand-mark splash" aria-hidden>
                  <img src="/logo.png" alt="" className="brand-logo-img" />
                </div>
                <div>
                  <h1>Cocktail</h1>
                  <span className="brand-sub">正在连接控制面…</span>
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
        progress={dlProgress}
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
        >
          <div className="brand-mark" aria-hidden>
            <img src="/logo.png" alt="" className="brand-logo-img" />
          </div>
          <div>
            <h1>{panelName === 'Cocktail Manager' ? 'Cocktail' : panelName}</h1>
            <span className="brand-sub">Manager · 26Q3</span>
          </div>
        </div>
        <span className="channel-chip">
          {envInfo?.release || envInfo?.version || 'v0.1'}
        </span>
        <span
          className={`plane-live${health === 'offline' ? ' offline' : ''}`}
          title={health}
        >
          <span className="dot" aria-hidden />
          {health === 'offline' ? '控制面离线' : '控制面在线'}
        </span>
        <span className="plane-meta">
          {envInfo?.version ? `v${envInfo.version.replace(/^v/, '')}` : 'v0.1'}
          {' · '}
          {authRequired ? '鉴权已启用' : '鉴权关闭'}
          {' · '}
          Docker {fleet?.docker.available ? '就绪' : '不可用'}
        </span>
        <span className="topbar-grow" />
        <div className="topbar-right">
          <details className="plane-menu">
            <summary>设置</summary>
            <div className="plane-menu-list">
              {PLANE_LINKS.map((link) => (
                <button
                  key={link.tab}
                  type="button"
                  className={
                    !selected &&
                    homeTab === link.tab &&
                    (link.tab !== 'extensions' || !pluginFocus)
                      ? 'is-current'
                      : undefined
                  }
                  onClick={(e) => {
                    goHome(link.tab)
                    closeDetails(e.currentTarget)
                  }}
                >
                  {link.label}
                </button>
              ))}
              {wasmPlugins.map((p) => (
                <button
                  key={p.id}
                  type="button"
                  className={
                    !selected && pluginFocus === p.id ? 'is-current' : undefined
                  }
                  onClick={(e) => {
                    openPlugin(p.id)
                    closeDetails(e.currentTarget)
                  }}
                >
                  {p.label}
                </button>
              ))}
            </div>
          </details>
          <span>{adminName}</span>
          <button
            type="button"
            onClick={() => {
              api.logout().finally(() => {
                setToken('')
                setGate('login')
              })
            }}
          >
            退出
          </button>
        </div>
      </header>

      <div className="app-body">
        <InstanceRail
          instances={instances}
          selectedId={selectedId}
          fleet={fleet}
          events={panelEvents}
          names={instanceNames}
          onSelect={selectInstance}
          onCreate={() => {
            setError(null)
            setView('create')
            writeHash({ kind: 'create' })
          }}
        />

        <div
          className={
            view === 'manager' && selected
              ? 'workspace has-instance'
              : 'workspace'
          }
        >
          {view === 'manager' && selected && (
            <div className="ws-head">
              <div className="who">
                <div>
                  <h1>
                    {selected.spec.name}
                    {selected.reattached && selected.status === 'running' ? (
                      <span className="takeover">已接管</span>
                    ) : null}
                  </h1>
                  <p className="who-sub">
                    {selected.spec.core}
                    {selected.spec.mc_version
                      ? ` ${selected.spec.mc_version}`
                      : ''}
                    {' · '}
                    <span className="mono">:{selected.spec.port}</span>
                    {' · '}
                    {selected.spec.runtime === 'docker' ? 'docker' : 'process'}
                    {selected.pid ? ` · pid ${selected.pid}` : ''}
                    {` · 节点 ${selected.node_id ?? selected.spec.node_id ?? 'local'}`}
                    {` · ${STATUS_LABEL[selected.status]}`}
                  </p>
                </div>
                <div className="meters">
                  <div className={`meter ${meterKind}`}>
                    <div className="meter-lab">
                      CPU{' '}
                      <b>
                        {statusOk && cpuVal != null
                          ? `${cpuVal.toFixed(1)}%`
                          : '—'}
                      </b>
                    </div>
                    <div className="meter-bar">
                      <span
                        style={
                          meterKind === 'is-spill'
                            ? undefined
                            : { width: `${statusOk ? cpuBar : 0}%` }
                        }
                      />
                    </div>
                  </div>
                  <div className={`meter ${meterKind}`}>
                    <div className="meter-lab">
                      内存{' '}
                      <b>
                        {statusOk && memUsed != null
                          ? `${Math.round(memPct)}%`
                          : '—'}
                      </b>
                    </div>
                    <div className="meter-bar">
                      <span
                        style={
                          meterKind === 'is-spill'
                            ? undefined
                            : { width: `${statusOk ? memPct : 0}%` }
                        }
                      />
                    </div>
                  </div>
                  <div className={`meter ${meterKind}`}>
                    <div className="meter-lab">
                      网络 <b>{statusOk ? formatBps(netRx) : '—'}</b>
                    </div>
                    <div className="meter-bar">
                      <span
                        style={
                          meterKind === 'is-spill'
                            ? undefined
                            : { width: `${statusOk ? netBar : 0}%` }
                        }
                      />
                    </div>
                  </div>
                </div>
              </div>
              <ul className="facts">
                <li>
                  <span className="k">TPS</span>
                  <span
                    className={
                      statusOk && tpsVal != null ? 'v live' : 'v dead'
                    }
                  >
                    {statusOk && tpsVal != null ? tpsVal.toFixed(2) : '—'}
                  </span>
                </li>
                <li>
                  <span className="k">玩家</span>
                  <span className={statusOk ? 'v' : 'v dead'}>
                    {statusOk
                      ? `${playersCount ?? 0}${playersMax != null ? `/${playersMax}` : ''}`
                      : '—'}
                  </span>
                </li>
                <li>
                  <span className="k">内存</span>
                  <span
                    className={
                      statusOk && memUsed != null ? 'v' : 'v dead'
                    }
                  >
                    {statusOk && memUsed != null
                      ? `${Math.round(memUsed)}/${memTotal ?? '—'}`
                      : `—/${memTotal ?? '—'}`}
                  </span>
                </li>
                {selected.reattached && selected.status === 'running' ? (
                  <li>
                    <span className="k">热接管</span>
                    <span className="v live">已接管</span>
                  </li>
                ) : (
                  <li>
                    <span className="k">健康</span>
                    <span
                      className={
                        selected.status === 'crashed'
                          ? 'v bad'
                          : selected.health_score != null && statusOk
                            ? 'v live'
                            : 'v dead'
                      }
                    >
                      {selected.health_score != null
                        ? `${selected.health_score}%`
                        : '—'}
                    </span>
                  </li>
                )}
              </ul>
              <nav className="subnav" aria-label="实例操作">
                <div className="subnav-groups">
                  {PRIMARY_TABS.map((item) => (
                    <button
                      key={item.id}
                      type="button"
                      className={
                        tab === item.id ? 'subnav-item active' : 'subnav-item'
                      }
                      onClick={() => goTab(item.id)}
                    >
                      {item.label}
                    </button>
                  ))}
                  <details
                    className={
                      moreTabOpen ? 'subnav-more is-current' : 'subnav-more'
                    }
                  >
                    <summary>更多</summary>
                    <div className="subnav-more-list">
                      {MORE_TABS.map((item) => (
                        <button
                          key={item.id}
                          type="button"
                          className={tab === item.id ? 'is-current' : undefined}
                          onClick={(e) => {
                            goTab(item.id)
                            closeDetails(e.currentTarget)
                          }}
                        >
                          {item.label}
                        </button>
                      ))}
                    </div>
                  </details>
                </div>
              </nav>
            </div>
          )}

        <main
          className={
            view === 'manager' && selected && tab === 'console'
              ? 'main is-console'
              : 'main'
          }
        >
          <div
            className="page-stage"
            key={
              view !== 'manager'
                ? view
                : selected
                  ? `${selected.id}:${tab}`
                  : homeTab
            }
          >
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
                writeHash({ kind: 'instance', id: inst.id, tab: next })
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
                writeHash({ kind: 'instance', id: inst.id, tab: 'control' })
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
                  events={panelEvents}
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
              ) : homeTab === 'events' ? (
                <div className="page-flow">
                  <div className="page-head">
                    <div>
                      <p className="page-eyebrow">控制面</p>
                      <h2 className="page-title">事件中心</h2>
                    </div>
                    <button
                      type="button"
                      className="btn btn-ghost"
                      onClick={() => goHome('overview')}
                    >
                      返回主界面
                    </button>
                  </div>
                  <div className="card-panel">
                    <EventFeed
                      events={panelEvents}
                      names={
                        new Map(instances.map((i) => [i.id, i.spec.name]))
                      }
                      onOpenInstance={selectInstance}
                    />
                  </div>
                </div>
              ) : homeTab === 'users' ? (
                <UsersPage
                  onBack={() => goHome('overview')}
                  onError={setError}
                />
              ) : homeTab === 'network' ? (
                <GlobalNetworkPage
                  onBack={() => goHome('overview')}
                  onOpenSettings={() => goHome('settings')}
                  onOpenInstance={selectInstance}
                  onError={setError}
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
                  focusId={pluginFocus}
                  onOpenPlugin={openPlugin}
                />
              ) : homeTab === 'audit' ? (
                <AuditPage
                  instances={instances}
                  onBack={() => goHome('overview')}
                  onOpenInstance={selectInstance}
                  onError={setError}
                />
              ) : homeTab === 'settings' ? (
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
                  busy={busy}
                  onBusy={setBusyState}
                  onError={setError}
                />
              ) : null}
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
                <DashPane
                  selected={selected}
                  events={panelEvents.filter(
                    (e) => !e.instance_id || e.instance_id === selected.id,
                  )}
                  onOpenNetwork={() => goTab('network')}
                />
              )}

              {tab === 'control' && (
                <>
                  <div className="power-row">
                    <button
                      type="button"
                      className="power primary"
                      disabled={
                        busy ||
                        selected.status === 'running' ||
                        selected.status === 'starting'
                      }
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
                      启动
                    </button>
                    <button
                      type="button"
                      className="power"
                      disabled={
                        busy ||
                        selected.status === 'stopped' ||
                        selected.status === 'created' ||
                        selected.status === 'crashed'
                      }
                      onClick={() =>
                        run(() => api.stopInstance(selected.id), '停止服务器…')
                      }
                    >
                      停止
                    </button>
                    <button
                      type="button"
                      className="power"
                      disabled={busy}
                      onClick={() =>
                        run(() => api.restartInstance(selected.id), '重启服务器…')
                      }
                    >
                      重启
                    </button>
                    {!selected.spec.eula_accepted &&
                      selected.spec.core !== 'demo' && (
                        <button
                          type="button"
                          className="power"
                          disabled={busy}
                          onClick={() => setView('eula')}
                        >
                          同意 EULA
                        </button>
                      )}
                    <button
                      type="button"
                      className="link-btn"
                      disabled={busy}
                      onClick={() =>
                        run(async () => {
                          await api.deleteInstance(selected.id)
                          setSelectedId(null)
                        })
                      }
                    >
                      删除实例
                    </button>
                  </div>
                  {!selected.spec.command && selected.spec.core !== 'demo' && (
                    <p className="error" style={{ marginBottom: '0.75rem' }}>
                      尚未配置启动命令。请到「更多 → 版本 / 导入」上传压缩包、jar 或下载核心。
                    </p>
                  )}
                  <dl className="ctl-dl">
                    <dt>状态</dt>
                    <dd>
                      {STATUS_LABEL[selected.status]}
                      {selected.reattached && selected.status === 'running'
                        ? ' · 已接管'
                        : ''}
                    </dd>
                    <dt>pid</dt>
                    <dd className="mono">{selected.pid ?? '—'}</dd>
                    <dt>运行时</dt>
                    <dd>
                      {selected.spec.runtime === 'docker' ? 'docker' : 'process'}
                      {' · '}
                      {selected.spec.core}
                      {selected.spec.mc_version
                        ? ` ${selected.spec.mc_version}`
                        : ''}
                    </dd>
                    <dt>内存</dt>
                    <dd>{selected.spec.memory_mib} MiB</dd>
                    <dt>启动</dt>
                    <dd>
                      <code>{effectiveCmd(selected)}</code>
                    </dd>
                    <dt>热接管</dt>
                    <dd>
                      {selected.reattached && selected.status === 'running'
                        ? '控制面重启后仍是这杯酒，只换了调酒师'
                        : '—'}
                    </dd>
                  </dl>
                </>
              )}

              {tab === 'console' && (
                <div
                  className={
                    selected.status === 'crashed'
                      ? 'console-well is-spill'
                      : selected.status === 'running' ||
                          selected.status === 'starting'
                        ? 'console-well is-live'
                        : 'console-well is-empty'
                  }
                >
                  <pre className="console-box tall">
                    {logs.length === 0
                      ? selected.status === 'crashed'
                        ? '酒洒了。井是空的。'
                        : selected.status === 'running' ||
                            selected.status === 'starting'
                          ? '等待日志…（启动后出现；支持历史缓冲）'
                          : `封口 · ${STATUS_LABEL[selected.status]}\n没有 STDIN。空杯。`
                      : logs.map((l) => l.line).join('\n')}
                  </pre>
                  <form className="cmd-row" onSubmit={sendCmd}>
                    <input
                      value={cmd}
                      onChange={(e) => setCmd(e.target.value)}
                      placeholder={
                        selected.status === 'running'
                          ? '输入命令，如 list / say hello'
                          : selected.status === 'crashed'
                            ? '已崩溃 · 无 STDIN'
                            : '实例已停止'
                      }
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
              )}

              {tab === 'version' && (
                <>
                  <div className="page-head">
                    <div>
                      <p className="page-eyebrow">{selected.spec.name}</p>
                      <h2 className="page-title">版本 / 自定义导入</h2>
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
                        <i className="fa fa-file-archive-o" /> 导入压缩包（7z / zip）
                        <input
                          type="file"
                          accept=".7z,.zip,.tar,.tar.gz,.tgz,.gz,.xz,.bz2,.jar"
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
                              const result = await api.importArchive(
                                selected.id,
                                file,
                                {
                                  core: 'custom',
                                  accept_eula: selected.spec.eula_accepted,
                                },
                              )
                              if (result.startup === 'none') {
                                setError(
                                  '已解压，但未检测到启动命令。请到设置里填写自定义启动命令。',
                                )
                              }
                            }, `解压导入：${file.name}`)
                            e.target.value = ''
                          }}
                        />
                      </label>
                      <p className="meta">
                        内置 7-Zip 解压整包到此实例目录（支持 7z / zip / tar.gz /
                        tar.xz）。解压后自动识别 server.jar、Forge 参数或
                        run.bat / start.sh，核心设为 custom。
                      </p>
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
                        导入 jar 后自动配置：java -jar server.jar nogui，并按设置注入
                        -Xmx/-Xms；容器运行时映射 主机端口→容器 25565。启动命令也可在「设置」中随时改。
                      </p>
                      <hr style={{ margin: '1.25rem 0', borderColor: '#e5eaf0' }} />
                      <label>
                        在线核心
                        <select
                          value={installCore}
                          onChange={(e) => setInstallCore(e.target.value)}
                        >
                          {INSTALLABLE_CORE_GROUPS.map((g) => (
                            <optgroup key={g.label} label={g.label}>
                              {g.items.map((item) => (
                                <option key={item.id} value={item.id}>
                                  {item.label}
                                </option>
                              ))}
                            </optgroup>
                          ))}
                        </select>
                      </label>
                      <label>
                        游戏版本
                        <select
                          value={installVer}
                          onChange={(e) => setInstallVer(e.target.value)}
                        >
                          {coreVersions.map((v) => (
                            <option key={v.id} value={v.id}>
                              {v.label ?? v.id}
                              {v.latest ? ' (latest)' : ''}
                            </option>
                          ))}
                        </select>
                      </label>
                      {coreHasLoaders(installCore) && (
                        <label>
                          {installCore === 'arclight'
                            ? '混合变体（可选）'
                            : '加载器版本（可选）'}
                          <select
                            value={installLoader}
                            onChange={(e) => setInstallLoader(e.target.value)}
                          >
                            <option value="">
                              {installCore === 'arclight'
                                ? '优先 NeoForge，没有则回退'
                                : '最新稳定（默认）'}
                            </option>
                            {installLoaders.map((l) => (
                              <option key={l.id} value={l.id}>
                                {l.label ? `${l.id}（${l.label}）` : l.id}
                                {l.latest ? ' · latest' : ''}
                                {l.recommended && !l.label
                                  ? ' · recommended'
                                  : ''}
                              </option>
                            ))}
                          </select>
                        </label>
                      )}
                      <p className="meta">
                        {installCore === 'forge' ||
                        installCore === 'neoforge' ||
                        installCore === 'quilt'
                          ? '将下载安装器并用托管的 Adoptium Temurin 执行（本机无 Java 时自动补全）。安装后自动写入启动参数（Windows 为 @win_args.txt）。'
                          : installCore === 'arclight'
                            ? '可指定 NeoForge / Forge / Fabric 变体；未选则优先 NeoForge。'
                            : '下载官方构建为 server.jar，并自动配置 java -jar server.jar nogui。'}
                      </p>
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
                                installLoader || undefined,
                              ),
                            `下载安装 ${installCore} ${installVer}${installLoader ? ` / ${installLoader}` : ''}…`,
                          )
                        }
                      >
                        <i className="fa fa-download" /> 下载并安装（自动配置启动命令）
                      </button>
                    </div>
                  </div>
                </>
              )}

              {tab === 'network' && (
                <NetworkPage
                  instance={selected}
                  history={metricHistory}
                  running={statusOk}
                  onBusy={setBusyState}
                  onError={setError}
                />
              )}

              {tab === 'players' && (
                <>
                  {selected.status === 'crashed' && displayPlayers.length === 0 ? (
                    <div className="spilled">
                      <h2>没有会话</h2>
                      <p>进程已经退出。踢 / op 没有对象。</p>
                    </div>
                  ) : selected.status !== 'running' &&
                    selected.status !== 'starting' &&
                    displayPlayers.length === 0 ? (
                    <div className="empty-cup">
                      <h2>没有人</h2>
                      <p>
                        {STATUS_LABEL[selected.status]}。名单是空的，不是藏起来了。
                      </p>
                    </div>
                  ) : (
                    <>
                      <p className="net-lead">
                        在线{' '}
                        <strong>
                          {playersCount ?? displayPlayers.length}
                          {playersMax != null ? ` / ${playersMax}` : ''}
                        </strong>
                        {' · '}
                        <button
                          type="button"
                          className="link-btn"
                          disabled={busy || selected.status !== 'running'}
                          onClick={() =>
                            run(async () => {
                              setPlayers(
                                await api.listPlayers(selected.id, {
                                  probe: true,
                                }),
                              )
                              setPlayerHistory(
                                await api.listPlayerHistory(selected.id),
                              )
                            })
                          }
                        >
                          刷新 list
                        </button>
                      </p>
                      {displayPlayers.length === 0 ? (
                        <p className="empty">暂无在线玩家（启动后点刷新）</p>
                      ) : (
                        <table className="player-table">
                          <thead>
                            <tr>
                              <th>玩家</th>
                              <th>来源</th>
                              <th>在线</th>
                              <th />
                            </tr>
                          </thead>
                          <tbody>
                            {displayPlayers.map((p) => (
                              <tr key={p.name}>
                                <td>
                                  {p.name}
                                  <div className="meta">{p.uuid ?? ''}</div>
                                </td>
                                <td className="mono">
                                  {p.world ?? 'world'}
                                  {p.ping_ms != null
                                    ? ` · ${p.ping_ms.toFixed(0)}ms`
                                    : ''}
                                </td>
                                <td>{formatDuration(p.session_secs)}</td>
                                <td>
                                  {(
                                    [
                                      ['kick', '踢出'],
                                      ['op', 'op'],
                                      ['deop', '撤 op'],
                                      ['ban', '封禁'],
                                    ] as const
                                  ).map(([a, label]) => (
                                    <button
                                      key={a}
                                      type="button"
                                      className="link-btn"
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
                                      {label}
                                    </button>
                                  ))}
                                </td>
                              </tr>
                            ))}
                          </tbody>
                        </table>
                      )}
                      {playerHistory.length > 0 && (
                        <div style={{ marginTop: '1.25rem' }}>
                          <h3 className="card-title">历史</h3>
                          <table className="player-table">
                            <thead>
                              <tr>
                                <th>玩家</th>
                                <th>UUID</th>
                                <th>首次加入</th>
                                <th>最后在线</th>
                                <th>累计</th>
                              </tr>
                            </thead>
                            <tbody>
                              {playerHistory.map((p) => (
                                <tr key={p.name}>
                                  <td>{p.name}</td>
                                  <td className="mono">{p.uuid ?? '—'}</td>
                                  <td className="meta">
                                    {p.first_seen
                                      ? new Date(p.first_seen).toLocaleString(
                                          'zh-CN',
                                          { hour12: false },
                                        )
                                      : '—'}
                                  </td>
                                  <td className="meta">
                                    {p.last_seen
                                      ? new Date(p.last_seen).toLocaleString(
                                          'zh-CN',
                                          { hour12: false },
                                        )
                                      : '—'}
                                  </td>
                                  <td>
                                    {formatDuration(p.total_secs)}
                                  </td>
                                </tr>
                              ))}
                            </tbody>
                          </table>
                        </div>
                      )}
                    </>
                  )}
                </>
              )}

              {tab === 'automations' && (
                <AutomationsPage
                  instance={selected}
                  busy={busy}
                  onBusy={setBusyState}
                  onError={setError}
                />
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
                      defaultLoader={defaultModrinthLoader(selected.spec.core)}
                      defaultProjectType={defaultModrinthProjectType(
                        selected.spec.core,
                      )}
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
                      <h2 className="page-title">备份策略</h2>
                    </div>
                  </div>
                  <div className="card-panel">
                    <p className="meta mb-1">
                      整包工作目录备份（世界 + 配置 + 插件）。到点自动备份，并只保留最近 N
                      份。对象存储上传尚未接入。
                    </p>
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
                            backup_keep: setBackupKeep,
                            backup_hour:
                              setBackupHour === '' ? null : Number(setBackupHour),
                            java_major: setJavaMajor,
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
                        Java 版本
                        <select
                          value={setJavaMajor}
                          onChange={(e) => {
                            const v = Number(e.target.value)
                            setSetJavaMajor(v)
                            const major =
                              v ||
                              recommendedJavaMajor(
                                selected.spec.mc_version || undefined,
                              )
                            if (
                              setRuntime === 'docker' &&
                              (!setImage.trim() ||
                                setImage.startsWith('eclipse-temurin:'))
                            ) {
                              setSetImage(`eclipse-temurin:${major}-jre`)
                            }
                          }}
                        >
                          <option value={0}>
                            自动（推荐{' '}
                            {recommendedJavaMajor(
                              selected.spec.mc_version || undefined,
                            )}
                            {selected.spec.mc_version
                              ? ` · ${selected.spec.mc_version}`
                              : ''}
                            ）
                          </option>
                          {JAVA_MAJORS.map((m) => (
                            <option key={m} value={m}>
                              Java {m}
                            </option>
                          ))}
                        </select>
                      </label>
                      <p className="meta">
                        命令填写 java 时，启动会把 Temurin 复制到本杯子的{' '}
                        <code>runtime/jre</code>
                        ，不用系统 Java。
                      </p>
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
                        每日备份时刻（0–23，空则关闭）
                        <input
                          type="number"
                          min={0}
                          max={23}
                          value={setBackupHour}
                          onChange={(e) =>
                            setSetBackupHour(
                              e.target.value === ''
                                ? ''
                                : Number(e.target.value),
                            )
                          }
                          placeholder="例如 3"
                        />
                      </label>
                      <label>
                        保留份数
                        <input
                          type="number"
                          min={1}
                          max={90}
                          value={setBackupKeep}
                          onChange={(e) =>
                            setSetBackupKeep(Number(e.target.value))
                          }
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
                      <p className="meta">
                        工作目录：{selected.spec.workdir}
                        {' · '}独占文件根
                        {selected.spec.runtime !== 'docker'
                          ? ' · 独立 JRE runtime/jre'
                          : ''}
                      </p>
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
          </div>
        </main>
        </div>
      </div>
    </div>
  )
}
