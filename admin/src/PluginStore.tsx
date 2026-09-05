import { useState } from 'react'
import {
  api,
  type HangarHit,
  type HangarVersion,
  type ModrinthHit,
  type ModrinthVersion,
  type SpigetHit,
  type SpigetVersion,
} from './api'
import { BrandImg, BRAND } from './brandIcons'

type Source = 'modrinth' | 'hangar' | 'spiget'

type Props = {
  instanceId: string
  busy: boolean
  defaultGameVersion?: string
  defaultLoader?: string
  onInstalled: () => void
  run: (fn: () => Promise<unknown>, label?: string) => Promise<void>
}

const SOURCES: {
  id: Source
  label: string
  hint: string
  icon?: string
}[] = [
  {
    id: 'modrinth',
    label: 'Modrinth',
    hint: '插件与模组',
    icon: BRAND.modrinth,
  },
  { id: 'hangar', label: 'Hangar', hint: 'Paper 生态' },
  {
    id: 'spiget',
    label: 'Spiget',
    hint: 'SpigotMC',
    icon: BRAND.spigotmc,
  },
]

function fmtCount(n: number) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 10_000) return `${(n / 1000).toFixed(1)}k`
  return n.toLocaleString()
}

function StoreIcon({
  src,
  fallback,
}: {
  src?: string | null
  fallback: string
}) {
  const [broken, setBroken] = useState(false)
  const showImg = Boolean(src) && !broken

  return (
    <div className="store-icon-wrap">
      {showImg ? (
        <img
          src={src!}
          alt=""
          className="store-icon"
          loading="lazy"
          onError={() => setBroken(true)}
        />
      ) : (
        <div className="store-icon placeholder">
          <i className={`fa ${fallback}`} />
        </div>
      )}
    </div>
  )
}

