import { useEffect, useState } from 'react'
import { api } from './api'

type Props = {
  instanceId: string
  busy: boolean
  onBusy: (v: boolean, label?: string) => void
  onError: (msg: string | null) => void
}

export default function SpecYamlPanel({
  instanceId,
  busy,
  onBusy,
  onError,
}: Props) {
  const [yaml, setYaml] = useState('')
  const [loading, setLoading] = useState(false)

  const load = async () => {
    setLoading(true)
    try {
      setYaml(await api.getInstanceSpec(instanceId))
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void load()
  }, [instanceId])

  const apply = async () => {
    onBusy(true, '应用声明式 spec…')
    onError(null)
    try {
      await api.applyInstanceSpec(instanceId, yaml)
      await load()
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      onBusy(false)
    }
  }

  return (
    <div className="card-panel" style={{ marginTop: '1rem' }}>
      <h3 className="card-title">声明式 Spec（YAML）</h3>
      <p className="meta mb-1">
        以 spec 为真相源。修改 <code>desired_running</code> / <code>node_id</code>{' '}
        后应用，控制面会协调本机或远程 Agent。
      </p>
      <textarea
        value={yaml}
        onChange={(e) => setYaml(e.target.value)}
        rows={18}
        spellCheck={false}
        style={{ fontFamily: 'ui-monospace, monospace', width: '100%' }}
        disabled={loading || busy}
      />
      <div className="btn-row mt-4">
        <button type="button" className="btn btn-ghost" disabled={busy} onClick={() => void load()}>
          重新加载
        </button>
        <button type="button" className="btn btn-primary" disabled={busy} onClick={() => void apply()}>
          应用 spec
        </button>
      </div>
    </div>
  )
}
