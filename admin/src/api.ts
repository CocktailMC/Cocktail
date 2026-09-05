export type InstanceStatus =
  | 'created'
  | 'starting'
  | 'running'
  | 'stopping'
  | 'stopped'
  | 'crashed'

export type InstanceSpec = {
  name: string
  workdir: string
  command?: string | null
  args: string[]
  memory_mib: number
  core: string
  port: number
  auto_restart: boolean
  eula_accepted: boolean
  webhook_url?: string | null
  runtime?: 'process' | 'docker'
  docker_image?: string | null
  cpu_limit?: number | null
  tags?: string[]
  group?: string | null
}

export type MetricSample = {
  ts: string
  cpu_pct: number
  memory_mib: number
  tps: number | null
  players: number
}

export type Instance = {
  id: string
  spec: InstanceSpec
  status: InstanceStatus
  created_at: string
  updated_at: string
  last_metrics: MetricSample | null
  last_players?: string[]
}

export type LogLine = {
  ts: string
  stream: string
  line: string
}

export type CreateInstanceBody = {
  name: string
  core?: string
  memory_mib?: number
  port?: number
  auto_restart?: boolean
  eula_accepted?: boolean
  command?: string
  args?: string[]
  workdir?: string
  runtime?: 'process' | 'docker'
  docker_image?: string
  cpu_limit?: number
  tags?: string[]
  group?: string
}

export type UpdateInstanceBody = {
  name?: string
  memory_mib?: number
  port?: number
  auto_restart?: boolean
  eula_accepted?: boolean
  command?: string
  args?: string[]
  core?: string
  webhook_url?: string
  runtime?: 'process' | 'docker'
  docker_image?: string
  cpu_limit?: number
  tags?: string[]
  group?: string
}

export type FileEntry = {
  name: string
  path: string
  is_dir: boolean
  size: number
}

export type FileContent = {
  path: string
  content: string
}

export type BackupInfo = {
  id: string
  created_at: string
  path: string
  size_bytes: number
}

export type PropertyEntry = { key: string; value: string }

export type PluginInfo = {
  name: string
  path: string
  size: number
  enabled: boolean
}

export type Schedule = {
  id: string
  instance_id: string
  kind: 'backup' | 'restart' | 'command'
  every_secs: number
  command?: string | null
  enabled: boolean
  next_run_at: string
}

export type CoreVersion = {
  id: string
  core: string
  latest: boolean
}

export type ModrinthHit = {
  project_id: string
  slug: string
  title: string
  description: string
  author: string
  project_type: string
  downloads: number
  icon_url?: string | null
  categories: string[]
  versions: string[]
}

export type ModrinthVersion = {
  id: string
  name: string
  version_number: string
  version_type: string
  game_versions: string[]
  loaders: string[]
  date_published: string
  downloads: number
  primary_filename: string
  primary_url: string
  primary_size: number
}

export type HangarHit = {
  id: number
  slug: string
  owner: string
  name: string
  description: string
  downloads: number
  avatar_url?: string | null
  category: string
  platforms: string[]
}

export type HangarVersion = {
  id: number
  name: string
  platform: string
  downloads: number
  filename: string
  download_url: string
  size: number
  game_versions: string[]
}

export type SpigetHit = {
  id: number
  name: string
  tag: string
  downloads: number
  external: boolean
  premium: boolean
  file_type: string
  author: string
  icon_url?: string | null
  tested_versions: string[]
}

export type SpigetVersion = {
  id: number
  name: string
  downloads: number
  release_date: number
}

export type PlayerInfo = { name: string }

export type WorldInfo = {
  name: string
  path: string
  size_bytes: number
}

const TOKEN_KEY = 'cocktail_api_token'

export function getToken(): string {
  return localStorage.getItem(TOKEN_KEY) ?? ''
}

export function setToken(token: string) {
  if (token) localStorage.setItem(TOKEN_KEY, token)
  else localStorage.removeItem(TOKEN_KEY)
}

function authHeaders(extra?: HeadersInit): HeadersInit {
  const token = getToken()
  const headers: Record<string, string> = {
    ...(extra as Record<string, string>),
  }
  if (token) headers.Authorization = `Bearer ${token}`
  return headers
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    ...init,
    headers: authHeaders(init?.headers),
  })
  if (!res.ok) {
    let message = res.statusText
    try {
      const body = (await res.json()) as { error?: string }
      if (body.error) message = body.error
    } catch {
      if (res.status === 401) message = '未授权：请登录'
    }
    throw new Error(message)
  }
  if (res.status === 204) return undefined as T
  return res.json() as Promise<T>
}