export default function PluginStore({
  instanceId,
  busy,
  defaultGameVersion = '',
  defaultLoader = 'paper',
  onInstalled,
  run,
}: Props) {
  const [source, setSource] = useState<Source>('modrinth')
  const [query, setQuery] = useState('')
  const [projectType, setProjectType] = useState<'plugin' | 'mod'>('plugin')
  const [gameVersion, setGameVersion] = useState(defaultGameVersion)
  const [loader, setLoader] = useState(defaultLoader)
  const [platform, setPlatform] = useState('PAPER')
  const [searched, setSearched] = useState(false)

  const [mrHits, setMrHits] = useState<ModrinthHit[]>([])
  const [mrTotal, setMrTotal] = useState(0)
  const [hgHits, setHgHits] = useState<HangarHit[]>([])
  const [hgTotal, setHgTotal] = useState(0)
  const [spHits, setSpHits] = useState<SpigetHit[]>([])

  const [expanded, setExpanded] = useState<string | null>(null)
  const [mrVersions, setMrVersions] = useState<ModrinthVersion[]>([])
  const [hgVersions, setHgVersions] = useState<HangarVersion[]>([])
  const [spVersions, setSpVersions] = useState<SpigetVersion[]>([])
  const [picked, setPicked] = useState('')

  const search = () =>
    run(async () => {
      setExpanded(null)
      setPicked('')
      setSearched(true)
      if (source === 'modrinth') {
        const res = await api.modrinthSearch({
          query,
          project_type: projectType,
          game_version: gameVersion || undefined,
          loader: loader || undefined,
          limit: 24,
        })
        setMrHits(res.hits)
        setMrTotal(res.total_hits)
      } else if (source === 'hangar') {
        const res = await api.hangarSearch({
          query,
          platform,
          limit: 24,
        })
        setHgHits(res.hits)
        setHgTotal(res.total_hits)
      } else {
        const res = await api.spigetSearch({ query, size: 24, page: 1 })
        setSpHits(res.hits)
      }
    }, `搜索 ${source}…`)

  const switchSource = (id: Source) => {
    setSource(id)
    setExpanded(null)
    setSearched(false)
  }

  const resultMeta =
    source === 'modrinth' && mrTotal > 0
      ? `共 ${fmtCount(mrTotal)} · 显示 ${mrHits.length}`
      : source === 'hangar' && hgTotal > 0
        ? `共 ${fmtCount(hgTotal)} · 显示 ${hgHits.length}`
        : source === 'spiget' && spHits.length > 0
          ? `显示 ${spHits.length}`
          : null

  return (
    <div className="store-panel">
      <div className="store-head">
        <div>
          <h3 className="card-title">
            <i className="fa fa-cloud-download" /> 插件商店
          </h3>
          <p className="store-sub">从公开源搜索并一键安装到本实例</p>
        </div>
        <div className="store-tabs" role="tablist">
          {SOURCES.map((s) => (
            <button
              key={s.id}
              type="button"
              role="tab"
              aria-selected={source === s.id}
              className={`store-tab${source === s.id ? ' active' : ''}`}
              onClick={() => switchSource(s.id)}
            >
              <span className="store-tab-label">
                {s.icon ? (
                  <BrandImg src={s.icon} alt="" height={14} />
                ) : (
                  <i className="fa fa-paper-plane" />
                )}{' '}
                {s.label}
              </span>
              <span className="store-tab-hint">{s.hint}</span>
            </button>
          ))}
        </div>
      </div>

      <div className="store-search">
        <div className="store-search-row">
          <div className="store-search-input">
            <i className="fa fa-search" />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={
                source === 'hangar'
                  ? '搜索 Hangar，如 ViaVersion'
                  : source === 'spiget'
                    ? '搜索 SpigotMC，如 EssentialsX'
                    : '搜索 Modrinth，如 LuckPerms'
              }
              onKeyDown={(e) => {
                if (e.key === 'Enter') search()
              }}
            />
          </div>
          <button
            type="button"
            className="btn btn-primary"
            disabled={busy}
            onClick={search}
          >
            搜索
          </button>
        </div>
        <div className="store-filters">
          {source === 'modrinth' && (
            <>
              <select
                value={projectType}
                onChange={(e) => {
                  const t = e.target.value as 'plugin' | 'mod'
                  setProjectType(t)
                  setLoader(t === 'plugin' ? 'paper' : 'fabric')
                }}
              >
                <option value="plugin">插件</option>
                <option value="mod">模组</option>
              </select>
              <select value={loader} onChange={(e) => setLoader(e.target.value)}>
                {projectType === 'plugin' ? (
                  <>
                    <option value="paper">paper</option>
                    <option value="spigot">spigot</option>
                    <option value="purpur">purpur</option>
                    <option value="velocity">velocity</option>
                  </>
                ) : (
                  <>
                    <option value="fabric">fabric</option>
                    <option value="forge">forge</option>
                    <option value="neoforge">neoforge</option>
                  </>
                )}
                <option value="">不限加载器</option>
              </select>
              <input
                className="store-ver"
                value={gameVersion}
                onChange={(e) => setGameVersion(e.target.value)}
                placeholder="MC 版本"
              />
            </>
          )}
          {source === 'hangar' && (
            <select value={platform} onChange={(e) => setPlatform(e.target.value)}>
              <option value="PAPER">PAPER</option>
              <option value="VELOCITY">VELOCITY</option>
              <option value="WATERFALL">WATERFALL</option>
            </select>
          )}
          {source === 'spiget' && (
            <span className="store-filter-note">
              付费 / 外链资源可能无法直接下载
            </span>
          )}
        </div>
      </div>

      {resultMeta && <p className="store-result-meta">{resultMeta}</p>}

      {source === 'modrinth' && (
        <ul className="store-hits">
          {mrHits.map((hit) => (
            <li key={hit.project_id} className="store-hit">
              <div className="store-hit-main">
                <StoreIcon src={hit.icon_url} fallback="fa-puzzle-piece" />
                <div className="store-hit-info">
                  <div className="store-hit-title">
                    <strong>{hit.title}</strong>
                    <span className="store-badge">{hit.project_type}</span>
                  </div>
                  <span className="store-hit-meta">
                    {hit.author}
                    <span className="dot" />
                    <i className="fa fa-download" /> {fmtCount(hit.downloads)}
                  </span>
                  <p className="store-desc">{hit.description}</p>
                </div>
                <div className="store-hit-actions">
                  <button
                    type="button"
                    className="btn btn-primary"
                    disabled={busy}
                    onClick={() =>
                      run(async () => {
                        await api.modrinthInstall(instanceId, {
                          project_id: hit.project_id,
                          project_type: hit.project_type,
                          game_version: gameVersion || undefined,
                          loader: loader || undefined,
                          target: projectType === 'plugin' ? 'plugins' : 'mods',
                        })
                        onInstalled()
                      }, `安装 ${hit.title}…`)
                    }
                  >
                    安装最新
                  </button>
                  <button
                    type="button"
                    className="btn btn-ghost"
                    disabled={busy}
                    onClick={() =>
                      run(async () => {
                        if (expanded === hit.project_id) {
                          setExpanded(null)
                          return
                        }
                        const list = await api.modrinthVersions(hit.project_id, {
                          game_version: gameVersion || undefined,
                          loader: loader || undefined,
                        })
                        setMrVersions(list)
                        setPicked(list[0]?.id ?? '')
                        setExpanded(hit.project_id)
                      }, '加载版本…')
                    }
                  >
                    {expanded === hit.project_id ? '收起' : '选版本'}
                  </button>
                </div>
              </div>
              {expanded === hit.project_id && (
                <div className="store-versions">
                  <select value={picked} onChange={(e) => setPicked(e.target.value)}>
                    {mrVersions.map((v) => (
                      <option key={v.id} value={v.id}>
                        {v.version_number} · {v.version_type} · {v.loaders.join(',')}
                      </option>
                    ))}
                  </select>
                  <button
                    type="button"
                    className="btn btn-primary"
                    disabled={busy || !picked}
                    onClick={() =>
                      run(async () => {
                        await api.modrinthInstall(instanceId, {
                          project_id: hit.project_id,
                          version_id: picked,
                          project_type: hit.project_type,
                          target: projectType === 'plugin' ? 'plugins' : 'mods',
                        })
                        onInstalled()
                      }, `安装 ${hit.title}…`)
                    }
                  >
                    安装所选
                  </button>
                </div>
              )}
            </li>
          ))}
          {mrHits.length === 0 && (
            <li className="store-empty">
              <i className="fa fa-search" />
              <span>
                {searched ? '没有匹配结果，换个关键词试试' : '输入关键词搜索 Modrinth'}
              </span>
            </li>
          )}
        </ul>
      )}

      {source === 'hangar' && (
        <ul className="store-hits">
          {hgHits.map((hit) => (
            <li key={hit.slug} className="store-hit">
              <div className="store-hit-main">
                <StoreIcon src={hit.avatar_url} fallback="fa-paper-plane" />
                <div className="store-hit-info">
                  <div className="store-hit-title">
                    <strong>{hit.name}</strong>
                    {hit.category ? (
                      <span className="store-badge">{hit.category}</span>
                    ) : null}
                  </div>
                  <span className="store-hit-meta">
                    {hit.owner}/{hit.slug}
                    <span className="dot" />
                    <i className="fa fa-download" /> {fmtCount(hit.downloads)}
                  </span>
                  <p className="store-desc">{hit.description}</p>
                </div>
                <div className="store-hit-actions">
                  <button
                    type="button"
                    className="btn btn-primary"
                    disabled={busy}
                    onClick={() =>
                      run(async () => {
                        await api.hangarInstall(instanceId, {
                          slug: hit.slug,
                          platform,
                        })
                        onInstalled()
                      }, `安装 ${hit.name}…`)
                    }
                  >
                    安装最新
                  </button>
                  <button
                    type="button"
                    className="btn btn-ghost"
                    disabled={busy}
                    onClick={() =>
                      run(async () => {
                        if (expanded === hit.slug) {
                          setExpanded(null)
                          return
                        }
                        const list = await api.hangarVersions(hit.slug, platform)
                        setHgVersions(list)
                        setPicked(list[0]?.name ?? '')
                        setExpanded(hit.slug)
                      }, '加载 Hangar 版本…')
                    }
                  >
                    {expanded === hit.slug ? '收起' : '选版本'}
                  </button>
                  <a
                    className="store-ext"
                    href={`https://hangar.papermc.io/${hit.owner}/${hit.slug}`}
                    target="_blank"
                    rel="noreferrer"
                  >
                    详情 <i className="fa fa-external-link" />
                  </a>
                </div>
              </div>
              {expanded === hit.slug && (
                <div className="store-versions">
                  <select value={picked} onChange={(e) => setPicked(e.target.value)}>
                    {hgVersions.map((v) => (
                      <option key={v.id} value={v.name}>
                        {v.name} · {(v.size / 1024).toFixed(0)} KiB ·{' '}
                        {v.game_versions.slice(0, 4).join(',')}
                      </option>
                    ))}
                  </select>
                  <button
                    type="button"
                    className="btn btn-primary"
                    disabled={busy || !picked}
                    onClick={() =>
                      run(async () => {
                        await api.hangarInstall(instanceId, {
                          slug: hit.slug,
                          version: picked,
                          platform,
                        })
                        onInstalled()
                      }, `安装 ${hit.name}…`)
                    }
                  >
                    安装所选
                  </button>
                </div>
              )}
            </li>
          ))}
          {hgHits.length === 0 && (
            <li className="store-empty">
              <i className="fa fa-search" />
              <span>
                {searched
                  ? '没有匹配结果，换个关键词试试'
                  : '搜索 Hangar（Paper / Velocity / Waterfall）'}
              </span>
            </li>
          )}
        </ul>
      )}

      {source === 'spiget' && (
        <ul className="store-hits">
          {spHits.map((hit) => (
            <li key={hit.id} className="store-hit">
              <div className="store-hit-main">
                <StoreIcon src={hit.icon_url} fallback="fa-cube" />
                <div className="store-hit-info">
                  <div className="store-hit-title">
                    <strong>{hit.name}</strong>
                    {hit.premium ? (
                      <span className="store-badge warn">付费</span>
                    ) : null}
                    {hit.external ? (
                      <span className="store-badge muted">外链</span>
                    ) : null}
                  </div>
                  <span className="store-hit-meta">
                    #{hit.id}
                    <span className="dot" />
                    <i className="fa fa-download" /> {fmtCount(hit.downloads)}
                    <span className="dot" />
                    {hit.file_type || 'jar'}
                  </span>
                  <p className="store-desc">{hit.tag}</p>
                </div>
                <div className="store-hit-actions">
                  <button
                    type="button"
                    className="btn btn-primary"
                    disabled={busy || hit.premium}
                    title={hit.premium ? '付费资源无法 API 下载' : undefined}
                    onClick={() =>
                      run(async () => {
                        await api.spigetInstall(instanceId, {
                          resource_id: hit.id,
                        })
                        onInstalled()
                      }, `安装 ${hit.name}…`)
                    }
                  >
                    安装
                  </button>
                  <button
                    type="button"
                    className="btn btn-ghost"
                    disabled={busy}
                    onClick={() =>
                      run(async () => {
                        const key = String(hit.id)
                        if (expanded === key) {
                          setExpanded(null)
                          return
                        }
                        const list = await api.spigetVersions(hit.id)
                        setSpVersions(list)
                        setPicked(list[0] ? String(list[0].id) : '')
                        setExpanded(key)
                      }, '加载 Spiget 版本…')
                    }
                  >
                    {expanded === String(hit.id) ? '收起' : '选版本'}
                  </button>
                  <a
                    className="store-ext"
                    href={`https://www.spigotmc.org/resources/${hit.id}/`}
                    target="_blank"
                    rel="noreferrer"
                  >
                    详情 <i className="fa fa-external-link" />
                  </a>
                </div>
              </div>
              {expanded === String(hit.id) && (
                <div className="store-versions">
                  <select value={picked} onChange={(e) => setPicked(e.target.value)}>
                    {spVersions.map((v) => (
                      <option key={v.id} value={v.id}>
                        {v.name} · id {v.id}
                      </option>
                    ))}
                  </select>
                  <button
                    type="button"
                    className="btn btn-primary"
                    disabled={busy || hit.premium || !picked}
                    onClick={() =>
                      run(async () => {
                        await api.spigetInstall(instanceId, {
                          resource_id: hit.id,
                          version_id: Number(picked),
                        })
                        onInstalled()
                      }, `安装 ${hit.name}…`)
                    }
                  >
                    安装所选
                  </button>
                </div>
              )}
            </li>
          ))}
          {spHits.length === 0 && (
            <li className="store-empty">
              <i className="fa fa-search" />
              <span>
                {searched ? '没有匹配结果，换个关键词试试' : '输入关键词搜索 Spiget'}
              </span>
            </li>
          )}
        </ul>
      )}
    </div>
  )
}
