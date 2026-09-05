import { useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { api, type NodeInfo } from './api'

type Props = {
  onBack: () => void
  onError: (msg: string | null) => void
}

export default function NodesPage({ onBack, onError }: Props) {
  const [nodes, setNodes] = useState<NodeInfo[]>([])
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  const [tokenOnce, setTokenOnce] = useState<{ name: string; token: string } | null>(
    null,
  )

  const load = async () => {
    try {
      setNodes(await api.listNodes())
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    }
  }

  useEffect(() => {
    void load()
    const t = setInterval(() => void load(), 8000)
    return () => clearInterval(t)
  }, [])

  const create = async (e: FormEvent) => {
    e.preventDefault()
    if (!name.trim()) return
    setBusy(true)
    onError(null)
    try {
      const created = await api.createNode(name.trim())
      setTokenOnce({ name: created.node.name, token: created.token })
      setName('')
      await load()
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(false)
    }
  }

  const remove = async (id: string, label: string) => {
    if (!window.confirm(`删除节点「${label}」？绑定该节点的实例需先删掉。`)) return
    setBusy(true)
    try {
      await api.deleteNode(id)
      await load()
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="page-flow">
      <div className="page-head">
        <div>
          <p className="page-eyebrow">控制面</p>
          <h2 className="page-title">节点 / Agent</h2>
        </div>
        <button type="button" className="btn btn-ghost" onClick={onBack}>
          返回主界面
        </button>
      </div>

      <div className="card-panel">
        <h3 className="card-title">注册远程节点</h3>
        <p className="meta mb-1">
          本机控制面始终在线。远程机器安装同一套二进制后运行{' '}
          <code>cocktail-agent</code>，用下面生成的 Token 接入。
        </p>
        <form className="settings" onSubmit={create}>
          <label>
            节点名称
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="edge-1"
              disabled={busy}
            />
          </label>
          <button type="submit" className="btn btn-primary" disabled={busy}>
            创建节点
          </button>
        </form>
        {tokenOnce && (
          <div className="card-panel" style={{ marginTop: '1rem' }}>
            <p className="meta">
              节点 <strong>{tokenOnce.name}</strong> 的 Token 只显示一次，请立刻复制到 Agent
              环境变量：
            </p>
            <pre className="console" style={{ whiteSpace: 'pre-wrap' }}>
              {`export COCKTAIL_PLANE=http://<控制面地址>:11011
export COCKTAIL_NODE_TOKEN=${tokenOnce.token}
cocktail-agent`}
            </pre>
          </div>
        )}
      </div>

      <div className="card-panel" style={{ marginTop: '1rem' }}>
        <h3 className="card-title">节点列表</h3>
        {nodes.length === 0 ? (
          <p className="meta">暂无节点（首次启动后会出现本机节点）</p>
        ) : (
          <div className="table-wrap">
            <table className="data-table">
              <thead>
                <tr>
                  <th>名称</th>
                  <th>类型</th>
                  <th>状态</th>
                  <th>主机</th>
                  <th>最近心跳</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {nodes.map((n) => (
                  <tr key={n.id}>
                    <td>
                      <strong>{n.name}</strong>
                      <div className="meta">{n.id}</div>
                    </td>
                    <td>{n.kind === 'local' ? '本机' : 'Agent'}</td>
                    <td>
                      <span className={`badge status-${n.online ? 'running' : 'stopped'}`}>
                        {n.online ? '在线' : '离线'}
                      </span>
                    </td>
                    <td>
                      {n.hostname || '—'}
                      {n.os ? ` · ${n.os}/${n.arch ?? ''}` : ''}
                    </td>
                    <td>{n.last_seen || '—'}</td>
                    <td>
                      {n.kind !== 'local' && (
                        <button
                          type="button"
                          className="btn btn-ghost"
                          disabled={busy}
                          onClick={() => remove(n.id, n.name)}
                        >
                          删除
                        </button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  )
}
