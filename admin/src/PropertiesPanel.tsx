import { useEffect, useMemo, useState } from 'react'
import type { PropertyEntry } from './api'

type FieldKind = 'bool' | 'number' | 'text' | 'select'

type FieldDef = {
  key: string
  label: string
  hint?: string
  kind: FieldKind
  options?: { value: string; label: string }[]
  min?: number
  max?: number
}

type GroupDef = {
  id: string
  title: string
  icon: string
  fields: FieldDef[]
}

const GROUPS: GroupDef[] = [
  {
    id: 'network',
    title: '网络与连接',
    icon: 'fa-globe',
    fields: [
      {
        key: 'server-port',
        label: '端口',
        hint: '玩家连接端口；Docker 下容器内仍为 25565',
        kind: 'number',
        min: 1,
        max: 65535,
      },
      {
        key: 'server-ip',
        label: '绑定 IP',
        hint: '留空表示监听所有网卡',
        kind: 'text',
      },
      {
        key: 'max-players',
        label: '最大玩家数',
        kind: 'number',
        min: 1,
        max: 10000,
      },
      {
        key: 'online-mode',
        label: '正版验证 (online-mode)',
        hint: '关闭后可离线/盗版进入，有安全风险',
        kind: 'bool',
      },
      {
        key: 'prevent-proxy-connections',
        label: '防止代理连接',
        kind: 'bool',
      },
      {
        key: 'network-compression-threshold',
        label: '网络压缩阈值',
        hint: '包大小超过此值才压缩；-1 禁用',
        kind: 'number',
        min: -1,
      },
    ],
  },
  {
    id: 'gameplay',
    title: '游戏玩法',
    icon: 'fa-gamepad',
    fields: [
      {
        key: 'gamemode',
        label: '游戏模式',
        kind: 'select',
        options: [
          { value: 'survival', label: '生存 survival' },
          { value: 'creative', label: '创造 creative' },
          { value: 'adventure', label: '冒险 adventure' },
          { value: 'spectator', label: '旁观 spectator' },
        ],
      },
      {
        key: 'force-gamemode',
        label: '强制游戏模式',
        kind: 'bool',
      },
      {
        key: 'difficulty',
        label: '难度',
        kind: 'select',
        options: [
          { value: 'peaceful', label: '和平 peaceful' },
          { value: 'easy', label: '简单 easy' },
          { value: 'normal', label: '普通 normal' },
          { value: 'hard', label: '困难 hard' },
        ],
      },
      {
        key: 'hardcore',
        label: '极限模式',
        kind: 'bool',
      },
      {
        key: 'pvp',
        label: '允许 PVP',
        kind: 'bool',
      },
      {
        key: 'allow-flight',
        label: '允许飞行',
        kind: 'bool',
      },
      {
        key: 'allow-nether',
        label: '允许下界',
        kind: 'bool',
      },
      {
        key: 'spawn-monsters',
        label: '生成怪物',
        kind: 'bool',
      },
      {
        key: 'spawn-animals',
        label: '生成动物',
        kind: 'bool',
      },
      {
        key: 'spawn-npcs',
        label: '生成村民 NPC',
        kind: 'bool',
      },
    ],
  },
  {
    id: 'world',
    title: '世界与生成',
    icon: 'fa-map',
    fields: [
      {
        key: 'level-name',
        label: '世界名',
        kind: 'text',
      },
      {
        key: 'level-seed',
        label: '种子',
        kind: 'text',
      },
      {
        key: 'level-type',
        label: '世界类型',
        kind: 'select',
        options: [
          { value: 'minecraft\\:normal', label: '普通 (minecraft:normal)' },
          { value: 'minecraft\\:flat', label: '超平坦' },
          { value: 'minecraft\\:large_biomes', label: '大型生物群系' },
          { value: 'minecraft\\:amplified', label: '放大化' },
          { value: 'default', label: 'default' },
          { value: 'flat', label: 'flat' },
          { value: 'largeBiomes', label: 'largeBiomes' },
          { value: 'amplified', label: 'amplified' },
        ],
      },
      {
        key: 'generator-settings',
        label: '生成器设置 JSON',
        kind: 'text',
      },
      {
        key: 'max-world-size',
        label: '最大世界大小',
        kind: 'number',
        min: 1,
      },
      {
        key: 'spawn-protection',
        label: '出生点保护半径',
        kind: 'number',
        min: 0,
      },
      {
        key: 'view-distance',
        label: '视距 (view-distance)',
        kind: 'number',
        min: 2,
        max: 32,
      },
      {
        key: 'simulation-distance',
        label: '模拟距离',
        kind: 'number',
        min: 2,
        max: 32,
      },
      {
        key: 'generate-structures',
        label: '生成建筑结构',
        kind: 'bool',
      },
    ],
  },
  {
    id: 'security',
    title: '安全与权限',
    icon: 'fa-shield',
    fields: [
      {
        key: 'white-list',
        label: '启用白名单',
        kind: 'bool',
      },
      {
        key: 'enforce-whitelist',
        label: '强制白名单',
        kind: 'bool',
      },
      {
        key: 'enable-command-block',
        label: '启用命令方块',
        kind: 'bool',
      },
      {
        key: 'op-permission-level',
        label: 'OP 权限等级',
        kind: 'select',
        options: [
          { value: '1', label: '1' },
          { value: '2', label: '2' },
          { value: '3', label: '3' },
          { value: '4', label: '4' },
        ],
      },
      {
        key: 'function-permission-level',
        label: '函数权限等级',
        kind: 'select',
        options: [
          { value: '1', label: '1' },
          { value: '2', label: '2' },
          { value: '3', label: '3' },
          { value: '4', label: '4' },
        ],
      },
      {
        key: 'enable-status',
        label: '对外显示服务器状态',
        kind: 'bool',
      },
      {
        key: 'hide-online-players',
        label: '隐藏在线玩家列表',
        kind: 'bool',
      },
    ],
  },
  {
    id: 'misc',
    title: '显示与杂项',
    icon: 'fa-sliders',
    fields: [
      {
        key: 'motd',
        label: 'MOTD 标语',
        kind: 'text',
      },
      {
        key: 'max-tick-time',
        label: '看门狗超时 (ms)',
        hint: '-1 禁用；默认 60000',
        kind: 'number',
        min: -1,
      },
      {
        key: 'player-idle-timeout',
        label: '闲置踢出 (分钟)',
        hint: '0 表示不踢出',
        kind: 'number',
        min: 0,
      },
      {
        key: 'enable-query',
        label: '启用 GameSpy Query',
        kind: 'bool',
      },
      {
        key: 'enable-rcon',
        label: '启用 RCON',
        kind: 'bool',
      },
      {
        key: 'rcon.port',
        label: 'RCON 端口',
        kind: 'number',
        min: 1,
        max: 65535,
      },
      {
        key: 'rcon.password',
        label: 'RCON 密码',
        kind: 'text',
      },
      {
        key: 'resource-pack',
        label: '资源包 URL',
        kind: 'text',
      },
      {
        key: 'resource-pack-sha1',
        label: '资源包 SHA1',
        kind: 'text',
      },
      {
        key: 'require-resource-pack',
        label: '强制资源包',
        kind: 'bool',
      },
      {
        key: 'sync-chunk-writes',
        label: '同步写入区块',
        kind: 'bool',
      },
    ],
  },
]

