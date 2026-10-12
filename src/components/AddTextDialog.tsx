import { useEffect, useState } from "react";
import { defaultTextTitle, MAX_TITLE_CHARS, textByteLength, validatePastedText } from "../lib/documents";
import { formatBytes } from "../lib/format";
import { Dialog } from "./Dialog";

interface AddTextDialogProps {
  open: boolean;
  busy: boolean;
  error: string | null;
  disabledReason: string | null;
  onSubmit: (title: string, text: string) => void;
  onCancel: () => void;
}

export function AddTextDialog({ open, busy, error, disabledReason, onSubmit, onCancel }: AddTextDialogProps) {
  const [title, setTitle] = useState(() => defaultTextTitle());
  const [text, setText] = useState("");

  // Cada abertura começa com um formulário limpo.
  useEffect(() => {
    if (open) {
      setTitle(defaultTextTitle());
      setText("");
    }
  }, [open]);

  const problem = text.length > 0 ? validatePastedText(text, title) : null;
  const canSave = !busy && disabledReason === null && validatePastedText(text, title) === null;

  function cancel() {
    if (busy) return;
    onCancel();
  }

  return (
    <Dialog open={open} title="Adicionar texto copiado" onClose={cancel} wide>
      <form
        className="form"
        onSubmit={(event) => {
          event.preventDefault();
          if (canSave) onSubmit(title, text);
        }}
      >
        <label className="field">
          <span className="field__label">Título</span>
          <input
            className="field__input"
            type="text"
            value={title}
            maxLength={MAX_TITLE_CHARS}
            onChange={(event) => setTitle(event.target.value)}
            autoComplete="off"
            spellCheck={false}
          />
        </label>
        <label className="field">
          <span className="field__label">Texto</span>
          <textarea
            className="field__input field__textarea"
            value={text}
            onChange={(event) => setText(event.target.value)}
            aria-invalid={problem !== null}
            aria-describedby="text-help"
            rows={10}
            autoFocus
          />
          <span id="text-help" className={problem ? "field__help field__help--error" : "field__help"}>
            {problem ??
              `${formatBytes(textByteLength(text))} de 5 MB. O texto é salvo como um documento TXT dentro do projeto.`}
          </span>
        </label>
        {disabledReason ? (
          <p className="field__help field__help--error" role="alert">
            {disabledReason}
          </p>
        ) : null}
        {error ? (
          <p className="field__help field__help--error" role="alert">
            {error}
          </p>
        ) : null}
        <div className="dialog__actions">
          <button type="button" className="btn" onClick={cancel} disabled={busy}>
            Cancelar
          </button>
          <button type="submit" className="btn btn--primary" disabled={!canSave}>
            {busy ? "Salvando…" : "Salvar no projeto"}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
