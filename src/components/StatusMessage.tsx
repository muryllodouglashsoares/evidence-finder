export type Notice = { kind: "success" | "warning" | "error"; text: string };

interface StatusMessageProps {
  notice: Notice;
  onDismiss: () => void;
}

export function StatusMessage({ notice, onDismiss }: StatusMessageProps) {
  const isError = notice.kind === "error";
  return (
    <div
      className={`notice notice--${notice.kind}`}
      role={isError ? "alert" : "status"}
    >
      <p className="notice__text">{notice.text}</p>
      <button type="button" className="notice__close" onClick={onDismiss} aria-label="Dispensar mensagem">
        ×
      </button>
    </div>
  );
}
