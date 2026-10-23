"use client";

import { Menu, X } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useRef, useState } from "react";

export function MobileNav({ links, cta }: { links: Array<{ href: string; label: string }>; cta: { href: string; label: string } }) {
  const [open, setOpen] = useState(false);
  const toggleRef = useRef<HTMLButtonElement>(null);
  const path = usePathname();
  const [closedFor, setClosedFor] = useState(path);
  if (closedFor !== path) {
    setClosedFor(path);
    setOpen(false);
  }

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setOpen(false);
        toggleRef.current?.focus();
      }
    };
    const desktop = window.matchMedia("(min-width: 1024px)");
    const closeOnDesktop = () => {
      if (desktop.matches) setOpen(false);
    };
    closeOnDesktop();
    desktop.addEventListener("change", closeOnDesktop);
    document.addEventListener("keydown", onKey);
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.removeEventListener("keydown", onKey);
      desktop.removeEventListener("change", closeOnDesktop);
      document.body.style.overflow = previousOverflow;
    };
  }, [open]);

  return (
    <div className="lg:hidden">
      <button
        type="button"
        ref={toggleRef}
        aria-expanded={open}
        aria-controls="mobile-menu"
        aria-label={open ? "Close menu" : "Open menu"}
        onClick={() => setOpen((o) => !o)}
        className="-mr-2 inline-flex size-11 items-center justify-center rounded-full text-fg transition-colors hover:bg-surface-2"
      >
        {open ? <X size={20} /> : <Menu size={20} />}
      </button>
      {open && (
        <div id="mobile-menu" className="fixed inset-x-0 bottom-0 top-[72px] z-40 overflow-y-auto overscroll-contain bg-bg px-5 pb-8 pt-4 sm:px-8">
          <nav aria-label="Mobile" className="flex flex-col">
            {links.map((l) => (
              <Link key={l.href} href={l.href} onClick={() => setOpen(false)} className="py-3 font-serif text-[30px] leading-tight tracking-[-0.02em] transition-colors hover:text-accent-text">
                {l.label}
              </Link>
            ))}
          </nav>
          <Link
            href={cta.href}
            onClick={() => setOpen(false)}
            className="btn btn-primary mt-8 w-full"
          >
            {cta.label}
          </Link>
        </div>
      )}
    </div>
  );
}
