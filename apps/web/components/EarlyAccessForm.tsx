"use client";

import Link from "next/link";
import { useState } from "react";

type State = { kind: "idle" } | { kind: "sending" } | { kind: "done"; already: boolean } | { kind: "error"; message: string };

export function EarlyAccessForm() {
  const [state, setState] = useState<State>({ kind: "idle" });

  const submit = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const f = new FormData(e.currentTarget);
    setState({ kind: "sending" });
    try {
      const res = await fetch("/api/early-access", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          email: f.get("email"),
          platform: f.get("platform") || null,
          consent: f.get("consent") === "on",
          company: f.get("company"),
        }),
      });
      const data = (await res.json().catch(() => ({}))) as { status?: string; error?: string };
      if (res.ok) setState({ kind: "done", already: data.status === "already_registered" });
      else setState({ kind: "error", message: data.error ?? "Something went wrong. Please try again." });
    } catch {
      setState({ kind: "error", message: "Could not reach the server. Check your connection and try again." });
    }
  };

  if (state.kind === "done") {
    return (
      <p role="status" className="rounded-[14px] bg-ok-muted px-5 py-4 text-[16px] text-fg">
        {state.already ? "That address is already on the list. We will be in touch." : "Thanks. We will email you when a build for your platform is ready."}
      </p>
    );
  }

  return (
    <form onSubmit={submit} className="max-w-lg space-y-4" noValidate>
      <div>
        <label htmlFor="ea-email" className="text-[15px] font-medium">
          Email
        </label>
        <input
          id="ea-email"
          name="email"
          type="email"
          required
          autoComplete="email"
          maxLength={254}
          className="mt-1.5 h-12 w-full rounded-[12px] bg-surface px-4 text-[16px] outline-none ring-1 ring-line-strong focus-visible:ring-2 focus-visible:ring-accent"
        />
      </div>
      <div>
        <label htmlFor="ea-platform" className="text-[15px] font-medium">
          Platform
        </label>
        <select
          id="ea-platform"
          name="platform"
          defaultValue=""
          className="mt-1.5 h-12 w-full rounded-[12px] bg-surface px-3 text-[16px] outline-none ring-1 ring-line-strong focus-visible:ring-2 focus-visible:ring-accent"
        >
          <option value="">No preference</option>
          <option value="macos">macOS</option>
          <option value="windows">Windows</option>
          <option value="linux">Linux</option>
        </select>
      </div>
      <div className="hidden" aria-hidden="true">
        <label>
          Company <input name="company" tabIndex={-1} autoComplete="off" />
        </label>
      </div>
      <label className="flex items-start gap-3 text-[15px] text-fg-2">
        <input name="consent" type="checkbox" required className="mt-1 size-4 accent-[var(--rm-fg)]" />
        <span>
          Email me about early access. Your address is used only for this and can be removed on request. See the{" "}
          <Link href="/privacy" className="underline underline-offset-4">
            privacy statement
          </Link>
          .
        </span>
      </label>
      {state.kind === "error" && (
        <p role="alert" className="text-[15px] text-danger">
          {state.message}
        </p>
      )}
      <button
        type="submit"
        disabled={state.kind === "sending"}
        className="btn btn-primary disabled:opacity-60"
      >
        {state.kind === "sending" ? "Sending…" : "Request early access"}
      </button>
    </form>
  );
}
