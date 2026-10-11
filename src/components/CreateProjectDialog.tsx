import { useState } from "react";
import { core, normalizeError } from "../services/core";
import { validateProjectName } from "../lib/validation";
import type { DirectorySelection, ProjectInfo } from "../types/contracts";
import { ConfirmDialog } from "./ConfirmDialog";
import { Dialog } from "./Dialog";

interface CreateProjectDialogProps {
  open: boolean;
  onCancel: () => void;
  onCreated: (project: ProjectInfo) => void;
}

type Step = "form" | "confirm";

export function CreateProjectDialog({ open, onCancel, onCreated }: CreateProjectDialogProps) {
  const [name, setName] = useState("");
  const [location, setLocation] = useState<DirectorySelection | null>(null);
  const [step, setStep] = useState<Step>("form");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const nameError = name.length > 0 ? validateProjectName(name) : null;
  const canReview = validateProjectName(name) === null && location !== null;

  function reset() {
    setName("");
    setLocation(null);
    setStep("form");
    setError(null);
    setBusy(false);
  }

  function cancel() {
    if (busy) return;
    reset();
    onCancel();
  }

  async function chooseLocation() {
    setError(null);
    try {
      const selection = await core.pickDirectory("projectLocation");
      if (selection) setLocation(selection);
    } catch (e) {
      setError(normalizeError(e).message);
    }
  }

  async function create() {
    if (!location) return;
    setBusy(true);
    setError(null);
    try {
      const project = await core.createProject(name, location.token);
      reset();
      onCreated(project);
    } catch (e) {
      setError(normalizeError(e).message);
      setStep("form");
      setBusy(false);
    }
  }

  const trimmed = name.trim();

  return (
    <>
      <Dialog open={open && step === "form"} title="Novo projeto" onClose={cancel}>
        <form
          className="form"
          onSubmit={(event) => {
            event.preventDefault();
            if (canReview) setStep("confirm");
          }}
        >
          <label className="field">
            <span className="field__label">Nome do projeto</span>
            <input
              className="field__input"
              type="text"
              value={name}
              onChange={(event) => setName(event.target.value)}
              aria-invalid={nameError !== null}
              aria-describedby="name-help"
              autoFocus
              autoComplete="off"
              spellCheck={false}
            />
            <span id="name-help" className={nameError ? "field__help field__help--error" : "field__help"}>
              {nameError ?? "O nome também será o nome da pasta do projeto."}
            </span>
          </label>

          <div className="field">
            <span className="field__label">Local de armazenamento</span>
            <div className="field__row">
              <span className="path" title={location?.displayPath}>
                {location ? location.displayPath : "Nenhuma pasta escolhida"}
              </span>
              <button type="button" className="btn" onClick={() => void chooseLocation()}>
                Escolher pasta…
              </button>
            </div>
          </div>

          {error ? (
            <p className="field__help field__help--error" role="alert">
              {error}
            </p>
          ) : null}

          <div className="dialog__actions">
            <button type="button" className="btn" onClick={cancel}>
              Cancelar
            </button>
            <button type="submit" className="btn btn--primary" disabled={!canReview}>
              Revisar e criar
            </button>
          </div>
        </form>
      </Dialog>

      <ConfirmDialog
        open={open && step === "confirm"}
        title="Confirmar criação do projeto"
        confirmLabel="Criar projeto"
        busy={busy}
        onConfirm={() => void create()}
        onCancel={() => setStep("form")}
      >
        <p>Será criada uma nova pasta para o projeto em:</p>
        <p className="path path--block">
          {location ? `${location.displayPath} › ${trimmed}` : null}
        </p>
        <p className="muted">
          Nenhum arquivo existente será alterado ou removido. Se já houver algo com esse nome no
          local, a criação será cancelada.
        </p>
      </ConfirmDialog>
    </>
  );
}
