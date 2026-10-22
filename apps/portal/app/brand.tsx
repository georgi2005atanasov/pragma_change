/** Local copy of the Pragma Change site's brand artwork; no sibling-project dependency. */
export function Brand({ compact = false }: { compact?: boolean }) {
  return (
    <a className="brand" href="/" aria-label="PragmaChange home">
      <svg
        className="brand-mark"
        viewBox="0 0 42 42"
        fill="none"
        aria-hidden="true"
      >
        <path
          d="M8 7.5h16.1c7.5 0 12.4 4.5 12.4 11.2S31.6 30 24.1 30H8V7.5Z"
          stroke="currentColor"
          strokeWidth="2.6"
        />
        <path
          d="M8 16h16.4c2.9 0 4.8 1.1 5.8 2.7M8 23.5h16.4c2.9 0 4.8-1.1 5.8-2.7M8 30v5"
          stroke="currentColor"
          strokeWidth="2.6"
          strokeLinecap="round"
        />
        <circle cx="8" cy="7.5" r="2.5" fill="currentColor" />
        <circle cx="8" cy="16" r="2.5" fill="currentColor" />
        <circle cx="8" cy="23.5" r="2.5" fill="currentColor" />
      </svg>
      {!compact && (
        <span className="nav-label">
          Pragma<span className="brand-light">Change</span>
        </span>
      )}
    </a>
  );
}
export function WorkspaceLoading() {
  return (
    <main className="workspace-loading">
      <Brand />
      <p role="status">Opening your workspace…</p>
    </main>
  );
}
