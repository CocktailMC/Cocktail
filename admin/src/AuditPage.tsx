import { useEffect, useMemo, useState } from 'react'
import { api, type AuditEntry, type Instance } from './api'

const ACTION_LABEL: Record<string, string> = {
  'auth.setup': '初始化管理员',
  'auth.login': '登录',
  'auth.password': '修改密码',
  'settings.update': '更新面板设置',
  'instance.create': '创建实例',
  'instance.update': '更新实例',
  'instance.apply': '应用实例 spec',
  'node.create': '创建节点',
  'node.delete': '删除节点',
  'instance.start': '启动实例',
  'instance.stop': '停止实例',
  'instance.delete': '删除实例',
  'instance.command': '发送指令',
  'file.write': '写入文件',
  'file.delete': '删除文件',
  'file.mkdir': '新建目录',
  'backup.create': '创建备份',
  'backup.delete': '删除备份',
  'backup.restore': '恢复备份',
  'world.reset': '重置世界',
  'world.export': '导出世界',
  'world.import': '导入世界',
  'player.action': '玩家操作',
  'automation.fire': '自动化触发',
  'user.create': '创建用户',
  'user.delete': '删除用户',
}

const ACTION_FILTERS = [
  { value: '', label: '全部操作' },
  { value: 'auth', label: '登录 / 账号' },
  { value: 'settings', label: '面板设置' },
  { value: 'instance', label: '实例' },
  { value: 'file', label: '文件' },
  { value: 'backup', label: '备份' },
  { value: 'world', label: '世界' },
]

function actionLabel(action: string) {
  return ACTION_LABEL[action] ?? action
}

function formatTime(iso: string) {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return d.toLocaleString('zh-CN', { hour12: false })
}

function detailText(detail: unknown) {
  if (detail == null || (typeof detail === 'object' && Object.keys(detail as object).length === 0)) {
    return '—'
  }
  if (typeof detail === 'string') return detail
  return JSON.stringify(detail)
}

type Props = {
  instances: Instance[]
  onBack: () => void
  onOpenInstance: (id: string) => void
  onError: (msg: string | null) => void
}

export default function AuditPage({
  instances,
  onBack,
  onOpenInstance,
  onError,
}: Props) {
  const [items, setItems] = useState<AuditEntry[]>([])
  const [total, setTotal] = useState(0)
  const [limit] = useState(80)
  const [offset, setOffset] = useState(0)
  const [action, setAction] = useState('')
  const [instanceId, setInstanceId] = useState('')
  const [q, setQ] = useState('')
  const [qDraft, setQDraft] = useState('')
  const [loading, setLoading] = useState(false)

  const names = useMemo(() => {
    const m = new Map<string, string>()
    for (const i of instances) m.set(i.id, i.spec.name)
    return m
  }, [instances])

  const load = async () => {
    setLoading(true)
    onError(null)
    try {
      const res = await api.listAudit({
        limit,
        offset,
        action: action || undefined,
        instance_id: instanceId || undefined,
        q: q || undefined,
      })
      setItems(res.items)
      setTotal(res.total)
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void load()
  }, [offset, action, instanceId, q])

  const page = Math.floor(offset / limit) + 1
  const pages = Math.max(1, Math.ceil(total / limit))

  return (
    <div className="home-page">
      <div className="page-head">
        <div>
          <p className="page-eyebrow">Cocktail Manager</p>
          <h2 className="page-title">审计日志</h2>
        </div>
        <button type="button" className="btn btn-ghost" onClick={onBack}>
          <i className="fa fa-arrow-left" /> 返回主界面
        </button>
      </div>

      <div className="card-panel">
        <p className="meta" style={{ marginTop: 0 }}>
          记录登录、实例启停、文件与备份等操作。最多展示最近 8000 条。
        </p>
        <div className="audit-filters">
          <label>
            操作
            <select
              value={action}
              onChange={(e) => {
                setOffset(0)
                setAction(e.target.value)
              }}
            >
              {ACTION_FILTERS.map((f) => (
                <option key={f.value || 'all'} value={f.value}>
                  {f.label}
                </option>
              ))}
            </select>
          </label>
          <label>
            实例
            <select
              value={instanceId}
              onChange={(e) => {
                setOffset(0)
                setInstanceId(e.target.value)
              }}
            >
              <option value="">全部实例</option>
              {instances.map((i) => (
                <option key={i.id} value={i.id}>
                  {i.spec.name}
                </option>
              ))}
            </select>
          </label>
          <form
            className="audit-search"
            onSubmit={(e) => {
              e.preventDefault()
              setOffset(0)
              setQ(qDraft.trim())
            }}
          >
            <label>
              搜索
              <input
                value={qDraft}
                onChange={(e) => setQDraft(e.target.value)}
                placeholder="动作、操作者、详情…"
              />
            </label>
            <button type="submit" className="btn btn-primary">
              筛选
            </button>
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => {
                setQDraft('')
                setQ('')
                setAction('')
                setInstanceId('')
                setOffset(0)
              }}
            >
              重置
            </button>
          </form>
        </div>
      </div>

      <div className="card-panel mt-6">
        <div className="audit-toolbar">
          <span className="meta">
            {loading ? '读取中…' : `共 ${total} 条`}
            {!loading && total > 0 ? ` · 第 ${page} / ${pages} 页` : ''}
          </span>
          <div className="btn-row">
            <button
              type="button"
              className="btn btn-ghost"
              disabled={offset === 0 || loading}
              onClick={() => setOffset(Math.max(0, offset - limit))}
            >
              上一页
            </button>
            <button
              type="button"
              className="btn btn-ghost"
              disabled={offset + limit >= total || loading}
              onClick={() => setOffset(offset + limit)}
            >
              下一页
            </button>
          </div>
        </div>

        <table className="data-table audit-table">
          <thead>
            <tr>
              <th>时间</th>
              <th>操作</th>
              <th>实例</th>
              <th>操作者</th>
              <th>详情</th>
            </tr>
          </thead>
          <tbody>
            {items.map((row, i) => (
              <tr key={`${row.at}-${row.action}-${i}`}>
                <td className="audit-time">{formatTime(row.at)}</td>
                <td>
                  <code className="audit-action">{actionLabel(row.action)}</code>
                </td>
                <td>
                  {row.instance_id ? (
                    <button
                      type="button"
                      className="link-btn"
                      onClick={() => onOpenInstance(row.instance_id!)}
                    >
                      {names.get(row.instance_id) ?? row.instance_id.slice(0, 8)}
                    </button>
                  ) : (
                    '—'
                  )}
                </td>
                <td>{row.actor}</td>
                <td className="audit-detail">{detailText(row.detail)}</td>
              </tr>
            ))}
            {!loading && items.length === 0 && (
              <tr>
                <td colSpan={5} className="empty">
                  还没有审计记录
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  )
}
