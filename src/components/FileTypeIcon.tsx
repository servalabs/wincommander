import type { ReactNode } from "react";
import type { ChipKind } from "@/lib/searchTokens";
import excelIcon from "@/assets/file-types/excel.svg";
import pdfIcon from "@/assets/file-types/pdf.svg";
import powerpointIcon from "@/assets/file-types/powerpoint.svg";
import wordIcon from "@/assets/file-types/word.svg";

export type FileTypeIconKind = ChipKind | "all";

export const FILE_TYPE_ICON_KINDS = [
  "pdf",
  "word",
  "excel",
  "slides",
  "text",
  "images",
  "videos",
  "audio",
  "archives",
  "code",
  "apps",
  "folders",
  "all",
] as const satisfies readonly FileTypeIconKind[];

const OFFICIAL_PNG: Partial<Record<FileTypeIconKind, string>> = {
  pdf: pdfIcon,
  word: wordIcon,
  excel: excelIcon,
  slides: powerpointIcon,
};

function FoldedDoc({ fill }: { fill: string }) {
  return (
    <>
      <path fill={fill} d="M3.8 1.4h8.3L16.6 6.1v12.5H3.8z" />
      <path fill="#fff" fillOpacity="0.3" d="M12.1 1.4 16.6 6.1h-4.5z" />
    </>
  );
}

