"use client";
import { Suspense, useEffect, useRef, useState } from "react";
import { WorkspaceProvider, useWorkspace } from "./workspace";
import { sections, captions, signOut, Empty } from "./ui";
import OverviewSection from "./sections/overview";
import EndpointsSection from "./sections/endpoints";
import PipelinesSection from "./sections/pipelines";
import DatasetsSection from "./sections/datasets";
import TrainingSection from "./sections/training";
import ReplaySection from "./sections/replay";
import ReleasesSection from "./sections/releases";
import BehaviorSection from "./sections/behavior";
import OnboardingSection from "./sections/onboarding";
import ServersSection from "./sections/servers";
import AuditSection from "./sections/audit";
import { Brand, WorkspaceLoading } from "./brand";
export default function Portal() {
  return (
    <Suspense fallback={<WorkspaceLoading />}>
      <WorkspaceProvider>
        <PortalShell />
      </WorkspaceProvider>
    </Suspense>
  );
}
function PortalShell() {
  const {
    section,
    setSection,
    setError,
    setNotice,
    counts,
    run,
    refresh,
    busy,
    error,
    notice,
    loaded,
  } = useWorkspace();
  const [collapsed, setCollapsed] = useState(false);
  const [mobileOpen, setMobileOpen] = useState(false);
  const menu = useRef<HTMLButtonElement>(null);
  const sidebar = useRef<HTMLElement>(null);
  useEffect(() => {
    try {
      setCollapsed(localStorage.getItem("pragma.sidebar.collapsed") === "true");
    } catch {}
  }, []);
  function toggleSidebar() {
    const next = !collapsed;
    document.documentElement.dataset.sidebarCollapsed = String(next);
    setCollapsed(next);
    try {
      localStorage.setItem("pragma.sidebar.collapsed", String(next));
    } catch {}
  }
  function closeMenu() {
    setMobileOpen(false);
    requestAnimationFrame(() => menu.current?.focus());
  }
  useEffect(() => {
    if (!mobileOpen) return;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    sidebar.current?.querySelector<HTMLButtonElement>("button")?.focus();
    function keyboard(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.preventDefault();
        closeMenu();
      }
      if (e.key === "Tab") {
        const controls = [
          ...(sidebar.current?.querySelectorAll<HTMLElement>(
            "a[href],button:not(:disabled)",
          ) || []),
        ].filter((el) => el.getClientRects().length);
        const first = controls[0],
          last = controls[controls.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last?.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first?.focus();
        }
      }
    }
    function resize() {
      if (window.innerWidth > 800) setMobileOpen(false);
    }
    document.addEventListener("keydown", keyboard);
    window.addEventListener("resize", resize);
    return () => {
      document.body.style.overflow = previousOverflow;
      document.removeEventListener("keydown", keyboard);
      window.removeEventListener("resize", resize);
    };
  }, [mobileOpen]);
  return (
    <div
      className={`shell ${collapsed ? "nav-collapsed" : ""} ${mobileOpen ? "nav-open" : ""}`}
    >
      {mobileOpen && (
        <button
          className="nav-backdrop"
          aria-label="Close navigation"
          onClick={closeMenu}
        />
      )}

      <aside
        ref={sidebar}
        id="workspace-navigation"
        className="sidebar"
        aria-label="Workspace navigation"
        role={mobileOpen ? "dialog" : undefined}
        aria-modal={mobileOpen || undefined}
      >
        <div className="sidebar-header">
          <Brand />
          <button
            className="collapse-toggle secondary"
            aria-label="Collapse sidebar"
            title="Collapse sidebar"
            aria-expanded={!collapsed}
            aria-controls="workspace-navigation"
            onClick={toggleSidebar}
          >
            <span aria-hidden="true">‹</span>
          </button>
          <button
            className="mobile-nav-close secondary"
            onClick={closeMenu}
            aria-label="Close navigation"
          >
            <span aria-hidden="true">‹</span>
          </button>
        </div>
        <div className="workspace">
          <span className="workspace-dot" />
          <div>
            Self-hosted workspace<small>Administrator</small>
          </div>
        </div>
        <div className="nav-caption">WORKSPACE</div>
        <nav>
          {sections.map((s, i) => (
            <button
              aria-label={s}
              title={s}
              aria-current={section === i ? "page" : undefined}
              className={section === i ? "active" : ""}
              onClick={() => {
                setSection(i);
                if (mobileOpen) closeMenu();
                setError("");
                setNotice("");
              }}
              key={s}
            >
              <span className="nav-icon">
                {["◫", "⌘", "⋈", "▤", "◉", "▷", "↗", "≡", "▣", "⇢", "✧"][i]}
              </span>
              <span className="nav-label">{s}</span>
              {i === 4 && counts.pending_jobs > 0 && (
                <span className="nav-count">{counts.pending_jobs}</span>
              )}
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <span className="status-dot" /> Inference runs at your proxy
          <p>The control plane manages configuration and model releases.</p>
          <button className="text-button" onClick={() => run(signOut)}>
            Sign out ↗
          </button>
        </div>
      </aside>
      <main className="main" inert={mobileOpen}>
        <header className="topbar">
          <div className="topbar-leading">
            <button
              className="expand-toggle secondary"
              aria-label="Expand sidebar"
              title="Expand sidebar"
              aria-expanded={false}
              aria-controls="workspace-navigation"
              onClick={toggleSidebar}
            >
              <span aria-hidden="true">›</span>
            </button>
            <button
              ref={menu}
              className="mobile-menu secondary"
              aria-label="Open navigation"
              aria-expanded={mobileOpen}
              aria-controls="workspace-navigation"
              onClick={() => setMobileOpen(true)}
            >
              ☰
            </button>
            <span>
              Workspace <span className="slash">/</span> {sections[section]}
            </span>
          </div>
          <span className="top-label">
            PRAGMACHANGE <b>v0.1</b>
          </span>
          <button
            className="mobile-signout secondary small"
            onClick={() => run(signOut)}
          >
            Sign out
          </button>
        </header>
        <div className="page">
          <div className="page-heading">
            <div>
              <div className="eyebrow">BEHAVIORAL SECURITY</div>
              <h1>{sections[section]}</h1>
              <p>{captions[section]}</p>
            </div>
            <button
              className="secondary"
              onClick={() => run(refresh)}
              disabled={busy}
            >
              ↻ Refresh
            </button>
          </div>
          {error && (
            <div className="error" role="alert">
              {error}
            </div>
          )}
          {notice && (
            <div className="success" role="status">
              {notice}
            </div>
          )}
          {busy && (
            <div className="working" role="status">
              Working…
            </div>
          )}
          {!loaded ? (
            <Empty>Connecting to your management API…</Empty>
          ) : (
            <>
              {section === 0 && <OverviewSection />}
              {section === 1 && <EndpointsSection />}
              {section === 2 && <PipelinesSection />}
              {section === 3 && <DatasetsSection />}
              {section === 4 && <TrainingSection />}
              {section === 5 && <ReplaySection />}
              {section === 6 && <ReleasesSection />}
              {section === 7 && <AuditSection />}
              {section === 8 && <ServersSection />}
              {section === 9 && <BehaviorSection />}
              {section === 10 && <OnboardingSection />}
            </>
          )}
          <footer>
            PragmaChange{" "}
            <span>
              Local inference. Versioned configuration. Explicit decisions.
            </span>
          </footer>
        </div>
      </main>
    </div>
  );
}
