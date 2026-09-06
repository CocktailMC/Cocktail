import { useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { api, type Automation, type Instance } from './api'

type Props = {
  instance: Instance
  busy: boolean
  onBusy: (v: boolean, label?: string) => void
  onError: (msg: string | null) => void
}

const COND: { id: string; label: string }[] = [
  { id: 'tps_below', label: 'TPS 低于阈值（持续）' },
  { id: 'players_above', label: '在线人数高于阈值' },
  { id: 'cpu_above', label: 'CPU 高于阈值' },
  { id: 'crashed', label: '进程崩溃 / 退出' },
]

export default function AutomationsPage({
  instance,
  busy,
  onBusy,
  onError,
}: Props) {
  const [rows, setRows] = useState<Automation[]>([])
  const [name, setName] = useState('TPS 过低重启')
  const [condition, setCondition] = useState('tps_below')
  const [threshold, setThreshold] = useState(10)
  const [duration, setDuration] = useState(60)
  const [actions, setActions] = useState('restart,notify_qq')
  const [standby, setStandby] = useState('')

  const load = async () => {
    try {
      setRows(await api.listAutomations(instance.id))
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    }
  }

  useEffect(() => {
    void load()
  }, [instance.id])

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    onBusy(true, '创建规则…')
    onError(null)
    try {
      const acts = actions
        .split(',')
        .map((s) => s.trim())
        .filter(Boolean)
      if (standby.trim()) acts.push(`start:${standby.trim()}`)
      await api.createAutomation({
        instance_id: instance.id,
        name: name.trim(),
        condition,
        threshold,
        duration_secs: condition === 'crashed' ? 0 : duration,
        actions: acts,
      })
      await load()
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err))
    } finally {
      onBusy(false)
    }
  }

  return (
    <>
      <div className="page-head">
        <div>
          <p className="page-eyebrow">{instance.spec.name}</p>
          <h2 className="page-title">自动化</h2>
        </div>
      </div>
      <div className="card-panel">
        <p className="meta mb-1">
          条件持续满足后执行动作。动作可用：<code>restart</code>、<code>start</code>、
          <code>notify_qq</code>、<code>start:&lt;实例ID&gt;</code>、<code>command:say hi</code>
          。崩溃自动拉起也可在设置里打开「崩溃后自动重启」。
        </p>
        <form className="settings" onSubmit={submit}>
          <label>
            名称
            <input value={name} onChange={(e) => setName(e.target.value)} required />
          </label>
          <label>
            条件
            <select value={condition} onChange={(e) => setCondition(e.target.value)}>
              {COND.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.label}
                </option>
              ))}
            </select>
          </label>
          {condition !== 'crashed' && (
            <>
              <label>
                阈值
                <input
                  type="number"
                  step={0.1}
                  value={threshold}
                  onChange={(e) => setThreshold(Number(e.target.value))}
                />
              </label>
              <label>
                持续秒数
                <input
                  type="number"
                  min={0}
                  value={duration}
                  onChange={(e) => setDuration(Number(e.target.value))}
                />
              </label>
            </>
          )}
          <label>
            动作（逗号分隔）
            <input
              value={actions}
              onChange={(e) => setActions(e.target.value)}
              placeholder="restart,notify_qq"
            />
          </label>
          <label>
            同时启动备用实例 ID（可选）
            <input
              value={standby}
              onChange={(e) => setStandby(e.target.value)}
              placeholder="另一台实例的 UUID"
            />
          </label>
          <button type="submit" className="btn btn-primary" disabled={busy}>
            添加规则
          </button>
        </form>
      </div>
      <div className="card-panel" style={{ marginTop: '1rem' }}>
        {rows.length === 0 ? (
          <p className="empty">还没有规则</p>
        ) : (
          <table className="data-table">
            <thead>
              <tr>
                <th>名称</th>
                <th>条件</th>
                <th>动作</th>
                <th>最近触发</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.id}>
                  <td>
                    <strong>{r.name}</strong>
                    <div className="meta">{r.enabled ? '启用' : '停用'}</div>
                  </td>
                  <td className="meta">
                    {r.condition}
                    {r.condition !== 'crashed'
                      ? ` ${r.threshold} / ${r.duration_secs}s`
                      : ''}
                  </td>
                  <td className="meta">{r.actions.join(', ')}</td>
                  <td className="meta">
                    {r.last_fired
                      ? new Date(r.last_fired).toLocaleString('zh-CN', {
                          hour12: false,
                        })
                      : '—'}
                  </td>
                  <td>
                    <button
                      type="button"
                      className="link-btn danger"
                      disabled={busy}
                      onClick={() => {
                        onBusy(true)
                        api
                          .deleteAutomation(r.id)
                          .then(load)
                          .catch((e: Error) => onError(e.message))
                          .finally(() => onBusy(false))
                      }}
                    >
                      删除
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </>
  )
}
