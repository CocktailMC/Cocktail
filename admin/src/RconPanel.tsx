import { useEffect, useState } from 'react'
import { api } from './api'

type Props = {
  instanceId: string
  running: boolean
  onError: (msg: string | null) => void
}

const QUICK: string[] = [
  'list',
  'tps',
  'seed',
  'difficulty',
  'weather clear',
  'time set day',
  'save-all',
  'whitelist list',
  'banlist players',
]

export default function RconPanel({ instanceId, running, onError }: Props) {
  const [enabled, setEnabled] = useState(false)
  const [port, setPort] = useState<number | null>(null)
  const [host, setHost] = useState<string | null>(null)
  const [password, setPassword] = useState('')
  const [command, setCommand] = useState('')
  const [busy, setBusy] = useState(false)
  const [output, setOutput] = useState<string[]>([])

  const load = async () => {
    try {
      const st = await api.rconStatus(instanceId)
      setEnabled(st.enabled)
      setPort(st.port ?? null)
      setHost(st.host ?? null)
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    }
  }

  useEffect(() => {
    void load()
  }, [instanceId])

  const toggle = async (next: boolean) => {
    setBusy(true)
    onError(null)
    try {
      const res = await api.rconSetup(instanceId, {
        enable: next,
        password: password.trim() || undefined,
      })
      setEnabled(res.enabled)
      setPort(res.port)
      await load()
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const exec = async (cmd: string) => {
    if (!cmd.trim()) return
    setBusy(true)
    onError(null)
    try {
      const res = await api.rconExec(instanceId, cmd)
      setOutput((prev) => [`$ ${cmd}`, res.response || '(空响应)', ...prev].slice(0, 60))
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="card-panel" style={{ marginTop: '1rem' }}>
      <h3 className="card-title">RCON 控制通道</h3>
      <p className="meta mb-1">
        通过 Minecraft 原生 RCON 协议直连服务器，绕开标准输入，可在不重启的前提下执行任意服务端命令。
        当前状态：{enabled ? `已开启 ${host ?? ''}:${port ?? ''}` : '未开启'}
      </p>
      <div className="settings">
        <label>
          RCON 密码
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder="留空则自动生成随机密码"
            disabled={busy}
          />
        </label>
        <button
          type="button"
          className="btn btn-primary"
          disabled={busy}
          onClick={() => void toggle(!enabled)}
        >
          {enabled ? '关闭 RCON' : '开启 RCON'}
        </button>
      </div>
      <div className="settings" style={{ marginTop: '0.75rem' }}>
        <label>
          命令
          <input
            value={command}
            onChange={(e) => setCommand(e.target.value)}
            placeholder="list"
            disabled={busy || !running || !enabled}
          />
        </label>
        <button
          type="button"
          className="btn btn-primary"
          disabled={busy || !running || !enabled}
          onClick={() => {
            void exec(command)
            setCommand('')
          }}
        >
          发送
        </button>
      </div>
      <div className="chip-row" style={{ marginTop: '0.5rem' }}>
        {QUICK.map((q) => (
          <button
            key={q}
            type="button"
            className="link-btn"
            disabled={busy || !running || !enabled}
            onClick={() => void exec(q)}
          >
            {q}
          </button>
        ))}
      </div>
      {!running && <p className="meta">实例未运行，RCON 无法连接。</p>}
      {output.length > 0 && (
        <pre className="console" style={{ whiteSpace: 'pre-wrap', marginTop: '0.75rem' }}>
          {output.join('\n')}
        </pre>
      )}
    </div>
  )
}