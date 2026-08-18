import type { ReactNode } from "react";

export function TrustStatusRow({
  icon,
  label,
  value,
  tone,
  action
}: {
  icon: ReactNode;
  label: string;
  value: string;
  tone: TrustTone;
  action?: ReactNode;
}) {
  return (
    <div className="trust-status-row">
      <span className={`trust-status-icon ${tone}`} aria-hidden="true">
        {icon}
      </span>
      <span className="trust-status-copy">
        <span>{label}</span>
        <small>{value}</small>
      </span>
      {action ? <span className="trust-status-action">{action}</span> : null}
    </div>
  );
}

export function TrustActionButton({
  icon,
  label,
  disabled = false,
  variant = "primary",
  onClick
}: {
  icon: ReactNode;
  label: string;
  disabled?: boolean;
  variant?: "primary" | "secondary";
  onClick: () => void;
}) {
  return (
    <button
      className={`trust-action-button ${variant}`}
      type="button"
      disabled={disabled}
      onClick={onClick}
    >
      {icon}
      <span>{label}</span>
    </button>
  );
}

export type TrustTone = "good" | "warning" | "danger" | "neutral" | "progress";

export function DetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="settings-detail-row">
      <span>{label}</span>
      <small>{value}</small>
    </div>
  );
}
