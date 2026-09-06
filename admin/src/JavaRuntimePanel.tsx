import { useEffect, useState } from 'react'
import {
  api,
  formatBytes,
  type JavaImageType,
  type JavaInventory,
} from './api'
import { BrandImg, BRAND } from './brandIcons'
import { JAVA_MAJORS } from './cores'

type Props = {
  busy: boolean
  onBusy: (v: boolean, label?: string) => void
  onError: (msg: string | null) => void
}

function sourceLabel(src: string) {
  if (src === 'system') return '系统 Java'
  if (src === 'managed') return '已安装的 Temurin'
  return 'Adoptium 下载'
}

export default function JavaRuntimePanel({ busy, onBusy, onError }: Props) {
  const [inv, setInv] = useState<JavaInventory | null>(null)

  const load = async () => {
    setInv(await api.javaInventory())
  }

  useEffect(() => {
    load().catch((e: Error) => onError(e.message))
  }, [])

  const run = async (fn: () => Promise<unknown>, label: string) => {
    onBusy(true, label)
    onError(null)
    try {
      await fn()
      await load()
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      onBusy(false)
    }
  }

  const majors = inv?.available_lts?.length ? inv.available_lts : [...JAVA_MAJORS]
  const rec = inv?.recommended_major ?? 21

  return (
    <div className="card-panel mt-6">
      <h3 className="card-title">
        <BrandImg src={BRAND.openjdk} alt="" height={16} /> Java 运行时（Adoptium
        Temurin）
      </h3>
      <p className="meta">
        进程模式启动或安装 Forge/NeoForge/Quilt 时，若本机 Java 缺失或版本不够，会自动下载对应
        JRE。也可在此预先安装 JDK/JRE。
        {inv
          ? ` 当前平台 ${inv.adoptium_os}/${inv.adoptium_arch}。`
          : ''}
      </p>

      <table className="info-table">
        <tbody>
          <tr>
            <td>系统 Java</td>
            <td>
              {inv?.system
                ? `${inv.system.version}（主版本 ${inv.system.major}）· ${inv.system.java_bin}`
                : '未检测到 PATH 中的 java'}
            </td>
          </tr>
        </tbody>
      </table>

      <div className="java-actions">
        <button
          type="button"
          className="btn btn-primary"
          disabled={busy}
          onClick={() =>
            run(
              () => api.ensureJava({ major: rec, image_type: 'jre' }),
              `补全 Temurin ${rec} JRE…`,
            )
          }
        >
          补全推荐 JRE {rec}
        </button>
        <button
          type="button"
          className="btn btn-ghost"
          disabled={busy}
          onClick={() =>
            run(
              () =>
                api.ensureJava({
                  major: rec,
                  image_type: 'jre',
                  managed: true,
                }),
              `下载托管 Temurin ${rec} JRE…`,
            )
          }
        >
          强制下载托管副本
        </button>
        <button
          type="button"
          className="btn btn-ghost"
          disabled={busy}
          onClick={() =>
            run(() => load(), '刷新 Java 列表…')
          }
        >
          刷新
        </button>
      </div>

      <h4 className="java-sub">已安装</h4>
      {inv && inv.installed.length === 0 ? (
        <p className="meta">还没有托管运行时，启动服务器时会按需从 Adoptium 拉取。</p>
      ) : (
        <table className="data-table java-table">
          <thead>
            <tr>
              <th>ID</th>
              <th>版本</th>
              <th>类型</th>
              <th>大小</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {(inv?.installed ?? []).map((rt) => (
              <tr key={rt.id}>
                <td>
                  <code>{rt.id}</code>
                  {rt.release_name ? (
                    <div className="meta">{rt.release_name}</div>
                  ) : null}
                </td>
                <td>{rt.major}</td>
                <td>{rt.image_type.toUpperCase()}</td>
                <td>{formatBytes(rt.size_bytes)}</td>
                <td>
                  <button
                    type="button"
                    className="btn btn-ghost"
                    disabled={busy}
                    onClick={() =>
                      run(
                        () => api.deleteJava(rt.id),
                        `删除 ${rt.id}…`,
                      )
                    }
                  >
                    删除
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <h4 className="java-sub">从 Adoptium 安装</h4>
      <div className="java-install-grid">
        {majors.map((major) => (
          <div key={major} className="java-install-row">
            <strong>Java {major}</strong>
            {(['jre', 'jdk'] as JavaImageType[]).map((kind) => {
              const id = `temurin-${major}-${kind}`
              const have = inv?.installed.some((r) => r.id === id)
              return (
                <button
                  key={kind}
                  type="button"
                  className="btn btn-ghost"
                  disabled={busy || have}
                  onClick={() =>
                    run(
                      () => api.installJava(major, kind),
                      `下载 Temurin ${major} ${kind.toUpperCase()}…`,
                    )
                  }
                >
                  {have ? `已有 ${kind.toUpperCase()}` : `安装 ${kind.toUpperCase()}`}
                </button>
              )
            })}
          </div>
        ))}
      </div>
      <p className="meta">
        下载来源：Eclipse Adoptium。安装器与开服优先用 JRE；需要完整 JDK 时再装 JDK。
        {inv ? ` 补全会优先使用${sourceLabel('system')}，不够再下载。` : ''}
      </p>
    </div>
  )
}
