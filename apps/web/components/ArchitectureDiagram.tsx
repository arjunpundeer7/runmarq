/**
 * The real data path, drawn from the implementation: vendor hooks call
 * `runmarq emit`, which normalizes and forwards to the local collector; the
 * collector adds counts read from agent transcripts and git, keeps run state,
 * forecasts and history in SQLite and feeds the desktop app. Nothing crosses
 * the machine boundary.
 */
export function ArchitectureDiagram() {
  const agents = ["Claude Code", "Codex", "Cursor"];
  const box = "fill-[var(--rm-surface)]";
  return (
    <figure className="w-full">
      <div className="rounded-[16px] bg-surface-2 p-3 md:hidden">
        <p className="px-1 pb-2 text-[14px] text-muted">This computer</p>
        <ol className="space-y-2">
          {[
            ["Claude Code, Codex, Cursor", "official hooks"],
            ["runmarq emit", "normalize, minimize, spool if offline"],
            ["Transcripts, git", "counts only, no text"],
            ["Collector", "run state, estimator, SQLite history, local API"],
            ["Desktop app, tray, notifications", null],
          ].map(([t, d], i, all) => (
            <li key={t} className="rounded-[12px] bg-surface p-4">
              <p className="text-[16px] font-medium">{t}</p>
              {d && <p className="text-[14px] text-muted">{d}</p>}
              {i < all.length - 1 && <span className="sr-only">then</span>}
            </li>
          ))}
        </ol>
      </div>
      <svg
        viewBox="0 0 960 360"
        className="hidden h-auto w-full md:block"
        role="img"
        aria-labelledby="arch-title arch-desc"
        fontFamily="var(--font-sans)"
      >
        <title id="arch-title">How events flow through runmarq</title>
        <desc id="arch-desc">
          Claude Code, Codex and Cursor call their official hooks, which run runmarq emit. The emit step normalizes each event, drops prompt
          text and forwards it over an authenticated loopback connection to the local collector. The collector also reads token counts
          from the agents&apos; transcripts and change sizes from git, maintains run state, computes forecasts, stores timing data in
          SQLite and streams updates to the desktop app, tray and notifications. Everything stays on this computer.
        </desc>
        <rect x="8" y="8" width="944" height="344" rx="22" className="fill-[var(--rm-surface-2)]" />
        <text x="32" y="40" fontSize="14" className="fill-[var(--rm-muted)]">
          This computer
        </text>

        {agents.map((a, i) => (
          <g key={a}>
            <rect x="32" y={70 + i * 90} width="168" height="64" rx="12" className={box} />
            <text x="52" y={98 + i * 90} fontSize="16" fontWeight="500" className="fill-[var(--rm-fg)]">
              {a}
            </text>
            <text x="52" y={118 + i * 90} fontSize="13" className="fill-[var(--rm-muted)]">
              official hooks
            </text>
            <path d={`M200 ${102 + i * 90} C 236 ${102 + i * 90}, 236 192, 262 192`} fill="none" stroke="var(--rm-border-strong)" strokeWidth="2" />
          </g>
        ))}

        <rect x="262" y="148" width="176" height="88" rx="12" className={box} />
        <text x="282" y="180" fontSize="16" fontWeight="500" className="fill-[var(--rm-fg)]">
          runmarq emit
        </text>
        <text x="282" y="201" fontSize="13" className="fill-[var(--rm-muted)]">
          normalize, minimize
        </text>
        <text x="282" y="219" fontSize="13" className="fill-[var(--rm-muted)]">
          spool if offline
        </text>

        <rect x="262" y="262" width="176" height="64" rx="12" className={box} />
        <text x="282" y="290" fontSize="16" fontWeight="500" className="fill-[var(--rm-fg)]">
          Transcripts, git
        </text>
        <text x="282" y="310" fontSize="13" className="fill-[var(--rm-muted)]">
          counts only, no text
        </text>
        <path d="M438 294 C 470 294, 470 270, 500 270" fill="none" stroke="var(--rm-border-strong)" strokeWidth="2" />

        <path d="M438 192 H500" stroke="var(--rm-accent)" strokeWidth="2" />
        <text x="444" y="180" fontSize="12.5" className="fill-[var(--rm-muted)]">
          loopback
        </text>

        <rect x="500" y="88" width="216" height="208" rx="14" className={box} />
        <text x="522" y="120" fontSize="16" fontWeight="500" className="fill-[var(--rm-fg)]">
          Collector
        </text>
        {["Run state engine", "ETA estimator", "SQLite history", "Local API"].map((t, i) => (
          <g key={t}>
            <rect x="522" y={136 + i * 38} width="172" height="30" rx="7" className="fill-[var(--rm-surface-2)]" />
            <text x="536" y={156 + i * 38} fontSize="13.5" className="fill-[var(--rm-fg-2)]">
              {t}
            </text>
          </g>
        ))}

        {["Desktop app", "Tray", "Notifications"].map((t, i) => (
          <g key={t}>
            <path d={`M716 192 C 748 192, 748 ${102 + i * 90}, 776 ${102 + i * 90}`} fill="none" stroke="var(--rm-border-strong)" strokeWidth="2" />
            <rect x="776" y={70 + i * 90} width="152" height="64" rx="12" className={box} />
            <text x="796" y={107 + i * 90} fontSize="16" fontWeight="500" className="fill-[var(--rm-fg)]">
              {t}
            </text>
          </g>
        ))}
      </svg>
    </figure>
  );
}
