import { useEffect, useState } from "react";
import { defaultTextTitle, MAX_TITLE_CHARS, textByteLength, validatePastedText } from "../lib/documents";
import { formatBytes } from "../lib/format";
import { Dialog } from "./Dialog";
import { Icon } from "./Icon";

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
    <Dialog open={open} title="Adicionar texto ou conversa" onClose={cancel} wide>
      <form
        className="form"
        onSubmit={(event) => {
          event.preventDefault();
          if (canSave) onSubmit(title, text);
        }}
      >
        <label className="field">
          <span className="field__label">Título do documento</span>
          <input
            className="field__input"
            type="text"
            value={title}
            maxLength={MAX_TITLE_CHARS}
            onChange={(event) => setTitle(event.target.value)}
            placeholder="Ex.: conversa_whatsapp_2026.txt"
            autoComplete="off"
            spellCheck={false}
          />
        </label>
        <label className="field">
          <span className="field__label">Conteúdo do texto</span>
          <textarea
            className="field__input field__textarea"
            value={text}
            onChange={(event) => setText(event.target.value)}
            placeholder="Cole aqui transcrições, conversas em texto ou trechos copiados..."
            aria-invalid={problem !== null}
            aria-describedby="text-help"
            rows={10}
            autoFocus
          />
          <span id="text-help" className={problem ? "field__help field__help--error" : "field__help"}>
            {problem ??
              `${formatBytes(textByteLength(text))} de 5 MB. O texto será armazenado como um documento de texto local no projeto.`}
          </span>
        </label>
        {disabledReason ? (
          <p className="field__help field__help--error" role="alert">
            <Icon name="alert-circle" size={14} style={{ marginRight: "4px" }} />
            {disabledReason}
          </p>
        ) : null}
        {error ? (
          <p className="field__help field__help--error" role="alert">
            <Icon name="alert-triangle" size={14} style={{ marginRight: "4px" }} />
            {error}
          </p>
        ) : null}
        <div className="dialog__actions">
          <button type="button" className="btn" onClick={cancel} disabled={busy}>
            Cancelar
          </button>
          <button type="submit" className="btn btn--primary" disabled={!canSave}>
            <Icon name="plus" size={15} />
            <span>{busy ? "Salvando…" : "Salvar no projeto"}</span>
          </button>
        </div>
      </form>
    </Dialog>
  );
}