export type AuthSession = {
  token: string
  username: string
  panel_name: string
  role: string
}

export type MeInfo = {
  username: string
  role: string
  panel_name: string
  created_at: string
}

export type PanelSettings = {
  panel_name: string
  webhook_url?: string | null
  env_webhook_set: boolean
  env_api_token_set: boolean
  admin_username: string
  admin_created_at: string
  bind: string
  db_path: string
}

export type HealthInfo = {
  name: string
  version: string
  release: string
  status: string
  auth_required: boolean
  setup_required?: boolean
  panel_name?: string
  admin_username?: string | null
  os?: string
  arch?: string
  family?: string
  hostname?: string
  distro_id?: string
  distro_name?: string
  distro_version?: string
  kernel?: string
  wsl?: boolean
}

export const api = {
  health: () => request<HealthInfo>('/api/v1/health'),
  setup: (body: { username: string; password: string; panel_name?: string }) =>
    request<AuthSession>('/api/v1/setup', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }),
  login: (body: { username: string; password: string }) =>
    request<AuthSession>('/api/v1/auth/login', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }),
  logout: () =>
    request<void>('/api/v1/auth/logout', { method: 'POST' }).catch(() => undefined),
  me: () => request<MeInfo>('/api/v1/auth/me'),
  getSettings: () => request<PanelSettings>('/api/v1/settings'),
  updateSettings: (body: {
    panel_name?: string
    webhook_url?: string
    username?: string
  }) =>
    request<PanelSettings>('/api/v1/settings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }),
  changePassword: (body: { current_password: string; new_password: string }) =>
    request<void>('/api/v1/auth/password', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }),
  listInstances: () => request<Instance[]>('/api/v1/instances'),
  createInstance: (body: CreateInstanceBody) =>
    request<Instance>('/api/v1/instances', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }),
  updateInstance: (id: string, body: UpdateInstanceBody) =>
    request<Instance>(`/api/v1/instances/${id}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }),
  acceptEula: (id: string, accepted = true) =>
    request<Instance>(`/api/v1/instances/${id}/eula`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ accepted }),
    }),
  startInstance: (id: string) =>
    request<Instance>(`/api/v1/instances/${id}/start`, { method: 'POST' }),
  stopInstance: (id: string) =>
    request<Instance>(`/api/v1/instances/${id}/stop`, { method: 'POST' }),
  restartInstance: (id: string) =>
    request<Instance>(`/api/v1/instances/${id}/restart`, { method: 'POST' }),
  deleteInstance: (id: string) =>
    request<void>(`/api/v1/instances/${id}`, { method: 'DELETE' }),
  sendCommand: (id: string, command: string) =>
    request<void>(`/api/v1/instances/${id}/command`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ command }),
    }),
  listFiles: (id: string, path = '') =>
    request<FileEntry[]>(
      `/api/v1/instances/${id}/files?path=${encodeURIComponent(path)}`,
    ),
  readFile: (id: string, path: string) =>
    request<FileContent>(
      `/api/v1/instances/${id}/files/content?path=${encodeURIComponent(path)}`,
    ),
  writeFile: (id: string, path: string, content: string) =>
    request<FileContent>(`/api/v1/instances/${id}/files/content`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path, content }),
    }),
  deleteFile: (id: string, path: string) =>
    request<void>(
      `/api/v1/instances/${id}/files/content?path=${encodeURIComponent(path)}`,
      { method: 'DELETE' },
    ),
  uploadFile: async (id: string, path: string, file: File) => {
    const fd = new FormData()
    fd.append('file', file)
    const res = await fetch(
      `/api/v1/instances/${id}/files/upload?path=${encodeURIComponent(path)}`,
      { method: 'POST', headers: authHeaders(), body: fd },
    )
    if (!res.ok) throw new Error(await res.text())
    return res.json() as Promise<FileEntry>
  },
  mkdir: (id: string, path: string) =>
    request<FileEntry>(`/api/v1/instances/${id}/files/mkdir`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path }),
    }),
  installJar: async (
    id: string,
    file: File,
    opts?: { path?: string; core?: string; accept_eula?: boolean },
  ) => {
    const fd = new FormData()
    fd.append('file', file)
    const path = opts?.path ?? 'server.jar'
    const core = opts?.core ?? 'custom'
    const accept = opts?.accept_eula ?? true
    const qs = new URLSearchParams({
      path,
      core,
      accept_eula: String(accept),
    })
    const res = await fetch(`/api/v1/instances/${id}/install-jar?${qs}`, {
      method: 'POST',
      headers: authHeaders(),
      body: fd,
    })
    if (!res.ok) throw new Error(await res.text())
    return res.json() as Promise<Instance>
  },
  setStartupJar: (id: string, path: string) =>
    request<Instance>(`/api/v1/instances/${id}/startup-jar`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path }),
    }),
  downloadUrl: (id: string, path: string) => {
    const token = getToken()
    const q = token ? `&token=${encodeURIComponent(token)}` : ''
    return `/api/v1/instances/${id}/files/download?path=${encodeURIComponent(path)}${q}`
  },
  listBackups: (id: string) =>
    request<BackupInfo[]>(`/api/v1/instances/${id}/backups`),
  createBackup: (id: string) =>
    request<BackupInfo>(`/api/v1/instances/${id}/backups`, { method: 'POST' }),
  deleteBackup: (id: string, backupId: string) =>
    request<void>(`/api/v1/instances/${id}/backups/${backupId}`, {
      method: 'DELETE',
    }),
  restoreBackup: (id: string, backupId: string) =>
    request<void>(`/api/v1/instances/${id}/backups/${backupId}/restore`, {
      method: 'POST',
    }),
  getProperties: (id: string) =>
    request<PropertyEntry[]>(`/api/v1/instances/${id}/properties`),
  setProperties: (id: string, entries: PropertyEntry[]) =>
    request<PropertyEntry[]>(`/api/v1/instances/${id}/properties`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ entries }),
    }),
  listPlugins: (id: string) =>
    request<PluginInfo[]>(`/api/v1/instances/${id}/plugins`),
  enablePlugin: (id: string, name: string) =>
    request<PluginInfo>(
      `/api/v1/instances/${id}/plugins/${encodeURIComponent(name)}/enable`,
      { method: 'POST' },
    ),
  disablePlugin: (id: string, name: string) =>
    request<PluginInfo>(
      `/api/v1/instances/${id}/plugins/${encodeURIComponent(name)}/disable`,
      { method: 'POST' },
    ),
  listSchedules: () => request<Schedule[]>('/api/v1/schedules'),
  createSchedule: (body: {
    instance_id: string
    kind: 'backup' | 'restart' | 'command'
    every_secs: number
    command?: string
  }) =>
    request<Schedule>('/api/v1/schedules', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }),
  deleteSchedule: (id: string) =>
    request<void>(`/api/v1/schedules/${id}`, { method: 'DELETE' }),
  fleetSummary: () =>
    request<{
      total: number
      running: number
      stopped: number
      starting: number
      crashed: number
      by_group: { group: string; count: number }[]
      by_runtime: { runtime: string; count: number }[]
      docker: { available: boolean; version?: string; message: string }
    }>('/api/v1/fleet/summary'),
  fleetBulk: (action: string, ids: string[]) =>
    request<{ ok: string[]; failed: { id: string; error: string }[] }>(
      '/api/v1/fleet/bulk',
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ action, ids }),
      },
    ),
  dockerStatus: () =>
    request<{ available: boolean; version?: string; message: string }>(
      '/api/v1/docker/status',
    ),
  listCoreVersions: (core: string) =>
    request<CoreVersion[]>(`/api/v1/cores/${core}/versions`),
  installCore: (id: string, core: string, version: string) =>
    request<Instance>(`/api/v1/instances/${id}/install`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ core, version }),
    }),
  listPlayers: (id: string, opts?: { probe?: boolean }) => {
    const q = opts?.probe ? '?probe=true' : ''
    return request<PlayerInfo[]>(`/api/v1/instances/${id}/players${q}`)
  },
  playerAction: (
    id: string,
    name: string,
    action: 'kick' | 'ban' | 'pardon' | 'op' | 'deop',
    reason?: string,
  ) =>
    request<void>(
      `/api/v1/instances/${id}/players/${encodeURIComponent(name)}/${action}`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ reason }),
      },
    ),
  listWorlds: (id: string) =>
    request<WorldInfo[]>(`/api/v1/instances/${id}/worlds`),
  resetWorld: (id: string, world: string) =>
    request<void>(
      `/api/v1/instances/${id}/worlds/${encodeURIComponent(world)}/reset`,
      { method: 'POST' },
    ),
  exportWorld: (id: string, world: string) =>
    request<BackupInfo>(
      `/api/v1/instances/${id}/worlds/${encodeURIComponent(world)}/export`,
      { method: 'POST' },
    ),
  importWorld: async (id: string, world: string, file: File) => {
    const fd = new FormData()
    fd.append('file', file)
    const res = await fetch(
      `/api/v1/instances/${id}/worlds/${encodeURIComponent(world)}/import`,
      { method: 'POST', headers: authHeaders(), body: fd },
    )
    if (!res.ok) throw new Error(await res.text())
  },
  modrinthSearch: (params: {
    query?: string
    project_type?: string
    game_version?: string
    loader?: string
    limit?: number
    offset?: number
  }) => {
    const q = new URLSearchParams()
    if (params.query) q.set('query', params.query)
    if (params.project_type) q.set('project_type', params.project_type)
    if (params.game_version) q.set('game_version', params.game_version)
    if (params.loader) q.set('loader', params.loader)
    if (params.limit != null) q.set('limit', String(params.limit))
    if (params.offset != null) q.set('offset', String(params.offset))
    return request<{
      hits: ModrinthHit[]
      offset: number
      limit: number
      total_hits: number
    }>(`/api/v1/modrinth/search?${q}`)
  },
  modrinthVersions: (
    idOrSlug: string,
    params?: { game_version?: string; loader?: string },
  ) => {
    const q = new URLSearchParams()
    if (params?.game_version) q.set('game_version', params.game_version)
    if (params?.loader) q.set('loader', params.loader)
    const qs = q.toString()
    return request<ModrinthVersion[]>(
      `/api/v1/modrinth/projects/${encodeURIComponent(idOrSlug)}/versions${qs ? `?${qs}` : ''}`,
    )
  },
  modrinthInstall: (
    id: string,
    body: {
      project_id: string
      version_id?: string
      target?: string
      game_version?: string
      loader?: string
      project_type?: string
    },
  ) =>
    request<{
      path: string
      filename: string
      size: number
      project_id: string
      version_id: string
      version_number: string
      target: string
    }>(`/api/v1/instances/${id}/modrinth/install`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }),
  hangarSearch: (params: {
    query?: string
    platform?: string
    limit?: number
    offset?: number
  }) => {
    const q = new URLSearchParams()
    if (params.query) q.set('query', params.query)
    if (params.platform) q.set('platform', params.platform)
    if (params.limit != null) q.set('limit', String(params.limit))
    if (params.offset != null) q.set('offset', String(params.offset))
    return request<{
      hits: HangarHit[]
      total_hits: number
      limit: number
      offset: number
    }>(`/api/v1/hangar/search?${q}`)
  },
  hangarVersions: (slug: string, platform = 'PAPER') => {
    const q = new URLSearchParams({ platform, limit: '25' })
    return request<HangarVersion[]>(
      `/api/v1/hangar/projects/${encodeURIComponent(slug)}/versions?${q}`,
    )
  },
  hangarInstall: (
    id: string,
    body: { slug: string; version?: string; platform?: string },
  ) =>
    request<{
      path: string
      filename: string
      size: number
      slug: string
      version: string
      platform: string
    }>(`/api/v1/instances/${id}/hangar/install`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }),
  spigetSearch: (params: { query?: string; size?: number; page?: number }) => {
    const q = new URLSearchParams()
    if (params.query) q.set('query', params.query)
    if (params.size != null) q.set('size', String(params.size))
    if (params.page != null) q.set('page', String(params.page))
    return request<{ hits: SpigetHit[] }>(`/api/v1/spiget/search?${q}`)
  },
  spigetVersions: (resourceId: number) =>
    request<SpigetVersion[]>(`/api/v1/spiget/resources/${resourceId}/versions`),
  spigetInstall: (
    id: string,
    body: { resource_id: number; version_id?: number },
  ) =>
    request<{
      path: string
      filename: string
      size: number
      resource_id: number
      version_id?: number | null
      version_name: string
    }>(`/api/v1/instances/${id}/spiget/install`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }),
}

export function eventsWsUrl(): string {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws'
  const token = getToken()
  const q = token ? `?token=${encodeURIComponent(token)}` : ''
  return `${proto}://${location.host}/api/v1/events/ws${q}`
}

export function logsWsUrl(id: string): string {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws'
  const token = getToken()
  const q = token ? `?token=${encodeURIComponent(token)}` : ''
  return `${proto}://${location.host}/api/v1/instances/${id}/logs/ws${q}`
}
