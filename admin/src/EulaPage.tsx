import { useState } from 'react'
import type { Instance } from './api'
import { api } from './api'
import {
  EULA_AKA_URL,
  EULA_OFFICIAL_URL,
  EULA_SECTIONS,
  EULA_USAGE_URL,
  EULA_ZH_SUMMARY,
} from './eulaText'

type Props = {
  instance: Instance
  busy: boolean
  onBusy: (v: boolean, label?: string) => void
  onError: (msg: string | null) => void
  onCancel: () => void
  onAccepted: (inst: Instance) => void
}

export default function EulaPage({
  instance,
  busy,
  onBusy,
  onError,
  onCancel,
  onAccepted,
}: Props) {
  const [read, setRead] = useState(false)
  const [accepted, setAccepted] = useState(false)

  const submit = async () => {
    if (!read || !accepted) return
    onBusy(true, '保存 EULA 同意…')
    onError(null)
    try {
      const updated = await api.acceptEula(instance.id, true)
      onAccepted(updated)
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err))
    } finally {
      onBusy(false)
    }
  }

  return (
    <div className="page-flow">
      <div className="page-flow-head">
        <div>
          <p className="eyebrow">法律协议</p>
          <h2>同意 Minecraft EULA</h2>
          <p className="meta">
            实例 <strong>{instance.spec.name}</strong> 启动正式服务端前必须同意
            Mojang / Microsoft 最终用户许可协议。同意后将写入工作目录{' '}
            <code>eula.txt</code>（eula=true）。
          </p>
        </div>
        <button type="button" className="btn btn-ghost" onClick={onCancel}>
          返回
        </button>
      </div>

      <div className="card-panel eula-page">
        {instance.spec.eula_accepted ? (
          <p className="ok">此实例已同意 EULA。可直接返回启动服务器。</p>
        ) : null}

        <div className="eula-zh">{EULA_ZH_SUMMARY}</div>

        <p className="meta eula-links">
          官方原文与相关链接：
          <a href={EULA_OFFICIAL_URL} target="_blank" rel="noreferrer">
            minecraft.net/eula
          </a>
          {' · '}
          <a href={EULA_AKA_URL} target="_blank" rel="noreferrer">
            aka.ms/MinecraftEULA
          </a>
          {' · '}
          <a href={EULA_USAGE_URL} target="_blank" rel="noreferrer">
            Usage Guidelines
          </a>
        </p>

        <div className="eula-scroll" tabIndex={0}>
          {EULA_SECTIONS.map((sec) => (
            <section key={sec.title} className="eula-section">
              <h4>{sec.title}</h4>
              <pre>{sec.body}</pre>
            </section>
          ))}
        </div>

        <label className="check">
          <input
            type="checkbox"
            checked={read}
            onChange={(e) => setRead(e.target.checked)}
            disabled={instance.spec.eula_accepted}
          />
          我已阅读上方 EULA 原文，并知悉官网可能更新条款
        </label>
        <label className="check">
          <input
            type="checkbox"
            checked={accepted}
            onChange={(e) => setAccepted(e.target.checked)}
            disabled={!read || instance.spec.eula_accepted}
          />
          我同意 Minecraft EULA，授权 Cocktail Manager 写入 eula=true
        </label>

        <div className="wizard-actions">
          <button
            type="button"
            className="btn btn-ghost"
            disabled={busy}
            onClick={onCancel}
          >
            取消
          </button>
          <div className="spacer" />
          <button
            type="button"
            className="btn btn-primary"
            disabled={
              busy ||
              instance.spec.eula_accepted ||
              !read ||
              !accepted
            }
            onClick={submit}
          >
            {busy ? '保存中…' : '同意并继续'}
          </button>
        </div>
      </div>
    </div>
  )
}
