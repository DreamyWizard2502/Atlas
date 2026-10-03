import {
  Binary,
  Box,
  File,
  FileArchive,
  FileCode,
  FileSpreadsheet,
  FileText,
  Film,
  Folder,
  Image,
  Music,
  Package,
  Presentation,
  Type,
  type LucideIcon,
} from "lucide-react";
import { useState } from "react";
import { assetUrl, type Kind } from "../lib/api";

const ICONS: Record<Kind, LucideIcon> = {
  folder: Folder,
  image: Image,
  video: Film,
  audio: Music,
  pdf: FileText,
  doc: FileText,
  sheet: FileSpreadsheet,
  slides: Presentation,
  text: FileText,
  code: FileCode,
  archive: FileArchive,
  cad: Box,
  installer: Package,
  font: Type,
  object: Binary,
  other: File,
};

/** Largest image the thumbnail/Quick Look paths load (whole-file reads). */
export const MAX_THUMB_BYTES = 24 * 1024 * 1024;

export function FileIcon({ kind, size = 16, className = "" }: { kind: Kind; size?: number; className?: string }) {
  const Icon = ICONS[kind] ?? File;
  const tone = kind === "folder" ? "text-accent" : "text-ink-3";
  return <Icon size={size} strokeWidth={1.6} className={`${tone} shrink-0 ${className}`} />;
}

const THUMB_EXTS = new Set(["jpg", "jpeg", "png", "gif", "webp", "bmp", "svg", "ico", "avif"]);

export function canThumb(ext?: string) {
  return !!ext && THUMB_EXTS.has(ext.toLowerCase());
}

/** Image thumbnail via the asset protocol, falling back to the type icon. */
export function Thumb({ path, ext, kind, size, iconSize = 28 }: { path: string; ext?: string; kind: Kind; size?: number | null; iconSize?: number }) {
  const [failed, setFailed] = useState(false);
  // Anything that isn't really an image (or is huge) falls back to the icon.
  if (canThumb(ext) && !failed && (size == null || (size > 0 && size <= MAX_THUMB_BYTES))) {
    return (
      <img
        src={assetUrl(path)}
        loading="lazy"
        decoding="async"
        draggable={false}
        onError={() => setFailed(true)}
        className="h-full w-full object-cover"
        alt=""
      />
    );
  }
  return (
    <div className="flex h-full w-full items-center justify-center">
      <FileIcon kind={kind} size={iconSize} />
    </div>
  );
}

export function kindLabel(kind: Kind, ext?: string): string {
  if (kind === "folder") return "Folder";
  if (kind === "object") return "Compiled object file";
  if (ext) return `${ext.toUpperCase()} file`;
  return "File";
}
