import * as Dialog from "@radix-ui/react-dialog";
import * as Switch from "@radix-ui/react-switch";
import { cn } from "@runmarq/ui";
import type { ButtonHTMLAttributes, ReactNode, SelectHTMLAttributes } from "react";

type Variant = "primary" | "secondary" | "ghost" | "danger";

const VARIANTS: Record<Variant, string> = {
  primary: "bg-fg text-bg hover:bg-fg-2 disabled:bg-line-strong disabled:text-muted",
  secondary: "border border-line-strong bg-surface text-fg hover:bg-surface-2 disabled:text-faint",
  ghost: "text-fg-2 hover:bg-surface-2 hover:text-fg disabled:text-faint",
  danger: "border border-line-strong bg-surface text-danger hover:bg-danger-muted disabled:text-faint",
};

export function Button({
  variant = "secondary",
  size = "md",
  className,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: "sm" | "md" }) {
  return (
    <button
      type="button"
      className={cn(
        "inline-flex shrink-0 items-center justify-center gap-1.5 whitespace-nowrap rounded-lg font-medium transition-colors duration-150 disabled:cursor-not-allowed",
        size === "sm" ? "h-7 px-2.5 text-[12.5px]" : "h-8 px-3 text-[13px]",
        VARIANTS[variant],
        className,
      )}
      {...props}
    />
  );
}

export function Toggle({
  checked,
  onChange,
  label,
  description,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  description?: ReactNode;
  disabled?: boolean;
}) {
  const id = `t-${label.replace(/\W+/g, "-").toLowerCase()}`;
  return (
    <div className="flex items-start justify-between gap-6 py-3">
      <div className="min-w-0">
        <label htmlFor={id} className={cn("text-[13.5px] font-medium", disabled ? "text-faint" : "text-fg")}>
          {label}
        </label>
        {description && <p className="mt-0.5 text-[12.5px] leading-relaxed text-muted">{description}</p>}
      </div>
      <Switch.Root
        id={id}
        checked={checked}
        onCheckedChange={onChange}
        disabled={disabled}
        className="relative mt-0.5 h-[18px] w-8 shrink-0 rounded-full bg-line-strong transition-colors data-[state=checked]:bg-fg disabled:opacity-50"
      >
        <Switch.Thumb className="block size-[14px] translate-x-[2px] rounded-full bg-surface shadow-sm transition-transform duration-150 data-[state=checked]:translate-x-[16px]" />
      </Switch.Root>
    </div>
  );
}

export function Select({ className, children, ...props }: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      className={cn(
        "rm-select h-8 appearance-none rounded-lg border border-line-strong bg-surface pl-2.5 pr-7 text-[13px] text-fg outline-none focus-visible:border-accent disabled:text-faint",
        className,
      )}
      {...props}
    >
      {children}
    </select>
  );
}

export function PageHeader({ title, description, actions }: { title: string; description?: ReactNode; actions?: ReactNode }) {
  return (
    <header className="flex items-end justify-between gap-6 pb-6">
      <div>
        <h1 className="text-[22px] font-semibold tracking-[-0.02em]">{title}</h1>
        {description && <p className="mt-1 max-w-xl text-[13.5px] text-muted">{description}</p>}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </header>
  );
}

export function Section({ title, description, children, actions }: { title: string; description?: ReactNode; children: ReactNode; actions?: ReactNode }) {
  return (
    <section className="border-t border-line py-6 first:border-t-0 first:pt-0">
      <div className="mb-3 flex items-baseline justify-between gap-4">
        <div>
          <h2 className="text-[14px] font-semibold tracking-[-0.01em]">{title}</h2>
          {description && <p className="mt-0.5 text-[12.5px] text-muted">{description}</p>}
        </div>
        {actions}
      </div>
      {children}
    </section>
  );
}

export function EmptyState({ title, children, icon }: { title: string; children?: ReactNode; icon?: ReactNode }) {
  return (
    <div className="flex flex-col items-start rounded-[var(--radius-card)] border border-dashed border-line-strong px-6 py-10">
      {icon && <div className="mb-3 text-faint">{icon}</div>}
      <p className="text-[15px] font-medium">{title}</p>
      {children && <div className="mt-1.5 max-w-md text-[13px] leading-relaxed text-muted">{children}</div>}
    </div>
  );
}

export function Notice({ tone = "neutral", children }: { tone?: "neutral" | "warn" | "danger"; children: ReactNode }) {
  return (
    <div
      role={tone === "danger" ? "alert" : "status"}
      className={cn(
        "rounded-lg px-3.5 py-2.5 text-[13px] leading-relaxed",
        tone === "neutral" && "bg-surface-2 text-fg-2",
        tone === "warn" && "bg-warn-muted text-warn",
        tone === "danger" && "bg-danger-muted text-danger",
      )}
    >
      {children}
    </div>
  );
}

export function Modal({
  open,
  onOpenChange,
  title,
  description,
  children,
  footer,
  wide,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  title: string;
  description?: ReactNode;
  children?: ReactNode;
  footer?: ReactNode;
  wide?: boolean;
}) {
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/25 backdrop-blur-[1px]" />
        <Dialog.Content
          className={cn(
            "rm-rise fixed left-1/2 top-1/2 max-h-[85vh] w-[calc(100vw-48px)] -translate-x-1/2 -translate-y-1/2 overflow-auto rounded-[14px] border border-line bg-surface p-6 shadow-soft scroll-quiet",
            wide ? "max-w-2xl" : "max-w-md",
          )}
        >
          <Dialog.Title className="text-[16px] font-semibold tracking-[-0.01em]">{title}</Dialog.Title>
          {description && <Dialog.Description className="mt-1 text-[13px] text-muted">{description}</Dialog.Description>}
          {children && <div className="mt-4">{children}</div>}
          {footer && <div className="mt-6 flex justify-end gap-2">{footer}</div>}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return <code className="selectable rounded-md bg-surface-2 px-1.5 py-0.5 font-mono text-[12px] text-fg-2">{children}</code>;
}

export function Spinner({ className }: { className?: string }) {
  return (
    <span
      role="status"
      aria-label="Loading"
      className={cn("inline-block size-3.5 animate-spin rounded-full border-[1.5px] border-line-strong border-t-fg-2", className)}
    />
  );
}
