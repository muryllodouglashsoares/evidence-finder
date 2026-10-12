import { useState } from "react";
import { useDocuments } from "../hooks/useDocuments";
import {
  availabilityHint,
  AVAILABILITY_LABEL,
  AVAILABILITY_TONE,
  FORMAT_LABEL,
  needsDetails,
  STORAGE_LABEL,
  summarizeImport,
} from "../lib/documents";
import { formatBytes, formatDateTime } from "../lib/format";
import { core, normalizeError } from "../services/core";
import type { DocumentInfo, ImportResult, PickedFiles, StorageMode } from "../types/contracts";
import { AddTextDialog } from "./AddTextDialog";
import { ConfirmDialog } from "./ConfirmDialog";
import { ImportDialog } from "./ImportDialog";
import { ImportResultDialog } from "./ImportResultDialog";
import type { Notice } from "./StatusMessage";

interface DocumentsPanelProps {
  projectId: string;
  onNotice: (notice: Notice | null) => void;
}

export function DocumentsPanel({ projectId, onNotice }: DocumentsPanelProps) {
  const { documents, limit, loading, error, refresh } = useDocuments(projectId);
  const [picking, setPicking] = useState(false);
  const [picked, setPicked] = useState<PickedFiles | null>(null);
  const [importing, setImporting] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);
  const [result, setResult] = useState<ImportResult | null>(null);
  const [addingText, setAddingText] = useState(false);
  const [savingText, setSavingText] = useState(false);
  const [textError, setTextError] = useState<string | null>(null);
  const [toRemove, setToRemove] = useState<DocumentInfo | null>(null);
  const [removing, setRemoving] = useState(false);
  const [verifyingId, setVerifyingId] = useState<string | null>(null);

  const remaining = Math.max(0, limit - documents.length);
  const full = remaining === 0;
  const fullReason = full ? `O projeto atingiu o limite de ${limit} documentos.` : null;
  const busy = picking || importing || savingText || removing || verifyingId !== null;

  function report(importResult: ImportResult) {
    onNotice(summarizeImport(importResult));
    if (needsDetails(importResult)) setResult(importResult);
  }

  async function startImport() {
    setPicking(true);
    onNotice(null);
    try {
      const selection = await core.pickDocuments(projectId);
      if (selection && selection.files.length > 0) {
        setImportError(null);
        setPicked(selection);
      } else if (selection) {
        onNotice({ kind: "warning", text: "Nenhum dos itens escolhidos pôde ser lido." });
      }
    } catch (e) {
      onNotice({ kind: "error", text: normalizeError(e).message });
    } finally {
      setPicking(false);
    }
  }

  async function confirmImport(mode: StorageMode) {
    if (!picked) return;
    setImporting(true);
    setImportError(null);
    try {
      const importResult = await core.importDocuments(
        projectId,
        picked.files.map((f) => f.token),
        mode,
      );
      setPicked(null);
      report(importResult);
    } catch (e) {
      setImportError(normalizeError(e).message);
    } finally {
      setImporting(false);
      void refresh();
    }
  }

  async function saveText(title: string, text: string) {
    setSavingText(true);
    setTextError(null);
    try {
      const textResult = await core.addTextDocument(projectId, title, text);
      setAddingText(false);
      report(textResult);
    } catch (e) {
      setTextError(normalizeError(e).message);
    } finally {
      setSavingText(false);
      void refresh();
    }
  }

  async function verify(doc: DocumentInfo) {
    setVerifyingId(doc.id);
    onNotice(null);
    try {
      const checked = await core.verifyDocument(projectId, doc.id);
      onNotice(
        checked.availability === "available"
          ? { kind: "success", text: `"${doc.name}": o conteúdo confere com o registro.` }
          : { kind: "warning", text: `"${doc.name}": ${AVAILABILITY_LABEL[checked.availability].toLowerCase()}.` },
      );
    } catch (e) {
      onNotice({ kind: "error", text: normalizeError(e).message });
    } finally {
      setVerifyingId(null);
      void refresh();
    }
  }

  async function confirmRemove() {
    if (!toRemove) return;
    setRemoving(true);
    try {
      const removed = await core.removeDocument(projectId, toRemove.id);
      onNotice(
        removed.warning
          ? { kind: "warning", text: removed.warning }
          : { kind: "success", text: `"${toRemove.name}" foi removido do projeto. O arquivo original não foi alterado.` },
      );
    } catch (e) {
      onNotice({ kind: "error", text: normalizeError(e).message });
    } finally {
      setToRemove(null);
      setRemoving(false);
      void refresh();
    }
  }

  return (
    <section className="docs" aria-labelledby="docs-title">
      <div className="docs__header">
        <div>
          <h2 id="docs-title" className="section-title">
            Documentos
          </h2>
          <p className="muted small" aria-live="polite">
            {loading ? "Carregando…" : `${documents.length} de ${limit} documentos`}
          </p>
        </div>
        <div className="actions">
          <button
            type="button"
            className="btn btn--primary"
            onClick={() => void startImport()}
            disabled={busy || full || loading}
            title={fullReason ?? undefined}
          >
            {picking ? "Abrindo…" : "Importar arquivos…"}
          </button>
          <button
            type="button"
            className="btn"
            onClick={() => {
              setTextError(null);
              onNotice(null);
              setAddingText(true);
            }}
            disabled={busy || full || loading}
            title={fullReason ?? undefined}
          >
            Adicionar texto…
          </button>
        </div>
      </div>

      {error ? (
        <div className="empty">
          <p>{error}</p>
          <button type="button" className="btn" onClick={() => void refresh()}>
            Tentar novamente
          </button>
        </div>
      ) : loading ? (
        <p className="muted" role="status">
          Carregando documentos…
        </p>
      ) : documents.length === 0 ? (
        <div className="empty">
          <p>Ainda não há documentos neste projeto.</p>
          <p className="muted">
            Importe imagens (PNG, JPEG), arquivos TXT e PDF, ou cole um texto. Os documentos ficam
            organizados aqui; a leitura do conteúdo (OCR) e a busca chegam em fases futuras.
          </p>
        </div>
      ) : (
        <ul className="doclist">
          {documents.map((doc) => {
            const tone = AVAILABILITY_TONE[doc.availability];
            const hint = availabilityHint(doc.availability, doc.storageMode);
            return (
              <li key={doc.id} className="doc">
                <div className="doc__main">
                  <p className="doc__name" title={doc.name}>
                    {doc.name}
                  </p>
                  <p className="doc__meta">
                    <span className="badge">{FORMAT_LABEL[doc.format]}</span>
                    <span>{formatBytes(doc.sizeBytes)}</span>
                    <span>{STORAGE_LABEL[doc.storageMode]}</span>
                    <span>Importado em {formatDateTime(doc.importedAt)}</span>
                  </p>
                  {doc.sourcePath ? (
                    <p className="doc__path path" title={doc.sourcePath}>
                      {doc.sourcePath}
                    </p>
                  ) : null}
                  <p className="doc__status">
                    <span className={`badge badge--${tone}`}>{AVAILABILITY_LABEL[doc.availability]}</span>
                    {hint ? <span className="muted small"> {hint}</span> : null}
                  </p>
                </div>
                <div className="doc__actions">
                  <button
                    type="button"
                    className="btn btn--small"
                    onClick={() => void verify(doc)}
                    disabled={busy}
                    title="Relê o arquivo e confere o SHA-256 registrado"
                  >
                    {verifyingId === doc.id ? "Verificando…" : "Verificar"}
                  </button>
                  <button
                    type="button"
                    className="btn btn--small btn--quiet"
                    onClick={() => setToRemove(doc)}
                    disabled={busy}
                  >
                    Remover
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      )}

      <ImportDialog
        picked={picked}
        remaining={remaining}
        busy={importing}
        error={importError}
        onConfirm={(mode) => void confirmImport(mode)}
        onCancel={() => setPicked(null)}
      />
      <ImportResultDialog result={result} onClose={() => setResult(null)} />
      <AddTextDialog
        open={addingText}
        busy={savingText}
        error={textError}
        disabledReason={fullReason}
        onSubmit={(title, text) => void saveText(title, text)}
        onCancel={() => setAddingText(false)}
      />
      <ConfirmDialog
        open={toRemove !== null}
        title="Remover documento do projeto?"
        confirmLabel={toRemove?.storageMode === "copy" ? "Remover e excluir a cópia" : "Remover do projeto"}
        busy={removing}
        onConfirm={() => void confirmRemove()}
        onCancel={() => setToRemove(null)}
      >
        <p>
          <strong>{toRemove?.name}</strong>
        </p>
        {toRemove?.storageMode === "copy" ? (
          <p>
            O registro será removido e a <strong>cópia guardada dentro do projeto será excluída</strong>.
            O arquivo original, em sua pasta de origem, não será alterado nem apagado.
          </p>
        ) : (
          <p>
            Apenas o registro será removido do projeto. Nenhum arquivo é excluído: o original
            continua intacto onde está.
          </p>
        )}
      </ConfirmDialog>
    </section>
  );
}
