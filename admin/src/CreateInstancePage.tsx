import { useEffect, useMemo, useState } from 'react'
import type { FormEvent } from 'react'
import {
  api,
  type CoreLoader,
  type CoreVersion,
  type CreateInstanceBody,
  type DockerImage,
  type Instance,
  type NodeInfo,
} from './api'
import {
  INSTALLABLE_CORE_GROUPS,
  JAVA_MAJORS,
  coreHasLoaders,
  isInstallableCore,
  recommendedJavaMajor,
} from './cores'
import {
  EULA_AKA_URL,
  EULA_OFFICIAL_URL,
  EULA_SECTIONS,
  EULA_ZH_SUMMARY,
} from './eulaText'

const STEPS = ['基本信息', '运行时与资源', '核心与启动', 'EULA', '确认创建'] as const

type Props = {
  usedPorts: number[]
  dockerAvailable: boolean
  dockerMessage: string
  busy: boolean
  onBusy: (v: boolean, label?: string) => void
  onError: (msg: string | null) => void
  onCancel: () => void
  onCreated: (inst: Instance, next: 'version' | 'dashboard') => void
}

export default function CreateInstancePage({
  usedPorts,
  dockerAvailable,
  dockerMessage,
  busy,
  onBusy,
  onError,
  onCancel,
  onCreated,
}: Props) {
  const [step, setStep] = useState(0)
  const [name, setName] = useState('my-server')
  const [group, setGroup] = useState('default')
  const [tags, setTags] = useState('')
  const [core, setCore] = useState('custom')
  const [runtime, setRuntime] = useState<'process' | 'docker'>('process')
  const [image, setImage] = useState('eclipse-temurin:21-jre')
  const [cpu, setCpu] = useState(1)
  const [memory, setMemory] = useState(2048)
  const [port, setPort] = useState(() => {
    let p = 25565
    while (usedPorts.includes(p)) p += 1
    return p
  })
  const [autoRestart, setAutoRestart] = useState(false)
  const [command, setCommand] = useState('java')
  const [args, setArgs] = useState('-jar server.jar nogui')
  const [eulaRead, setEulaRead] = useState(false)
  const [eulaAccepted, setEulaAccepted] = useState(false)
  const [jarFile, setJarFile] = useState<File | null>(null)
  const [nodes, setNodes] = useState<NodeInfo[]>([])
  const [nodeId, setNodeId] = useState('local')
  const [gameVersions, setGameVersions] = useState<CoreVersion[]>([])
  const [gameVersion, setGameVersion] = useState('')
  const [loaders, setLoaders] = useState<CoreLoader[]>([])
  const [loader, setLoader] = useState('')
  const [versionsLoading, setVersionsLoading] = useState(false)
  const [javaMajor, setJavaMajor] = useState(0)
  const [dockerImages, setDockerImages] = useState<DockerImage[]>([])

  useEffect(() => {
    api
      .listNodes()
      .then((list) => {
        setNodes(list)
        if (!list.some((n) => n.id === nodeId) && list[0]) {
          setNodeId(list[0].id)
        }
      })
      .catch(() => undefined)
  }, [])

  useEffect(() => {
    if (!dockerAvailable) return
    api
      .dockerImages()
      .then(setDockerImages)
      .catch(() => setDockerImages([]))
  }, [dockerAvailable])

  useEffect(() => {
    if (!isInstallableCore(core)) {
      setGameVersions([])
      setGameVersion('')
      setLoaders([])
      setLoader('')
      return
    }
    let cancelled = false
    setVersionsLoading(true)
    setLoaders([])
    setLoader('')
    api
      .listCoreVersions(core)
      .then((list) => {
        if (cancelled) return
        setGameVersions(list)
        setGameVersion(list.find((v) => v.latest)?.id ?? list[0]?.id ?? '')
      })
      .catch((err: Error) => {
        if (cancelled) return
        setGameVersions([])
        setGameVersion('')
        onError(err.message)
      })
      .finally(() => {
        if (!cancelled) setVersionsLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [core])

  useEffect(() => {
    if (!isInstallableCore(core) || !gameVersion || !coreHasLoaders(core)) {
      setLoaders([])
      setLoader('')
      return
    }
    let cancelled = false
    api
      .listCoreLoaders(core, gameVersion)
      .then((list) => {
        if (cancelled) return
        setLoaders(list)
        setLoader('')
      })
      .catch(() => {
        if (cancelled) return
        setLoaders([])
        setLoader('')
      })
    return () => {
      cancelled = true
    }
  }, [core, gameVersion])

  const needsEula = core !== 'demo'
  const portConflict = usedPorts.includes(port)

  const previewCmd = useMemo(() => {
    if (core === 'demo') return '内置 demo（无需 jar）'
    return `${command.trim() || 'java'} ${args.trim()}`
  }, [core, command, args])

  const canNext = () => {
    if (step === 0) return name.trim().length > 0
    if (step === 1) {
      if (port < 1 || port > 65535 || portConflict) return false
      if (memory < 256) return false
      if (runtime === 'docker' && !image.trim()) return false
      return true
    }
    if (step === 2) return true
    if (step === 3) {
      if (!needsEula) return true
      return eulaRead && eulaAccepted
    }
    return true
  }

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    if (step < STEPS.length - 1) {
      if (!canNext()) return
      setStep((s) => s + 1)
      return
    }
    const autoInstall =
      isInstallableCore(core) && Boolean(gameVersion) && !jarFile
    onBusy(
      true,
      jarFile
        ? '创建实例并导入 jar…'
        : autoInstall
          ? `创建实例并安装 ${core} ${gameVersion}${loader ? ` / ${loader}` : ''}…`
          : '创建实例…',
    )
    onError(null)
    try {
      const body: CreateInstanceBody = {
        name: name.trim(),
        core,
        memory_mib: memory,
        port,
        auto_restart: autoRestart,
        eula_accepted: needsEula ? eulaAccepted : true,
        runtime,
        group: group.trim() || 'default',
        node_id: nodeId,
        tags: tags
          .split(',')
          .map((t) => t.trim())
          .filter(Boolean),
        docker_image: runtime === 'docker' ? image.trim() : undefined,
        cpu_limit: runtime === 'docker' ? cpu : undefined,
        java_major: javaMajor >= 8 ? javaMajor : undefined,
      }
      if (core === 'custom' && !jarFile) {
        body.command = command.trim() || 'java'
        body.args = args.trim().split(/\s+/).filter(Boolean)
      }
      const created = await api.createInstance(body)
      if (jarFile) {
        await api.installJar(created.id, jarFile, {
          path: 'server.jar',
          core: 'custom',
          accept_eula: eulaAccepted || !needsEula,
        })
      } else if (autoInstall) {
        await api.installCore(created.id, core, gameVersion, loader || undefined)
      }
      const fresh = (await api.listInstances()).find((i) => i.id === created.id)
      onCreated(
        fresh ?? created,
        core === 'demo' || jarFile || autoInstall ? 'dashboard' : 'version',
      )
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
          <p className="eyebrow">新建实例</p>
          <h2>创建 Minecraft 服务器</h2>
          <p className="meta">
            配置运行时、资源与启动方式；正式核心需同意 Mojang EULA。
          </p>
        </div>
        <button type="button" className="btn btn-ghost" onClick={onCancel}>
          返回
        </button>
      </div>

      <ol className="wizard-steps">
        {STEPS.map((label, i) => (
          <li
            key={label}
            className={
              i === step ? 'active' : i < step ? 'done' : undefined
            }
          >
            <span className="step-num">{i + 1}</span>
            {label}
          </li>
        ))}
      </ol>

      <form className="card-panel wizard-panel" onSubmit={submit}>
        {step === 0 && (
          <div className="settings">
            <h3 className="card-title">基本信息</h3>
            <label>
              实例名称
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                required
                placeholder="my-server"
              />
            </label>
            <label>
              调度节点
              <select value={nodeId} onChange={(e) => setNodeId(e.target.value)}>
                {(nodes.length ? nodes : [{ id: 'local', name: '本机控制面', kind: 'local', created_at: '', online: true }]).map(
                  (n) => (
                    <option key={n.id} value={n.id}>
                      {n.name}
                      {n.kind === 'local' ? '（本机）' : n.online ? '（在线）' : '（离线）'}
                      {n.instance_count != null ? ` · ${n.instance_count} 实例` : ''}
                    </option>
                  ),
                )}
              </select>
            </label>
            <p className="meta">
              部署节点可选本机或远程 Agent。节点名称建议用「上海 / 北京 / 海外」区分区域。
            </p>
            <label>
              分组
              <input
                value={group}
                onChange={(e) => setGroup(e.target.value)}
                placeholder="default"
              />
            </label>
            <label>
              标签（逗号分隔）
              <input
                value={tags}
                onChange={(e) => setTags(e.target.value)}
                placeholder="survival, smp"
              />
            </label>
          </div>
        )}

        {step === 1 && (
          <div className="settings">
            <h3 className="card-title">运行时与资源</h3>
            <fieldset className="settings" style={{ border: 0, padding: 0, margin: 0 }}>
              <legend className="label">运行方式</legend>
              <label className="check">
                <input
                  type="radio"
                  name="runtime"
                  checked={runtime === 'process'}
                  onChange={() => setRuntime('process')}
                />
                Java 进程（本机 java）
              </label>
              <label className="check">
                <input
                  type="radio"
                  name="runtime"
                  checked={runtime === 'docker'}
                  onChange={() => setRuntime('docker')}
                  disabled={!dockerAvailable}
                />
                Docker 容器
              </label>
            </fieldset>
            <label>
              Java 版本
              <select
                value={javaMajor}
                onChange={(e) => {
                  const v = Number(e.target.value)
                  setJavaMajor(v)
                  const major = v || recommendedJavaMajor(gameVersion || undefined)
                  if (
                    runtime === 'docker' &&
                    (!image.trim() || image.startsWith('eclipse-temurin:'))
                  ) {
                    setImage(`eclipse-temurin:${major}-jre`)
                  }
                }}
              >
                <option value={0}>
                  自动（推荐 {recommendedJavaMajor(gameVersion || undefined)}
                  {gameVersion ? ` · ${gameVersion}` : ''}）
                </option>
                {JAVA_MAJORS.map((m) => (
                  <option key={m} value={m}>
                    Java {m}
                    {m === 8 ? '（1.16 及更早）' : ''}
                    {m === 17 ? '（1.17–1.20.4）' : ''}
                    {m === 21 ? '（1.20.5+）' : ''}
                  </option>
                ))}
              </select>
            </label>
            <p className="meta">
              本机进程：若 PATH 里没有合适的 Java，会从 Adoptium 自动下载 Temurin
              JRE。Docker：默认使用 eclipse-temurin 镜像。
            </p>
            {runtime === 'docker' && (
              <>
                <p className={dockerAvailable ? 'meta ok' : 'error'}>
                  {dockerMessage ||
                    (dockerAvailable ? 'Docker 就绪' : 'Docker 不可用')}
                </p>
                <label>
                  镜像
                  <input
                    value={image}
                    onChange={(e) => setImage(e.target.value)}
                    list="docker-images"
                    placeholder="eclipse-temurin:21-jre"
                  />
                  <datalist id="docker-images">
                    {dockerImages.map((im) => (
                      <option key={im.id + im.repo_tag} value={im.repo_tag}>
                        {im.size}
                      </option>
                    ))}
                  </datalist>
                </label>
                {dockerImages.length > 0 && (
                  <p className="meta">
                    本机已有 {dockerImages.length} 个镜像；端口映射 主机:{port} →
                    容器:25565，内存/CPU 作为资源限制。
                  </p>
                )}
                <label>
                  CPU 限制（--cpus）
                  <input
                    type="number"
                    min={0.1}
                    step={0.1}
                    value={cpu}
                    onChange={(e) => setCpu(Number(e.target.value))}
                  />
                </label>
              </>
            )}
            <label>
              内存 MiB（进程注入 -Xmx；容器同时 --memory）
              <input
                type="number"
                min={256}
                value={memory}
                onChange={(e) => setMemory(Number(e.target.value))}
              />
            </label>
            <label>
              端口
              <input
                type="number"
                min={1}
                max={65535}
                value={port}
                onChange={(e) => setPort(Number(e.target.value))}
              />
            </label>
            {portConflict && (
              <p className="error">端口 {port} 已被其他实例占用</p>
            )}
            <p className="meta">
              Docker 映射为 主机:{port} → 容器:25565；进程模式直接监听该端口。
            </p>
            <label className="check">
              <input
                type="checkbox"
                checked={autoRestart}
                onChange={(e) => setAutoRestart(e.target.checked)}
              />
              崩溃后自动重启
            </label>
          </div>
        )}

        {step === 2 && (
          <div className="settings">
            <h3 className="card-title">核心与启动</h3>
            <label>
              服务端类型
              <select
                value={core}
                onChange={(e) => setCore(e.target.value)}
              >
                <option value="custom">自定义 jar（推荐）</option>
                {INSTALLABLE_CORE_GROUPS.map((g) => (
                  <optgroup key={g.label} label={g.label}>
                    {g.items.map((item) => (
                      <option key={item.id} value={item.id}>
                        {item.label}（创建后可在线安装）
                      </option>
                    ))}
                  </optgroup>
                ))}
                <option value="demo">Demo（无需 jar，仅联调）</option>
              </select>
            </label>
            {core === 'custom' && (
              <>
                <label>
                  导入 server.jar（可选，也可创建后再导入）
                  <input
                    type="file"
                    accept=".jar"
                    onChange={(e) => setJarFile(e.target.files?.[0] ?? null)}
                  />
                </label>
                {jarFile && (
                  <p className="meta ok">已选择：{jarFile.name}</p>
                )}
                <label>
                  启动命令
                  <input
                    value={command}
                    onChange={(e) => setCommand(e.target.value)}
                    placeholder="java"
                    disabled={!!jarFile}
                  />
                </label>
                <label>
                  启动参数
                  <input
                    value={args}
                    onChange={(e) => setArgs(e.target.value)}
                    placeholder="-jar server.jar nogui"
                    disabled={!!jarFile}
                  />
                </label>
                <p className="meta">
                  {jarFile
                    ? '导入 jar 后将自动配置为 java -jar server.jar nogui'
                    : `当前预览：${previewCmd}`}
                </p>
              </>
            )}
            {isInstallableCore(core) && (
              <>
                <label>
                  游戏版本（可选，默认最新）
                  <select
                    value={gameVersion}
                    onChange={(e) => setGameVersion(e.target.value)}
                    disabled={versionsLoading}
                  >
                    <option value="">稍后安装</option>
                    {gameVersions.map((v) => (
                      <option key={v.id} value={v.id}>
                        {v.label ?? v.id}
                        {v.latest ? '（最新）' : ''}
                      </option>
                    ))}
                  </select>
                </label>
                {coreHasLoaders(core) && gameVersion && (
                  <label>
                    {core === 'arclight' ? '混合变体（可选）' : '加载器版本（可选）'}
                    <select
                      value={loader}
                      onChange={(e) => setLoader(e.target.value)}
                    >
                      <option value="">
                        {core === 'arclight'
                          ? '优先 NeoForge，没有则回退'
                          : '最新稳定（默认）'}
                      </option>
                      {loaders.map((l) => (
                        <option key={l.id} value={l.id}>
                          {l.label ? `${l.id}（${l.label}）` : l.id}
                          {l.latest ? ' · latest' : ''}
                          {l.recommended && !l.label ? ' · recommended' : ''}
                        </option>
                      ))}
                    </select>
                  </label>
                )}
                <p className="meta">
                  {gameVersion
                    ? core === 'forge' ||
                      core === 'neoforge' ||
                      core === 'quilt'
                      ? '创建后会自动下载安装器；若本机没有合适的 Java，会从 Adoptium 补全 Temurin 再安装。'
                      : '创建后会自动下载服务器包并配置启动命令。'
                    : '未选版本则创建空实例，之后可在「版本 / jar」页安装。'}
                </p>
              </>
            )}
            {core === 'demo' && (
              <p className="meta">Demo 模式不需要 EULA 与 jar，适合验证控制面。</p>
            )}
          </div>
        )}

        {step === 3 && (
          <div className="eula-step">
            <h3 className="card-title">Minecraft EULA</h3>
            {needsEula ? (
              <>
                <div className="eula-zh">{EULA_ZH_SUMMARY}</div>
                <p className="meta eula-links">
                  官方原文：
                  <a href={EULA_OFFICIAL_URL} target="_blank" rel="noreferrer">
                    minecraft.net/eula
                  </a>
                  {' · '}
                  <a href={EULA_AKA_URL} target="_blank" rel="noreferrer">
                    aka.ms/MinecraftEULA
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
                    checked={eulaRead}
                    onChange={(e) => setEulaRead(e.target.checked)}
                  />
                  我已阅读上方 EULA 原文（及官网最新版本）
                </label>
                <label className="check">
                  <input
                    type="checkbox"
                    checked={eulaAccepted}
                    onChange={(e) => setEulaAccepted(e.target.checked)}
                    disabled={!eulaRead}
                  />
                  我同意 Minecraft EULA，并授权 Cocktail 写入 eula=true
                </label>
              </>
            ) : (
              <p className="meta">Demo 实例无需同意 EULA，可直接创建。</p>
            )}
          </div>
        )}

        {step === 4 && (
          <div className="settings">
            <h3 className="card-title">确认创建</h3>
            <table className="info-table">
              <tbody>
                <tr>
                  <td>名称</td>
                  <td>{name.trim()}</td>
                </tr>
                <tr>
                  <td>分组 / 标签</td>
                  <td>
                    {group || 'default'}
                    {tags ? ` · ${tags}` : ''}
                  </td>
                </tr>
                <tr>
                  <td>运行时</td>
                  <td>
                    {runtime === 'docker'
                      ? `Docker · ${image} · ${cpu} CPU`
                      : '本机进程'}
                    {` · Java ${javaMajor >= 8 ? javaMajor : `自动 ${recommendedJavaMajor(gameVersion || undefined)}`}`}
                  </td>
                </tr>
                <tr>
                  <td>内存 / 端口</td>
                  <td>
                    {memory} MiB · :{port}
                  </td>
                </tr>
                <tr>
                  <td>核心</td>
                  <td>
                    {core}
                    {jarFile
                      ? ` · 将导入 ${jarFile.name}`
                      : isInstallableCore(core) && gameVersion
                        ? ` · ${gameVersion}${loader ? ` / ${loader}` : ''}`
                        : isInstallableCore(core)
                          ? ' · 稍后安装'
                          : ''}
                  </td>
                </tr>
                <tr>
                  <td>启动</td>
                  <td>
                    <code>{jarFile ? 'java -jar server.jar nogui' : previewCmd}</code>
                  </td>
                </tr>
                <tr>
                  <td>EULA</td>
                  <td>{needsEula ? (eulaAccepted ? '已同意' : '未同意') : '不适用'}</td>
                </tr>
                <tr>
                  <td>自动重启</td>
                  <td>{autoRestart ? '是' : '否'}</td>
                </tr>
              </tbody>
            </table>
          </div>
        )}

        <div className="wizard-actions">
          <button
            type="button"
            className="btn btn-ghost"
            disabled={busy || step === 0}
            onClick={() => setStep((s) => Math.max(0, s - 1))}
          >
            上一步
          </button>
          <div className="spacer" />
          <button
            type="button"
            className="btn btn-ghost"
            disabled={busy}
            onClick={onCancel}
          >
            取消
          </button>
          <button
            type="submit"
            className="btn btn-primary"
            disabled={busy || !canNext()}
          >
            {step < STEPS.length - 1 ? '下一步' : busy ? '创建中…' : '创建实例'}
          </button>
        </div>
      </form>
    </div>
  )
}
