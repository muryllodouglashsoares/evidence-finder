import { Icon } from "./Icon";

export type Notice = { kind: "success" | "warning" | "error"; text: string };

interface StatusMessageProps {
  notice: Notice;
  onDismiss: () => void;
}

export function StatusMessage({ notice, onDismiss }: StatusMessageProps) {
  const isError = notice.kind === "error";

  const iconName =
    notice.kind === "success"
      ? "check"
      : notice.kind === "warning"
        ? "alert-triangle"
        : "alert-circle";

  return (
    <div
      className={`notice notice--${notice.kind}`}
      role={isError ? "alert" : "status"}
    >
      <div className="notice__text">
        <Icon name={iconName} size={18} />
        <span>{notice.text}</span>
      </div>
      <button
        type="button"
        className="notice__close"
        onClick={onDismiss}
        aria-label="Dispensar mensagem"
      >
        <Icon name="x" size={16} />
      </button>
    </div>
  );
}