function glyph(kind: FileTypeIconKind): ReactNode {
  switch (kind) {
    case "text":
      return (
        <>
          <FoldedDoc fill="#5B7394" />
          <rect x="6" y="8.3" width="8" height="1.2" rx="0.4" fill="#fff" />
          <rect x="6" y="11" width="8" height="1.2" rx="0.4" fill="#fff" />
          <rect x="6" y="13.7" width="5.4" height="1.2" rx="0.4" fill="#fff" />
        </>
      );
    case "images":
      return (
        <>
          <rect x="1.6" y="3.4" width="16.8" height="13.2" rx="2" fill="#3D8B6E" />
          <circle cx="6.6" cy="7.6" r="1.7" fill="#F6E27A" />
          <path fill="#fff" fillOpacity="0.92" d="M3.1 14.8 7.6 9.6l3.1 2.8 2.6-3.2 4.6 5.6z" />
        </>
      );
    case "videos":
      return (
        <>
          <rect x="1.5" y="3.6" width="17" height="12.8" rx="2" fill="#7B3FA0" />
          <rect x="2.7" y="5.2" width="1.15" height="1.15" rx="0.2" fill="#fff" fillOpacity="0.7" />
          <rect x="2.7" y="8.1" width="1.15" height="1.15" rx="0.2" fill="#fff" fillOpacity="0.7" />
          <rect x="2.7" y="11" width="1.15" height="1.15" rx="0.2" fill="#fff" fillOpacity="0.7" />
          <rect x="16.15" y="5.2" width="1.15" height="1.15" rx="0.2" fill="#fff" fillOpacity="0.7" />
          <rect x="16.15" y="8.1" width="1.15" height="1.15" rx="0.2" fill="#fff" fillOpacity="0.7" />
          <rect x="16.15" y="11" width="1.15" height="1.15" rx="0.2" fill="#fff" fillOpacity="0.7" />
          <path fill="#fff" d="M8 7.1v5.8l5.2-2.9z" />
        </>
      );
    case "audio":
      return (
        <>
          <rect x="1.6" y="1.6" width="16.8" height="16.8" rx="3" fill="#5B4FC9" />
          <ellipse cx="7.2" cy="13.6" rx="2.9" ry="2.15" fill="#fff" transform="rotate(-18 7.2 13.6)" />
          <rect x="9.55" y="5.1" width="1.35" height="9" rx="0.35" fill="#fff" />
          <path fill="#fff" d="M10.9 5.1c3.1.45 4.3 2.4 4.2 4.8-1.55-.85-2.95-1.35-4.2-1.55z" />
        </>
      );
    case "archives":
      return (
        <>
          <path fill="#C4A035" d="M4.1 4.2h11.8v12.2H4.1z" />
          <path fill="#A88620" d="M4.1 4.2h11.8v3.1H4.1z" />
          <rect x="9.15" y="4.2" width="1.7" height="12.2" fill="#3A3320" />
          {([5.5, 7.1, 8.7, 10.3, 11.9, 13.5] as const).map((y) => (
            <rect key={y} x="8.55" y={y} width="2.9" height="0.85" rx="0.15" fill="#EFE6C4" />
          ))}
          <rect x="8.35" y="9.05" width="3.3" height="2.5" rx="0.35" fill="#2F2A1C" />
        </>
      );
    case "code":
      return (
        <>
          <rect x="1.6" y="1.6" width="16.8" height="16.8" rx="3" fill="#2D3A4A" />
          <path
            d="M7.6 6.2 4.3 10 7.6 13.8M12.4 6.2 15.7 10 12.4 13.8M11.15 5.6 8.85 14.4"
            fill="none"
            stroke="#7EE0B0"
            strokeWidth="1.55"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </>
      );
    case "apps":
      return (
        <>
          <rect x="2.2" y="3.2" width="15.6" height="13.6" rx="1.8" fill="#3D5A80" />
          <rect x="2.2" y="3.2" width="15.6" height="3.4" rx="1.8" fill="#2A4264" />
          <rect x="2.2" y="4.9" width="15.6" height="1.7" fill="#2A4264" />
          <circle cx="4.6" cy="4.9" r="0.7" fill="#ff5a45" />
          <circle cx="6.7" cy="4.9" r="0.7" fill="#e8b84a" />
          <circle cx="8.8" cy="4.9" r="0.7" fill="#79c98f" />
          <rect x="4.4" y="8.6" width="5.2" height="5.8" rx="0.6" fill="#fff" fillOpacity="0.2" />
          <rect x="10.4" y="8.6" width="5.2" height="2.5" rx="0.6" fill="#fff" fillOpacity="0.14" />
        </>
      );
    case "folders":
      return (
        <>
          <path fill="#D4A017" d="M2.2 5.3h5.1l1.5 1.9H17.8v1.6H2.2z" />
          <path fill="#E8B84A" d="M2.2 8.2h15.6v8.3H2.2z" />
          <path fill="#F3D36A" d="M2.2 8.2h15.6v2.1H2.2z" />
        </>
      );
    case "all":
      return (
        <>
          <path fill="#9AA5B1" d="M6.4 2.1h8.1L17.6 5.8v10.6H6.4z" />
          <path fill="#fff" fillOpacity="0.28" d="M14.5 2.1 17.6 5.8h-3.1z" />
          <path fill="#6B7C93" d="M2.4 4.6h8.2L13.8 8.4v10.2H2.4z" />
          <path fill="#fff" fillOpacity="0.3" d="M10.6 4.6 13.8 8.4h-3.2z" />
        </>
      );
    default:
      return (
        <>
          <FoldedDoc fill="#6B7C93" />
          <rect x="6" y="8.6" width="8" height="1.15" rx="0.4" fill="#fff" fillOpacity="0.85" />
          <rect x="6" y="11.3" width="6.2" height="1.15" rx="0.4" fill="#fff" fillOpacity="0.85" />
        </>
      );
  }
}

export default function FileTypeIcon({ kind, size = 18 }: { kind: FileTypeIconKind; size?: number }) {
  const officialSrc = OFFICIAL_PNG[kind];
  if (officialSrc) {
    return (
      <img
        className="esb-file-type-icon"
        data-kind={kind}
        src={officialSrc}
        width={size}
        height={size}
        alt=""
        aria-hidden="true"
      />
    );
  }

  return (
    <svg
      className="esb-file-type-icon"
      data-kind={kind}
      width={size}
      height={size}
      viewBox="0 0 20 20"
      aria-hidden="true"
      focusable="false"
    >
      {glyph(kind)}
    </svg>
  );
}
