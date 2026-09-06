export type CoreGroup = {
  label: string
  items: { id: string; label: string }[]
}

/** 可在线安装的核心（与控制面 versions.rs 对齐）。 */
export const INSTALLABLE_CORE_GROUPS: CoreGroup[] = [
  {
    label: '插件端',
    items: [
      { id: 'paper', label: 'Paper' },
      { id: 'folia', label: 'Folia' },
      { id: 'purpur', label: 'Purpur' },
      { id: 'leaves', label: 'Leaves' },
    ],
  },
  {
    label: '原版 / 模组',
    items: [
      { id: 'vanilla', label: 'Vanilla' },
      { id: 'fabric', label: 'Fabric' },
      { id: 'quilt', label: 'Quilt' },
      { id: 'forge', label: 'Forge' },
      { id: 'neoforge', label: 'NeoForge' },
    ],
  },
  {
    label: '混合端',
    items: [
      { id: 'mohist', label: 'Mohist（Forge + Bukkit）' },
      { id: 'banner', label: 'Banner（Fabric + Bukkit）' },
      { id: 'arclight', label: 'Arclight（Forge/NeoForge + Bukkit）' },
    ],
  },
]

export const INSTALLABLE_CORE_IDS = INSTALLABLE_CORE_GROUPS.flatMap((g) =>
  g.items.map((i) => i.id),
)

export function recommendedJavaMajor(mc?: string): number {
  if (!mc) return 21
  const nums = mc
    .split(/[^\d]+/)
    .filter(Boolean)
    .map((s) => Number(s))
    .filter((n) => Number.isFinite(n))
  const minor =
    nums[0] === 1 ? (nums[1] ?? 0) : (nums[0] ?? 0)
  const patch =
    nums[0] === 1 ? (nums[2] ?? 0) : (nums[1] ?? 0)
  if (minor >= 21 || (minor === 20 && patch >= 5)) return 21
  if (minor >= 17) return 17
  return 8
}

export const JAVA_MAJORS = [8, 11, 17, 21, 25] as const

export function isInstallableCore(core: string): boolean {
  return INSTALLABLE_CORE_IDS.includes(core)
}

export function coreHasLoaders(core: string): boolean {
  return ['fabric', 'quilt', 'forge', 'neoforge', 'arclight'].includes(core)
}

export function defaultModrinthLoader(core: string): string {
  switch (core) {
    case 'fabric':
    case 'quilt':
    case 'banner':
      return 'fabric'
    case 'forge':
    case 'mohist':
      return 'forge'
    case 'neoforge':
    case 'arclight':
      return 'neoforge'
    case 'folia':
      return 'folia'
    case 'purpur':
    case 'leaves':
      return 'purpur'
    default:
      return 'paper'
  }
}

export function defaultModrinthProjectType(
  core: string,
): 'plugin' | 'mod' {
  return ['fabric', 'quilt', 'forge', 'neoforge'].includes(core)
    ? 'mod'
    : 'plugin'
}
