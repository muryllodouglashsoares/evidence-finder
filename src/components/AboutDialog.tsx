import pkg from "../../package.json";
import { Dialog } from "./Dialog";
import { Icon } from "./Icon";
import type { IconName } from "./Icon";

interface AboutDialogProps {
  open: boolean;
  onClose: () => void;
}

const SECTIONS: { icon: IconName; title: string; text: string }[] = [
  {
    icon: "search",
    title: "Para que serve",
    text: "Pesquisa local em evidências digitais: capturas de tela, imagens, conversas em texto e PDFs, com o trecho encontrado ligado ao documento de origem.",
  },
  {
    icon: "file",
    title: "Arquivos aceitos",
    text: "PNG, JPEG, TXT e PDF (até 500 MB por arquivo e 500 documentos por projeto), além de texto copiado e colado. O OCR de imagens e de PDFs digitalizados usa o Tesseract, que precisa estar instalado.",
  },
  {
    icon: "layers",
    title: "Como a busca funciona",
    text: "A busca por palavras, frase exata ou combinação de termos usa um índice local (SQLite FTS5), com filtros e contexto do trecho. A busca semântica local, por significado ou híbrida, é opcional e exige gerar um índice no projeto.",
  },
  {
    icon: "shield-check",
    title: "Privacidade e integridade",
    text: "Tudo é processado neste computador, sem contas, telemetria ou conexão externa. Os arquivos originais nunca são alterados nem apagados, e o SHA-256 permite conferir a integridade.",
  },
  {
    icon: "download",
    title: "Exportação",
    text: "Os resultados selecionados saem em relatório PDF ou em uma pasta de evidências com manifest.json.",
  },
];

export function AboutDialog({ open, onClose }: AboutDialogProps) {
  return (
    <Dialog open={open} title="Sobre o Evidence Finder" onClose={onClose}>
      <dl className="about">
        {SECTIONS.map((section) => (
          <div key={section.title} className="about__item">
            <dt>
              <Icon name={section.icon} size={15} />
              <span>{section.title}</span>
            </dt>
            <dd>{section.text}</dd>
          </div>
        ))}
      </dl>
      <div className="dialog__actions about__footer">
        <span className="muted small">Versão {pkg.version}</span>
        <button type="button" className="btn btn--small" onClick={onClose}>
          Fechar
        </button>
      </div>
    </Dialog>
  );
}
