// Small presentational primitives shared across screens.
import { LoaderCircle, X } from "lucide-react";
import { useEffect, type ReactNode } from "react";

export function Card({
  title,
  subtitle,
  action,
  children,
  className = "",
  bodyClass = "p-4",
}: {
  title?: ReactNode;
  subtitle?: ReactNode;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
  bodyClass?: string;
}) {
  return (
    <section className={`card flex min-w-0 flex-col ${className}`}>
      {(title || action) && (
        <header className="flex items-start justify-between gap-3 px-4 pt-3.5">
          <div className="min-w-0">
            {title && <h2 className="text-[13.5px] font-semibold text-ink">{title}</h2>}
            {subtitle && <p className="mt-0.5 truncate text-xs text-ink-3">{subtitle}</p>}
          </div>
          {action && <div className="flex shrink-0 items-center gap-1">{action}</div>}
        </header>
      )}
      <div className={`min-h-0 flex-1 ${bodyClass}`}>{children}</div>
    </section>
  );
}

type BtnVariant = "primary" | "secondary" | "ghost" | "danger" | "success";

export function Button({
  children,
  onClick,
  variant = "secondary",
  disabled,
  title,
  className = "",
  size = "md",
  type = "button",
}: {
  children: ReactNode;
  onClick?: (e: React.MouseEvent) => void;
  variant?: BtnVariant;
  disabled?: boolean;
  title?: string;
  className?: string;
  size?: "sm" | "md";
  type?: "button" | "submit";
}) {
  const base =
    "inline-flex items-center justify-center gap-1.5 rounded-lg font-medium transition-colors duration-100 disabled:opacity-45 disabled:pointer-events-none whitespace-nowrap";
  const sizes = { sm: "h-7 px-2.5 text-xs", md: "h-8 px-3 text-[13px]" };
  const variants: Record<BtnVariant, string> = {
    primary: "bg-accent text-white hover:bg-accent-strong",
    secondary: "border border-line bg-surface text-ink hover:bg-surface-2",
    ghost: "text-ink-2 hover:bg-surface-2 hover:text-ink",
    danger: "border border-line bg-surface text-danger hover:bg-surface-2",
    success: "bg-green-mark text-white hover:brightness-95",
  };
  return (
    <button type={type} title={title} disabled={disabled} onClick={onClick} className={`${base} ${sizes[size]} ${variants[variant]} ${className}`}>
      {children}
    </button>
  );
}

export function IconButton({
  children,
  onClick,
  title,
  active,
  disabled,
  className = "",
}: {
  children: ReactNode;
  onClick?: (e: React.MouseEvent) => void;
  title: string;
  active?: boolean;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      disabled={disabled}
      onClick={onClick}
      className={`inline-flex h-7 w-7 items-center justify-center rounded-md transition-colors duration-100 disabled:opacity-40 ${
        active ? "bg-accent-soft text-accent" : "text-ink-2 hover:bg-surface-2 hover:text-ink"
      } ${className}`}
    >
      {children}
    </button>
  );
}

export function Spinner({ size = 14 }: { size?: number }) {
  return <LoaderCircle size={size} className="animate-spin text-ink-3" />;
}

export function Empty({ icon, title, body, action }: { icon?: ReactNode; title: string; body?: ReactNode; action?: ReactNode }) {
  return (
    <div className="flex h-full min-h-28 flex-col items-center justify-center gap-1.5 px-6 py-6 text-center">
      {icon && <div className="mb-1 text-ink-3">{icon}</div>}
      <p className="text-[13px] font-medium text-ink-2">{title}</p>
      {body && <p className="max-w-sm text-xs text-ink-3">{body}</p>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}

export function Pill({ children, tone = "neutral", title }: { children: ReactNode; tone?: "neutral" | "accent" | "amber" | "green" | "cold"; title?: string }) {
  const tones = {
    neutral: "bg-surface-2 text-ink-2",
    accent: "bg-accent-soft text-accent",
    amber: "bg-amber-soft text-amber",
    green: "bg-green-soft text-green",
    cold: "bg-surface-2 text-cold",
  };
  return (
    <span title={title} className={`inline-flex h-5 shrink-0 items-center gap-1 rounded-full px-2 text-[11px] font-medium ${tones[tone]}`}>
      {children}
    </span>
  );
}

export function Modal({
  title,
  children,
  onClose,
  footer,
  width = 460,
}: {
  title: ReactNode;
  children: ReactNode;
  onClose: () => void;
  footer?: ReactNode;
  width?: number;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);
  return (
    <div className="anim-fade fixed inset-0 z-50 flex items-center justify-center bg-[var(--overlay)] p-6" onMouseDown={onClose}>
      <div
        className="anim-pop card flex max-h-full w-full flex-col shadow-[var(--shadow)]"
        style={{ maxWidth: width }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <header className="flex items-center justify-between border-b border-line px-4 py-3">
          <h2 className="text-sm font-semibold">{title}</h2>
          <IconButton title="Close" onClick={onClose}>
            <X size={15} />
          </IconButton>
        </header>
        <div className="min-h-0 flex-1 overflow-auto p-4">{children}</div>
        {footer && <footer className="flex justify-end gap-2 border-t border-line px-4 py-3">{footer}</footer>}
      </div>
    </div>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: { value: T; label: ReactNode; title?: string }[];
  onChange: (v: T) => void;
}) {
  return (
    <div className="inline-flex rounded-lg border border-line bg-surface-2 p-0.5">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          title={o.title}
          onClick={() => onChange(o.value)}
          className={`inline-flex h-6 items-center gap-1 rounded-md px-2 text-xs transition-colors duration-100 ${
            value === o.value ? "bg-surface text-ink shadow-[0_0_0_1px_var(--border)]" : "text-ink-2 hover:text-ink"
          }`}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Checkbox({ checked, onChange, label, indeterminate }: { checked: boolean; onChange: (v: boolean) => void; label?: string; indeterminate?: boolean }) {
  return (
    <input
      type="checkbox"
      aria-label={label}
      checked={checked}
      ref={(el) => {
        if (el) el.indeterminate = !!indeterminate;
      }}
      onChange={(e) => onChange(e.target.checked)}
      onClick={(e) => e.stopPropagation()}
      className="h-3.5 w-3.5 shrink-0 accent-[var(--accent)]"
    />
  );
}

export function TextInput(props: React.InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className={`h-8 rounded-lg border border-line bg-surface px-2.5 text-[13px] text-ink placeholder:text-ink-3 focus:border-accent focus:outline-none ${props.className ?? ""}`}
    />
  );
}

export function Select(props: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      {...props}
      className={`h-8 rounded-lg border border-line bg-surface px-2 text-[13px] text-ink focus:border-accent focus:outline-none ${props.className ?? ""}`}
    />
  );
}
