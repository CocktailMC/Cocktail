type Props = {
  active: boolean
  label?: string
  /** Non-blocking status strip (e.g. starting/stopping) */
  statusHint?: string | null
}

/** Top indeterminate bar + optional blocking wait panel. */
export default function BusyOverlay({ active, label, statusHint }: Props) {
  const showBar = active || Boolean(statusHint)
  if (!showBar && !active) return null

  return (
    <>
      {showBar && (
        <div
          className={`load-bar ${active ? 'active' : 'hint'}`}
          role="progressbar"
          aria-busy={active}
          aria-label={label || statusHint || '加载中'}
        >
          <div className="load-bar-indeterminate" />
        </div>
      )}
      {active && (
        <div className="busy-overlay" role="alertdialog" aria-modal="true">
          <div className="busy-card">
            <div className="busy-spinner" aria-hidden />
            <p className="busy-title">{label || '处理中…'}</p>
            <p className="busy-sub">请稍候，完成后会自动刷新</p>
          </div>
        </div>
      )}
      {!active && statusHint && (
        <div className="status-toast">{statusHint}</div>
      )}
    </>
  )
}