function getVal(entries: PropertyEntry[], key: string): string {
  return entries.find((e) => e.key === key)?.value ?? ''
}

function setVal(
  entries: PropertyEntry[],
  key: string,
  value: string,
): PropertyEntry[] {
  const idx = entries.findIndex((e) => e.key === key)
  if (idx >= 0) {
    const next = [...entries]
    next[idx] = { key, value }
    return next
  }
  return [...entries, { key, value }]
}

function isTruthy(v: string) {
  return v === 'true' || v === '1'
}

type Props = {
  entries: PropertyEntry[]
  onChange: (entries: PropertyEntry[]) => void
  busy: boolean
  running: boolean
  cleanEpoch: number
  onSave: () => void
  onReload: () => void
}

export default function PropertiesPanel({
  entries,
  onChange,
  busy,
  running,
  cleanEpoch,
  onSave,
  onReload,
}: Props) {
  const [groupId, setGroupId] = useState(GROUPS[0].id)
  const [query, setQuery] = useState('')
  const [mode, setMode] = useState<'form' | 'raw'>('form')
  const [newKey, setNewKey] = useState('')
  const [newVal, setNewVal] = useState('')
  const [baseline, setBaseline] = useState(() => JSON.stringify(entries))

  useEffect(() => {
    setBaseline(JSON.stringify(entries))
  }, [cleanEpoch])

  const dirty = JSON.stringify(entries) !== baseline

  const knownKeys = useMemo(
    () => new Set(GROUPS.flatMap((g) => g.fields.map((f) => f.key))),
    [],
  )

  const extraEntries = useMemo(
    () => entries.filter((e) => !knownKeys.has(e.key)),
    [entries, knownKeys],
  )

  const activeGroup = GROUPS.find((g) => g.id === groupId) ?? GROUPS[0]

  const filteredFields = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return activeGroup.fields
    return GROUPS.flatMap((g) =>
      g.fields
        .filter(
          (f) =>
            f.key.toLowerCase().includes(q) ||
            f.label.toLowerCase().includes(q) ||
            (f.hint ?? '').toLowerCase().includes(q),
        )
        .map((f) => ({ ...f, _group: g.title })),
    ) as (FieldDef & { _group?: string })[]
  }, [activeGroup, query])

  const searching = query.trim().length > 0

  const renderField = (f: FieldDef, groupTag?: string) => {
    const value = getVal(entries, f.key)
    const present = entries.some((e) => e.key === f.key)

    if (f.kind === 'bool') {
      return (
        <div className="prop-field" key={f.key}>
          {groupTag && <p className="prop-group-tag">{groupTag}</p>}
          <div className="prop-field-head">
            <div>
              <div className="prop-label">{f.label}</div>
              <div className="prop-key">{f.key}</div>
              {f.hint && <p className="prop-hint">{f.hint}</p>}
            </div>
            <label className="prop-switch">
              <input
                type="checkbox"
                checked={isTruthy(value)}
                disabled={busy}
                onChange={(e) =>
                  onChange(setVal(entries, f.key, e.target.checked ? 'true' : 'false'))
                }
              />
              <span>{isTruthy(value) ? '开' : '关'}</span>
            </label>
          </div>
          {!present && (
            <p className="prop-missing">文件中尚无此项，保存时会写入</p>
          )}
        </div>
      )
    }

    if (f.kind === 'select' && f.options) {
      const opts = f.options
      const has = opts.some((o) => o.value === value)
      return (
        <div className="prop-field" key={f.key}>
          {groupTag && <p className="prop-group-tag">{groupTag}</p>}
          <label className="prop-label" htmlFor={`prop-${f.key}`}>
            {f.label}
          </label>
          <div className="prop-key">{f.key}</div>
          {f.hint && <p className="prop-hint">{f.hint}</p>}
          <select
            id={`prop-${f.key}`}
            value={has ? value : value || opts[0].value}
            disabled={busy}
            onChange={(e) => onChange(setVal(entries, f.key, e.target.value))}
          >
            {!has && value && (
              <option value={value}>当前：{value}</option>
            )}
            {opts.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        </div>
      )
    }

    return (
      <div className="prop-field" key={f.key}>
        {groupTag && <p className="prop-group-tag">{groupTag}</p>}
        <label className="prop-label" htmlFor={`prop-${f.key}`}>
          {f.label}
        </label>
        <div className="prop-key">{f.key}</div>
        {f.hint && <p className="prop-hint">{f.hint}</p>}
        <input
          id={`prop-${f.key}`}
          type={f.kind === 'number' ? 'number' : 'text'}
          min={f.min}
          max={f.max}
          value={value}
          disabled={busy}
          placeholder={present ? undefined : '（未设置）'}
          onChange={(e) => onChange(setVal(entries, f.key, e.target.value))}
        />
      </div>
    )
  }

  const rawText = entries.map((e) => `${e.key}=${e.value}`).join('\n')

  return (
    <div className="props-layout">
      <div className="props-toolbar">
        <div className="props-modes">
          <button
            type="button"
            className={mode === 'form' ? 'btn btn-primary' : 'btn btn-ghost'}
            onClick={() => setMode('form')}
          >
            <i className="fa fa-th-list" /> 分组编辑
          </button>
          <button
            type="button"
            className={mode === 'raw' ? 'btn btn-primary' : 'btn btn-ghost'}
            onClick={() => setMode('raw')}
          >
            <i className="fa fa-code" /> 全部键值
          </button>
        </div>
        <input
          className="form-input props-search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="搜索配置项（名称 / key）"
          disabled={mode === 'raw'}
        />
        <div className="props-actions">
          {dirty && <span className="props-dirty">未保存</span>}
          {running && (
            <span className="meta">运行中修改需重启后生效</span>
          )}
          <button
            type="button"
            className="btn btn-ghost"
            disabled={busy}
            onClick={onReload}
          >
            <i className="fa fa-refresh" /> 重新加载
          </button>
          <button
            type="button"
            className="btn btn-primary"
            disabled={busy || !dirty}
            onClick={onSave}
          >
            <i className="fa fa-save" /> 保存
          </button>
        </div>
      </div>

      {mode === 'form' ? (
        <div className="props-body">
          {!searching && (
            <nav className="props-nav">
              {GROUPS.map((g) => (
                <button
                  key={g.id}
                  type="button"
                  className={
                    g.id === groupId ? 'props-nav-item active' : 'props-nav-item'
                  }
                  onClick={() => setGroupId(g.id)}
                >
                  <i className={`fa ${g.icon}`} />
                  {g.title}
                </button>
              ))}
              <button
                type="button"
                className={
                  groupId === 'extra' ? 'props-nav-item active' : 'props-nav-item'
                }
                onClick={() => setGroupId('extra')}
              >
                <i className="fa fa-ellipsis-h" /> 其他项 ({extraEntries.length})
              </button>
            </nav>
          )}

          <div className="props-fields card-panel">
            {searching ? (
              <>
                <h3 className="card-title">搜索结果</h3>
                {filteredFields.length === 0 ? (
                  <p className="empty">无匹配项</p>
                ) : (
                  filteredFields.map((f) =>
                    renderField(f, (f as FieldDef & { _group?: string })._group),
                  )
                )}
              </>
            ) : groupId === 'extra' ? (
              <>
                <h3 className="card-title">其他 / 自定义键</h3>
                <p className="meta mb-1">
                  未归入常用分组的配置项，可在此编辑或新增。
                </p>
                {extraEntries.map((e) => (
                  <div className="prop-field" key={e.key}>
                    <label className="prop-label">{e.key}</label>
                    <div className="prop-row">
                      <input
                        value={e.value}
                        disabled={busy}
                        onChange={(ev) =>
                          onChange(setVal(entries, e.key, ev.target.value))
                        }
                      />
                      <button
                        type="button"
                        className="link-btn danger"
                        disabled={busy}
                        onClick={() =>
                          onChange(entries.filter((x) => x.key !== e.key))
                        }
                      >
                        删除
                      </button>
                    </div>
                  </div>
                ))}
                <div className="prop-add">
                  <input
                    placeholder="新键名"
                    value={newKey}
                    onChange={(e) => setNewKey(e.target.value)}
                  />
                  <input
                    placeholder="值"
                    value={newVal}
                    onChange={(e) => setNewVal(e.target.value)}
                  />
                  <button
                    type="button"
                    className="btn btn-ghost"
                    disabled={busy || !newKey.trim()}
                    onClick={() => {
                      const k = newKey.trim()
                      if (!k) return
                      onChange(setVal(entries, k, newVal))
                      setNewKey('')
                      setNewVal('')
                    }}
                  >
                    添加
                  </button>
                </div>
              </>
            ) : (
              <>
                <h3 className="card-title">
                  <i className={`fa ${activeGroup.icon}`} /> {activeGroup.title}
                </h3>
                <div className="prop-grid">
                  {activeGroup.fields.map((f) => renderField(f))}
                </div>
              </>
            )}
          </div>
        </div>
      ) : (
        <div className="card-panel props-raw">
          <p className="meta mb-1">
            逐行编辑 key=value。保存时按当前列表写回（不会保留注释行）。
          </p>
          <div className="prop-raw-list">
            {entries.map((e, idx) => (
              <div className="prop-raw-row" key={`${e.key}-${idx}`}>
                <input
                  className="prop-raw-key"
                  value={e.key}
                  disabled={busy}
                  onChange={(ev) => {
                    const next = [...entries]
                    next[idx] = { ...e, key: ev.target.value }
                    onChange(next)
                  }}
                />
                <span className="prop-eq">=</span>
                <input
                  className="prop-raw-val"
                  value={e.value}
                  disabled={busy}
                  onChange={(ev) => {
                    const next = [...entries]
                    next[idx] = { ...e, value: ev.target.value }
                    onChange(next)
                  }}
                />
                <button
                  type="button"
                  className="link-btn danger"
                  disabled={busy}
                  onClick={() =>
                    onChange(entries.filter((_, i) => i !== idx))
                  }
                >
                  删
                </button>
              </div>
            ))}
          </div>
          <div className="prop-add">
            <input
              placeholder="新键名"
              value={newKey}
              onChange={(e) => setNewKey(e.target.value)}
            />
            <input
              placeholder="值"
              value={newVal}
              onChange={(e) => setNewVal(e.target.value)}
            />
            <button
              type="button"
              className="btn btn-ghost"
              disabled={busy || !newKey.trim()}
              onClick={() => {
                const k = newKey.trim()
                if (!k) return
                onChange(setVal(entries, k, newVal))
                setNewKey('')
                setNewVal('')
              }}
            >
              添加一行
            </button>
          </div>
          <details className="prop-preview">
            <summary>预览文本</summary>
            <pre>{rawText || '（空）'}</pre>
          </details>
        </div>
      )}
    </div>
  )
}
