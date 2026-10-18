import type { CSSProperties } from "react";

export type IconName =
  | "search"
  | "folder"
  | "folder-open"
  | "file"
  | "file-text"
  | "file-pdf"
  | "file-image"
  | "shield"
  | "shield-check"
  | "check"
  | "alert-triangle"
  | "alert-circle"
  | "info"
  | "calendar"
  | "filter"
  | "sparkles"
  | "chevron-down"
  | "chevron-right"
  | "x"
  | "external-link"
  | "trash"
  | "copy"
  | "refresh"
  | "download"
  | "eye"
  | "plus"
  | "layers"
  | "database"
  | "clock"
  | "hash"
  | "arrow-right";

interface IconProps {
  name: IconName;
  size?: number | string;
  className?: string;
  style?: CSSProperties;
  "aria-hidden"?: boolean | "true" | "false";
}

export function Icon({
  name,
  size = 16,
  className = "",
  style,
  "aria-hidden": ariaHidden = true,
}: IconProps) {
  const sizeValue = typeof size === "number" ? `${size}px` : size;
  const mergedStyle: CSSProperties = {
    display: "inline-block",
    verticalAlign: "middle",
    flexShrink: 0,
    width: sizeValue,
    height: sizeValue,
    ...style,
  };

  const common = {
    width: "100%",
    height: "100%",
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 2,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
  };

  switch (name) {
    case "search":
      return (
        <span className={`icon icon--search ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <circle cx="11" cy="11" r="8" />
            <line x1="21" y1="21" x2="16.65" y2="16.65" />
          </svg>
        </span>
      );

    case "folder":
      return (
        <span className={`icon icon--folder ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
          </svg>
        </span>
      );

    case "folder-open":
      return (
        <span className={`icon icon--folder-open ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
            <polygon points="2 10 22 10 20 21 4 21" fill="currentColor" fillOpacity="0.12" />
          </svg>
        </span>
      );

    case "file":
      return (
        <span className={`icon icon--file ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
            <polyline points="14 2 14 8 20 8" />
          </svg>
        </span>
      );

    case "file-text":
      return (
        <span className={`icon icon--file-text ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
            <polyline points="14 2 14 8 20 8" />
            <line x1="16" y1="13" x2="8" y2="13" />
            <line x1="16" y1="17" x2="8" y2="17" />
            <polyline points="10 9 9 9 8 9" />
          </svg>
        </span>
      );

    case "file-pdf":
      return (
        <span className={`icon icon--file-pdf ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
            <polyline points="14 2 14 8 20 8" />
            <path d="M10 12h2a1.5 1.5 0 0 1 1.5 1.5v1a1.5 1.5 0 0 1-1.5 1.5h-2v-4z" />
          </svg>
        </span>
      );

    case "file-image":
      return (
        <span className={`icon icon--file-image ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
            <circle cx="8.5" cy="8.5" r="1.5" />
            <polyline points="21 15 16 10 5 21" />
          </svg>
        </span>
      );

    case "shield":
      return (
        <span className={`icon icon--shield ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
          </svg>
        </span>
      );

    case "shield-check":
      return (
        <span className={`icon icon--shield-check ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
            <polyline points="9 12 11 14 15 10" />
          </svg>
        </span>
      );

    case "check":
      return (
        <span className={`icon icon--check ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <polyline points="20 6 9 17 4 12" />
          </svg>
        </span>
      );

    case "alert-triangle":
      return (
        <span className={`icon icon--alert-triangle ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
            <line x1="12" y1="9" x2="12" y2="13" />
            <line x1="12" y1="17" x2="12.01" y2="17" />
          </svg>
        </span>
      );

    case "alert-circle":
      return (
        <span className={`icon icon--alert-circle ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <circle cx="12" cy="12" r="10" />
            <line x1="12" y1="8" x2="12" y2="12" />
            <line x1="12" y1="16" x2="12.01" y2="16" />
          </svg>
        </span>
      );

    case "info":
      return (
        <span className={`icon icon--info ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <circle cx="12" cy="12" r="10" />
            <line x1="12" y1="16" x2="12" y2="12" />
            <line x1="12" y1="8" x2="12.01" y2="8" />
          </svg>
        </span>
      );

    case "calendar":
      return (
        <span className={`icon icon--calendar ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <rect x="3" y="4" width="18" height="18" rx="2" ry="2" />
            <line x1="16" y1="2" x2="16" y2="6" />
            <line x1="8" y1="2" x2="8" y2="6" />
            <line x1="3" y1="10" x2="21" y2="10" />
          </svg>
        </span>
      );

    case "filter":
      return (
        <span className={`icon icon--filter ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3" />
          </svg>
        </span>
      );

    case "sparkles":
      return (
        <span className={`icon icon--sparkles ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <path d="M12 2v4m0 12v4M4.93 4.93l2.83 2.83m8.48 8.48l2.83 2.83M2 12h4m12 0h4M4.93 19.07l2.83-2.83m8.48-8.48l2.83-2.83" />
          </svg>
        </span>
      );

    case "chevron-down":
      return (
        <span className={`icon icon--chevron-down ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <polyline points="6 9 12 15 18 9" />
          </svg>
        </span>
      );

    case "chevron-right":
      return (
        <span className={`icon icon--chevron-right ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <polyline points="9 18 15 12 9 6" />
          </svg>
        </span>
      );

    case "x":
      return (
        <span className={`icon icon--x ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </span>
      );

    case "external-link":
      return (
        <span className={`icon icon--external-link ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
            <polyline points="15 3 21 3 21 9" />
            <line x1="10" y1="14" x2="21" y2="3" />
          </svg>
        </span>
      );

    case "trash":
      return (
        <span className={`icon icon--trash ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <polyline points="3 6 5 6 21 6" />
            <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
          </svg>
        </span>
      );

    case "copy":
      return (
        <span className={`icon icon--copy ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
            <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
          </svg>
        </span>
      );

    case "refresh":
      return (
        <span className={`icon icon--refresh ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <polyline points="23 4 23 10 17 10" />
            <polyline points="1 20 1 14 7 14" />
            <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
          </svg>
        </span>
      );

    case "download":
      return (
        <span className={`icon icon--download ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
            <polyline points="7 10 12 15 17 10" />
            <line x1="12" y1="15" x2="12" y2="3" />
          </svg>
        </span>
      );

    case "eye":
      return (
        <span className={`icon icon--eye ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
            <circle cx="12" cy="12" r="3" />
          </svg>
        </span>
      );

    case "plus":
      return (
        <span className={`icon icon--plus ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <line x1="12" y1="5" x2="12" y2="19" />
            <line x1="5" y1="12" x2="19" y2="12" />
          </svg>
        </span>
      );

    case "layers":
      return (
        <span className={`icon icon--layers ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <polygon points="12 2 2 7 12 12 22 7 12 2" />
            <polyline points="2 17 12 22 22 17" />
            <polyline points="2 12 12 17 22 12" />
          </svg>
        </span>
      );

    case "database":
      return (
        <span className={`icon icon--database ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <ellipse cx="12" cy="5" rx="9" ry="3" />
            <path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3" />
            <path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5" />
          </svg>
        </span>
      );

    case "clock":
      return (
        <span className={`icon icon--clock ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <circle cx="12" cy="12" r="10" />
            <polyline points="12 6 12 12 16 14" />
          </svg>
        </span>
      );

    case "hash":
      return (
        <span className={`icon icon--hash ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <line x1="4" y1="9" x2="20" y2="9" />
            <line x1="4" y1="15" x2="20" y2="15" />
            <line x1="10" y1="3" x2="8" y2="21" />
            <line x1="16" y1="3" x2="14" y2="21" />
          </svg>
        </span>
      );

    case "arrow-right":
      return (
        <span className={`icon icon--arrow-right ${className}`} style={mergedStyle} aria-hidden={ariaHidden}>
          <svg {...common}>
            <line x1="5" y1="12" x2="19" y2="12" />
            <polyline points="12 5 19 12 12 19" />
          </svg>
        </span>
      );

    default:
      return null;
  }
}
