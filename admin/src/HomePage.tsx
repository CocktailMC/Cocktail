import type { HealthInfo, Instance, InstanceStatus } from './api'
import { formatBps } from './api'
import { BrandImg, BRAND } from './brandIcons'
import EnvBrandBar from './EnvBrandBar'

const STATUS_LABEL: Record<InstanceStatus, string> = {
  created: '已创建',
  starting: '启动中',
  running: '运行中',
  stopping: '停止中',
  stopped: '已停止',
  crashed: '崩溃',
}

type Fleet = {
  total: number
  running: number
  stopped: number
  starting: number
  crashed: number
  docker: { available: boolean; message: string }
}

type Props = {
  instances: Instance[]
  fleet: Fleet | null
  health: string
  env: HealthInfo | null
  authRequired: boolean
  busy: boolean
  selectedIds: string[]
  onToggleSelect: (id: string, checked: boolean) => void
  onSelectAll: (checked: boolean) => void
  onOpenInstance: (id: string) => void
  onCreate: () => void
  onStart: (id: string) => void
  onStop: (id: string) => void
  onRestart: (id: string) => void
  onBulk: (action: 'start' | 'stop' | 'restart' | 'delete') => void
  onOpenSettings: () => void
}

export default function HomePage(props: Props) {
  const {
    instances,
    fleet,
    health,
    env,
    authRequired,
    busy,
    selectedIds,
    onToggleSelect,
    onSelectAll,
    onOpenInstance,
    onCreate,
    onStart,
    onStop,
    onRestart,
    onBulk,
    onOpenSettings,
  } = props
  const allSelected =
    instances.length > 0 && selectedIds.length === instances.length

  return (
    <div className="home-page">
      <div className="page-head">
        <div>
          <p className="page-eyebrow">Cocktail Manager</p>
          <h2 className="page-title">主界面</h2>
        </div>
        <div className="btn-row">
          <button type="button" className="btn btn-ghost" onClick={onOpenSettings}>
            <i className="fa fa-cog" /> 服务器设置
          </button>
          <button type="button" className="btn btn-primary" onClick={onCreate}>
            <i className="fa fa-plus" /> 创建实例
          </button>
        </div>
      </div>

      <EnvBrandBar env={env} offline={health === 'offline'} />

      <div className="stat-grid home-stats">
        <div className="card-panel stat-card">
          <div>
            <p className="label">实例总数</p>
            <p className="value">{fleet?.total ?? instances.length}</p>
          </div>
          <i className="fa fa-server icon primary" />
        </div>
        <div className="card-panel stat-card">
          <div>
            <p className="label">运行中</p>
            <p className="value success">{fleet?.running ?? 0}</p>
          </div>
          <i className="fa fa-play-circle icon success" />
        </div>
        <div className="card-panel stat-card">
          <div>
            <p className="label">已停止</p>
            <p className="value">{fleet?.stopped ?? 0}</p>
          </div>
          <i className="fa fa-pause-circle icon warning" />
        </div>
        <div className="card-panel stat-card">
          <div>
            <p className="label">异常 / 过渡</p>
            <p className="value">
              {(fleet?.crashed ?? 0) + (fleet?.starting ?? 0)}
            </p>
          </div>
          <i
            className={`fa fa-exclamation-triangle icon${
              (fleet?.crashed ?? 0) > 0 ? ' warning' : ' primary'
            }`}
          />
        </div>
      </div>

      <div className="grid-2 home-meta-grid">
        <div className="card-panel">
          <h3 className="card-title">
            <i className="fa fa-heartbeat" /> 控制面
          </h3>
          <table className="info-table">
            <tbody>
              <tr>
                <td>状态</td>
                <td>{health === 'offline' ? '离线' : '在线'}</td>
              </tr>
              <tr>
                <td>版本信息</td>
                <td>{health}</td>
              </tr>
              <tr>
                <td>API 鉴权</td>
                <td>{authRequired ? '已启用' : '未启用'}</td>
              </tr>
            </tbody>
          </table>
        </div>
        <div className="card-panel">
          <h3 className="card-title">
            <BrandImg src={BRAND.docker} alt="" height={16} /> Docker
          </h3>
          <table className="info-table">
            <tbody>
              <tr>
                <td>可用性</td>
                <td>
                  {fleet?.docker.available ? (
                    <span className="ok-text">就绪</span>
                  ) : (
                    <span className="bad-text">不可用</span>
                  )}
                </td>
              </tr>
              <tr>
                <td>说明</td>
                <td>{fleet?.docker.message || '—'}</td>
              </tr>
            </tbody>
          </table>
          <p className="meta" style={{ marginTop: '0.75rem' }}>
            容器运行时实例依赖 Docker；进程模式无需 Docker。
          </p>
        </div>
      </div>

      <div className="card-panel home-sources mt-6">
        <h3 className="card-title">插件源</h3>
        <div className="home-source-row">
          <span className="home-source-chip">
            <BrandImg src={BRAND.modrinth} alt="" /> Modrinth
          </span>
          <span className="home-source-chip">
            <i className="fa fa-paper-plane" /> Hangar
          </span>
          <span className="home-source-chip">
            <BrandImg src={BRAND.spigotmc} alt="" /> Spiget / SpigotMC
          </span>
        </div>
      </div>

      <div className="card-panel home-servers mt-6">
        <div className="home-servers-head">
          <h3 className="card-title" style={{ margin: 0 }}>
            <i className="fa fa-list" /> 服务器列表
          </h3>
          <div className="home-servers-tools">
            <label className="home-check">
              <input
                type="checkbox"
                checked={allSelected}
                onChange={(e) => onSelectAll(e.target.checked)}
                disabled={!instances.length}
              />
              全选
            </label>
            <button
              type="button"
              className="btn btn-ghost"
              disabled={!selectedIds.length || busy}
              onClick={() => onBulk('start')}
            >
              批量启动
            </button>
            <button
              type="button"
              className="btn btn-ghost"
              disabled={!selectedIds.length || busy}
              onClick={() => onBulk('stop')}
            >
              批量停止
            </button>
            <button
              type="button"
              className="btn btn-ghost"
              disabled={!selectedIds.length || busy}
              onClick={() => onBulk('restart')}
            >
              批量重启
            </button>
            <button
              type="button"
              className="btn btn-danger"
              disabled={!selectedIds.length || busy}
              onClick={() => onBulk('delete')}
            >
              批量删除
            </button>
          </div>
        </div>

        {instances.length === 0 ? (
          <div className="store-empty" style={{ marginTop: '0.75rem' }}>
            <i className="fa fa-server" />
            <span>还没有服务器实例</span>
            <button type="button" className="btn btn-primary" onClick={onCreate}>
              <i className="fa fa-plus" /> 创建第一个实例
            </button>
          </div>
        ) : (
          <ul className="server-cards">
            {instances.map((inst) => {
              const running = inst.status === 'running'
              const mem = inst.last_metrics?.memory_mib
              const players = inst.last_metrics?.players
              const tps = inst.last_metrics?.tps
              return (
                <li key={inst.id} className="server-card">
                  <div className="server-card-top">
                    <label
                      className="home-check"
                      onClick={(e) => e.stopPropagation()}
                    >
                      <input
                        type="checkbox"
                        checked={selectedIds.includes(inst.id)}
                        onChange={(e) =>
                          onToggleSelect(inst.id, e.target.checked)
                        }
                      />
                    </label>
                    <button
                      type="button"
                      className="server-card-main"
                      onClick={() => onOpenInstance(inst.id)}
                    >
                      <div className="server-card-title">
                        <strong>{inst.spec.name}</strong>
                        <span className={`badge status-${inst.status}`}>
                          {STATUS_LABEL[inst.status]}
                        </span>
                      </div>
                      <span className="server-card-meta">
                        {inst.spec.core}
                        <span className="dot" />:{inst.spec.port}
                        <span className="dot" />
                        {inst.spec.runtime}
                        <span className="dot" />
                        {inst.spec.memory_mib} MiB
                        <span className="dot" />
                        {inst.node_id ?? inst.spec.node_id ?? 'local'}
                      </span>
                    </button>
                  </div>
                  <div className="server-card-stats">
                    <span>
                      <i className="fa fa-users" /> {players ?? '—'}
                    </span>
                    <span>
                      <i className="fa fa-microchip" />{' '}
                      {mem != null ? `${mem} MiB` : '—'}
                    </span>
                    <span>
                      <i className="fa fa-tachometer" />{' '}
                      {tps != null ? tps.toFixed(1) : '—'}
                    </span>
                    <span>
                      <i className="fa fa-exchange" />{' '}
                      {running
                        ? `${formatBps(inst.last_metrics?.net_rx_bps)} ↓`
                        : '—'}
                    </span>
                  </div>
                  <div className="server-card-actions">
                    <button
                      type="button"
                      className="btn btn-primary"
                      disabled={busy || running}
                      onClick={() => onStart(inst.id)}
                    >
                      <i className="fa fa-play" /> 启动
                    </button>
                    <button
                      type="button"
                      className="btn btn-ghost"
                      disabled={busy || !running}
                      onClick={() => onStop(inst.id)}
                    >
                      <i className="fa fa-stop" /> 停止
                    </button>
                    <button
                      type="button"
                      className="btn btn-ghost"
                      disabled={busy}
                      onClick={() => onRestart(inst.id)}
                    >
                      <i className="fa fa-refresh" /> 重启
                    </button>
                    <button
                      type="button"
                      className="btn btn-ghost"
                      onClick={() => onOpenInstance(inst.id)}
                    >
                      进入 <i className="fa fa-arrow-right" />
                    </button>
                  </div>
                </li>
              )
            })}
          </ul>
        )}
      </div>
    </div>
  )
}
