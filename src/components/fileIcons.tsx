/**
 * A distinct icon for every kind of file the browser can show.
 *
 * The listing used three emoji — folder, page, link — so a PHP file, a tarball and a
 * private key were the same picture, and scanning a directory meant reading every
 * filename. This gives each family its own shape and each specific type its own colour.
 *
 * # Shape by family, colour by type
 *
 * Drawing a bespoke glyph per extension would mean a hundred hand-built paths, and at the
 * 14px this renders at, most of them would be indistinguishable mush. So the *shape* says
 * what kind of thing it is — archive, image, key, database — and the *colour* says which
 * one, using each ecosystem's own: PHP indigo, Go cyan, Rust orange, Ruby red. Two
 * channels at a size where one is not enough, and both survive being small.
 *
 * Icons follow the house style in `icons.tsx`: 24×24, `currentColor` stroke, 1.8 wide,
 * round caps. Colour is applied by the caller through `className`, so a row can dim or
 * highlight the whole thing without the icon knowing.
 */
import type { ComponentType, SVGProps } from "react";
import { FolderIcon } from "./icons";

type IconProps = SVGProps<SVGSVGElement> & { size?: number };

function base({ size = 16, ...props }: IconProps) {
  return {
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.8,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    ...props,
  };
}

// ---------------------------------------------------------------------------
// The shapes.
// ---------------------------------------------------------------------------

/** A plain sheet with a folded corner — anything we have no better idea about. */
export function PlainFileIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" />
      <path d="M14 3v5h5" />
    </svg>
  );
}

/** Source code: angle brackets. */
export function CodeFileIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M9 8 5 12l4 4" />
      <path d="m15 8 4 4-4 4" />
    </svg>
  );
}

/** Markup — HTML and template languages: a tag. */
export function MarkupIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M4 12 8 8v8z" />
      <path d="M11 18 14 6" />
      <path d="M20 12 16 8v8z" />
    </svg>
  );
}

/** Stylesheets: a droplet, for the paint. */
export function StyleIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M12 3s6 6.6 6 10.5a6 6 0 0 1-12 0C6 9.6 12 3 12 3z" />
    </svg>
  );
}

/** Structured data — JSON, YAML, TOML: braces. */
export function DataIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M9 4H8a2 2 0 0 0-2 2v3a2 2 0 0 1-2 2 2 2 0 0 1 2 2v3a2 2 0 0 0 2 2h1" />
      <path d="M15 4h1a2 2 0 0 1 2 2v3a2 2 0 0 0 2 2 2 2 0 0 0-2 2v3a2 2 0 0 1-2 2h-1" />
    </svg>
  );
}

/** Settings files: sliders. */
export function ConfigIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <line x1="4" y1="8" x2="20" y2="8" />
      <line x1="4" y1="16" x2="20" y2="16" />
      <circle cx="9" cy="8" r="2" />
      <circle cx="15" cy="16" r="2" />
    </svg>
  );
}

/** Archives: a box with a band across the lid. */
export function ArchiveIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <rect x="3" y="4" width="18" height="5" rx="1" />
      <path d="M5 9v9a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V9" />
      <line x1="10" y1="13" x2="14" y2="13" />
    </svg>
  );
}

/** Images: the classic frame with a horizon and a sun. */
export function ImageFileIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <circle cx="8.5" cy="9.5" r="1.5" />
      <path d="m21 16-5-5-5 5-3-3-5 5" />
    </svg>
  );
}

/** Audio: a note. */
export function AudioIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M9 18V6l10-2v12" />
      <circle cx="6.5" cy="18" r="2.5" />
      <circle cx="16.5" cy="16" r="2.5" />
    </svg>
  );
}

/** Video: a film strip. */
export function VideoIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <rect x="3" y="5" width="18" height="14" rx="2" />
      <line x1="7" y1="5" x2="7" y2="19" />
      <line x1="17" y1="5" x2="17" y2="19" />
      <line x1="3" y1="12" x2="21" y2="12" />
    </svg>
  );
}

/** Documents that are meant to be read: a sheet with lines of text. */
export function DocIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" />
      <path d="M14 3v5h5" />
      <line x1="8.5" y1="13" x2="15.5" y2="13" />
      <line x1="8.5" y1="16.5" x2="13.5" y2="16.5" />
    </svg>
  );
}

/** Spreadsheets and delimited data: a grid. */
export function SheetIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <line x1="3" y1="10" x2="21" y2="10" />
      <line x1="3" y1="15" x2="21" y2="15" />
      <line x1="10" y1="10" x2="10" y2="20" />
    </svg>
  );
}

/** Slide decks: a screen on a stand. */
export function SlidesIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <rect x="3" y="4" width="18" height="11" rx="1.5" />
      <path d="M12 15v3" />
      <path d="M8.5 21 12 18l3.5 3" />
    </svg>
  );
}

/** Databases and dumps: the cylinder. */
export function DatabaseFileIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <ellipse cx="12" cy="6" rx="8" ry="3" />
      <path d="M4 6v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6" />
      <path d="M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3" />
    </svg>
  );
}

/** Keys, certificates, anything secret. */
export function KeyFileIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <circle cx="7.5" cy="15.5" r="3.5" />
      <path d="m10 13 8-8" />
      <path d="m15 8 2 2" />
      <path d="m18 5 2 2" />
    </svg>
  );
}

/** Logs: ruled lines, ragged like output. */
export function LogIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <line x1="4" y1="7" x2="20" y2="7" />
      <line x1="4" y1="11" x2="16" y2="11" />
      <line x1="4" y1="15" x2="19" y2="15" />
      <line x1="4" y1="19" x2="12" y2="19" />
    </svg>
  );
}

/** Shell scripts: a prompt. */
export function ShellIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="m7 9 3 3-3 3" />
      <line x1="12.5" y1="15" x2="17" y2="15" />
    </svg>
  );
}

/** Compiled things: a chip. */
export function BinaryIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <rect x="7" y="7" width="10" height="10" rx="1.5" />
      <path d="M10 3v4M14 3v4M10 17v4M14 17v4M3 10h4M3 14h4M17 10h4M17 14h4" />
    </svg>
  );
}

/** Fonts. */
export function FontIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="m5 19 6-14 6 14" />
      <line x1="7.5" y1="14" x2="14.5" y2="14" />
    </svg>
  );
}

/** Disc and filesystem images. */
export function DiskIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <circle cx="12" cy="12" r="9" />
      <circle cx="12" cy="12" r="2.5" />
    </svg>
  );
}

/** Symbolic links: a chain. */
export function SymlinkIcon(props: IconProps) {
  return (
    <svg {...base(props)}>
      <path d="M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7" />
      <path d="M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7" />
    </svg>
  );
}

// ---------------------------------------------------------------------------
// What each extension is.
// ---------------------------------------------------------------------------

export interface FileKind {
  Icon: ComponentType<IconProps>;
  /** A Tailwind text colour class — the icon inherits it through currentColor. */
  className: string;
  /** Shown on hover, so the picture never has to carry the whole meaning. */
  label: string;
}

const code = (className: string, label: string): FileKind => ({
  Icon: CodeFileIcon,
  className,
  label,
});

/**
 * Extension → kind. Colours are each ecosystem's own where it has one, which is what
 * makes them recognisable without a legend.
 */
const BY_EXTENSION: Record<string, FileKind> = {
  // --- languages ---------------------------------------------------------
  php: code("text-indigo-400", "PHP source"),
  js: code("text-yellow-400", "JavaScript"),
  mjs: code("text-yellow-400", "JavaScript module"),
  cjs: code("text-yellow-400", "CommonJS module"),
  jsx: code("text-cyan-400", "React component"),
  ts: code("text-blue-400", "TypeScript"),
  tsx: code("text-blue-400", "React component (TS)"),
  py: code("text-amber-300", "Python"),
  rb: code("text-red-400", "Ruby"),
  go: code("text-cyan-300", "Go"),
  rs: code("text-orange-400", "Rust"),
  java: code("text-orange-500", "Java"),
  kt: code("text-purple-400", "Kotlin"),
  c: code("text-blue-300", "C"),
  h: code("text-blue-300", "C header"),
  cpp: code("text-blue-400", "C++"),
  cc: code("text-blue-400", "C++"),
  hpp: code("text-blue-300", "C++ header"),
  cs: code("text-green-400", "C#"),
  swift: code("text-orange-400", "Swift"),
  lua: code("text-blue-400", "Lua"),
  pl: code("text-indigo-300", "Perl"),
  pm: code("text-indigo-300", "Perl module"),
  r: code("text-blue-300", "R"),
  scala: code("text-red-400", "Scala"),
  dart: code("text-teal-400", "Dart"),
  ex: code("text-purple-300", "Elixir"),
  exs: code("text-purple-300", "Elixir script"),
  erl: code("text-red-300", "Erlang"),
  hs: code("text-purple-400", "Haskell"),
  clj: code("text-green-300", "Clojure"),
  vim: code("text-green-400", "Vim script"),
  sql: { Icon: DatabaseFileIcon, className: "text-sky-300", label: "SQL script" },

  // --- shells ------------------------------------------------------------
  sh: { Icon: ShellIcon, className: "text-green-400", label: "Shell script" },
  bash: { Icon: ShellIcon, className: "text-green-400", label: "Bash script" },
  zsh: { Icon: ShellIcon, className: "text-green-400", label: "Zsh script" },
  fish: { Icon: ShellIcon, className: "text-green-300", label: "Fish script" },
  ps1: { Icon: ShellIcon, className: "text-blue-400", label: "PowerShell script" },
  bat: { Icon: ShellIcon, className: "text-gray-300", label: "Batch file" },
  cmd: { Icon: ShellIcon, className: "text-gray-300", label: "Batch file" },

  // --- web ---------------------------------------------------------------
  html: { Icon: MarkupIcon, className: "text-orange-400", label: "HTML" },
  htm: { Icon: MarkupIcon, className: "text-orange-400", label: "HTML" },
  xml: { Icon: MarkupIcon, className: "text-orange-300", label: "XML" },
  vue: { Icon: MarkupIcon, className: "text-emerald-400", label: "Vue component" },
  svelte: { Icon: MarkupIcon, className: "text-orange-500", label: "Svelte component" },
  astro: { Icon: MarkupIcon, className: "text-purple-400", label: "Astro component" },
  twig: { Icon: MarkupIcon, className: "text-lime-400", label: "Twig template" },
  blade: { Icon: MarkupIcon, className: "text-red-400", label: "Blade template" },
  ejs: { Icon: MarkupIcon, className: "text-yellow-300", label: "EJS template" },
  hbs: { Icon: MarkupIcon, className: "text-amber-400", label: "Handlebars template" },
  css: { Icon: StyleIcon, className: "text-sky-400", label: "Stylesheet" },
  scss: { Icon: StyleIcon, className: "text-pink-400", label: "Sass stylesheet" },
  sass: { Icon: StyleIcon, className: "text-pink-400", label: "Sass stylesheet" },
  less: { Icon: StyleIcon, className: "text-blue-400", label: "Less stylesheet" },

  // --- data and config ---------------------------------------------------
  json: { Icon: DataIcon, className: "text-yellow-300", label: "JSON" },
  jsonc: { Icon: DataIcon, className: "text-yellow-300", label: "JSON with comments" },
  yaml: { Icon: DataIcon, className: "text-rose-300", label: "YAML" },
  yml: { Icon: DataIcon, className: "text-rose-300", label: "YAML" },
  toml: { Icon: DataIcon, className: "text-orange-300", label: "TOML" },
  ini: { Icon: ConfigIcon, className: "text-gray-300", label: "INI configuration" },
  conf: { Icon: ConfigIcon, className: "text-gray-300", label: "Configuration" },
  cfg: { Icon: ConfigIcon, className: "text-gray-300", label: "Configuration" },
  env: { Icon: ConfigIcon, className: "text-amber-400", label: "Environment file" },
  properties: { Icon: ConfigIcon, className: "text-gray-300", label: "Properties" },
  lock: { Icon: ConfigIcon, className: "text-gray-500", label: "Lockfile" },

  // --- documents ---------------------------------------------------------
  md: { Icon: DocIcon, className: "text-sky-300", label: "Markdown" },
  markdown: { Icon: DocIcon, className: "text-sky-300", label: "Markdown" },
  rst: { Icon: DocIcon, className: "text-sky-300", label: "reStructuredText" },
  txt: { Icon: DocIcon, className: "text-gray-400", label: "Text" },
  pdf: { Icon: DocIcon, className: "text-red-400", label: "PDF" },
  doc: { Icon: DocIcon, className: "text-blue-400", label: "Word document" },
  docx: { Icon: DocIcon, className: "text-blue-400", label: "Word document" },
  odt: { Icon: DocIcon, className: "text-blue-300", label: "OpenDocument text" },
  rtf: { Icon: DocIcon, className: "text-blue-300", label: "Rich text" },
  tex: { Icon: DocIcon, className: "text-emerald-300", label: "LaTeX" },
  csv: { Icon: SheetIcon, className: "text-green-400", label: "CSV" },
  tsv: { Icon: SheetIcon, className: "text-green-400", label: "TSV" },
  xls: { Icon: SheetIcon, className: "text-green-500", label: "Excel spreadsheet" },
  xlsx: { Icon: SheetIcon, className: "text-green-500", label: "Excel spreadsheet" },
  ods: { Icon: SheetIcon, className: "text-green-300", label: "OpenDocument sheet" },
  ppt: { Icon: SlidesIcon, className: "text-orange-400", label: "PowerPoint" },
  pptx: { Icon: SlidesIcon, className: "text-orange-400", label: "PowerPoint" },
  odp: { Icon: SlidesIcon, className: "text-orange-300", label: "OpenDocument slides" },

  // --- archives ----------------------------------------------------------
  zip: { Icon: ArchiveIcon, className: "text-amber-400", label: "Zip archive" },
  tar: { Icon: ArchiveIcon, className: "text-amber-400", label: "Tar archive" },
  gz: { Icon: ArchiveIcon, className: "text-amber-400", label: "Gzip archive" },
  tgz: { Icon: ArchiveIcon, className: "text-amber-400", label: "Gzipped tarball" },
  bz2: { Icon: ArchiveIcon, className: "text-amber-400", label: "Bzip2 archive" },
  xz: { Icon: ArchiveIcon, className: "text-amber-400", label: "Xz archive" },
  zst: { Icon: ArchiveIcon, className: "text-amber-400", label: "Zstandard archive" },
  lz4: { Icon: ArchiveIcon, className: "text-amber-400", label: "LZ4 archive" },
  "7z": { Icon: ArchiveIcon, className: "text-amber-500", label: "7-Zip archive" },
  rar: { Icon: ArchiveIcon, className: "text-purple-400", label: "RAR archive" },
  jar: { Icon: ArchiveIcon, className: "text-orange-500", label: "Java archive" },
  war: { Icon: ArchiveIcon, className: "text-orange-500", label: "Web archive" },
  deb: { Icon: ArchiveIcon, className: "text-rose-400", label: "Debian package" },
  rpm: { Icon: ArchiveIcon, className: "text-red-400", label: "RPM package" },
  apk: { Icon: ArchiveIcon, className: "text-green-400", label: "Android package" },
  pkg: { Icon: ArchiveIcon, className: "text-gray-300", label: "Package" },
  dmg: { Icon: DiskIcon, className: "text-gray-300", label: "Disk image" },
  iso: { Icon: DiskIcon, className: "text-gray-300", label: "Disc image" },
  img: { Icon: DiskIcon, className: "text-gray-300", label: "Disk image" },

  // --- media -------------------------------------------------------------
  png: { Icon: ImageFileIcon, className: "text-violet-300", label: "PNG image" },
  jpg: { Icon: ImageFileIcon, className: "text-violet-300", label: "JPEG image" },
  jpeg: { Icon: ImageFileIcon, className: "text-violet-300", label: "JPEG image" },
  gif: { Icon: ImageFileIcon, className: "text-violet-300", label: "GIF image" },
  bmp: { Icon: ImageFileIcon, className: "text-violet-300", label: "Bitmap image" },
  webp: { Icon: ImageFileIcon, className: "text-violet-300", label: "WebP image" },
  avif: { Icon: ImageFileIcon, className: "text-violet-300", label: "AVIF image" },
  heic: { Icon: ImageFileIcon, className: "text-violet-300", label: "HEIC image" },
  tiff: { Icon: ImageFileIcon, className: "text-violet-300", label: "TIFF image" },
  ico: { Icon: ImageFileIcon, className: "text-violet-200", label: "Icon" },
  svg: { Icon: ImageFileIcon, className: "text-amber-300", label: "SVG image" },
  psd: { Icon: ImageFileIcon, className: "text-blue-400", label: "Photoshop document" },
  xcf: { Icon: ImageFileIcon, className: "text-orange-300", label: "GIMP image" },
  mp3: { Icon: AudioIcon, className: "text-pink-300", label: "MP3 audio" },
  wav: { Icon: AudioIcon, className: "text-pink-300", label: "WAV audio" },
  flac: { Icon: AudioIcon, className: "text-pink-300", label: "FLAC audio" },
  ogg: { Icon: AudioIcon, className: "text-pink-300", label: "Ogg audio" },
  opus: { Icon: AudioIcon, className: "text-pink-300", label: "Opus audio" },
  m4a: { Icon: AudioIcon, className: "text-pink-300", label: "M4A audio" },
  aac: { Icon: AudioIcon, className: "text-pink-300", label: "AAC audio" },
  mid: { Icon: AudioIcon, className: "text-pink-200", label: "MIDI" },
  mp4: { Icon: VideoIcon, className: "text-fuchsia-300", label: "MP4 video" },
  mkv: { Icon: VideoIcon, className: "text-fuchsia-300", label: "Matroska video" },
  avi: { Icon: VideoIcon, className: "text-fuchsia-300", label: "AVI video" },
  mov: { Icon: VideoIcon, className: "text-fuchsia-300", label: "QuickTime video" },
  webm: { Icon: VideoIcon, className: "text-fuchsia-300", label: "WebM video" },
  flv: { Icon: VideoIcon, className: "text-fuchsia-300", label: "Flash video" },
  wmv: { Icon: VideoIcon, className: "text-fuchsia-300", label: "WMV video" },
  m4v: { Icon: VideoIcon, className: "text-fuchsia-300", label: "M4V video" },

  // --- databases ---------------------------------------------------------
  db: { Icon: DatabaseFileIcon, className: "text-sky-400", label: "Database" },
  sqlite: { Icon: DatabaseFileIcon, className: "text-sky-400", label: "SQLite database" },
  sqlite3: { Icon: DatabaseFileIcon, className: "text-sky-400", label: "SQLite database" },
  mdb: { Icon: DatabaseFileIcon, className: "text-sky-400", label: "Access database" },
  dump: { Icon: DatabaseFileIcon, className: "text-sky-300", label: "Database dump" },
  bak: { Icon: DatabaseFileIcon, className: "text-gray-400", label: "Backup" },

  // --- secrets -----------------------------------------------------------
  pem: { Icon: KeyFileIcon, className: "text-yellow-400", label: "PEM key or certificate" },
  key: { Icon: KeyFileIcon, className: "text-yellow-400", label: "Private key" },
  ppk: { Icon: KeyFileIcon, className: "text-yellow-400", label: "PuTTY private key" },
  crt: { Icon: KeyFileIcon, className: "text-emerald-400", label: "Certificate" },
  cer: { Icon: KeyFileIcon, className: "text-emerald-400", label: "Certificate" },
  pfx: { Icon: KeyFileIcon, className: "text-emerald-400", label: "PKCS#12 bundle" },
  p12: { Icon: KeyFileIcon, className: "text-emerald-400", label: "PKCS#12 bundle" },
  pub: { Icon: KeyFileIcon, className: "text-yellow-200", label: "Public key" },
  gpg: { Icon: KeyFileIcon, className: "text-yellow-400", label: "GPG data" },
  asc: { Icon: KeyFileIcon, className: "text-yellow-400", label: "PGP armoured data" },
  kdbx: { Icon: KeyFileIcon, className: "text-yellow-400", label: "KeePass database" },

  // --- fonts and binaries ------------------------------------------------
  ttf: { Icon: FontIcon, className: "text-rose-300", label: "TrueType font" },
  otf: { Icon: FontIcon, className: "text-rose-300", label: "OpenType font" },
  woff: { Icon: FontIcon, className: "text-rose-300", label: "Web font" },
  woff2: { Icon: FontIcon, className: "text-rose-300", label: "Web font" },
  eot: { Icon: FontIcon, className: "text-rose-300", label: "Embedded font" },
  exe: { Icon: BinaryIcon, className: "text-emerald-400", label: "Executable" },
  msi: { Icon: BinaryIcon, className: "text-emerald-400", label: "Windows installer" },
  dll: { Icon: BinaryIcon, className: "text-teal-300", label: "Dynamic library" },
  so: { Icon: BinaryIcon, className: "text-teal-300", label: "Shared object" },
  dylib: { Icon: BinaryIcon, className: "text-teal-300", label: "Dynamic library" },
  a: { Icon: BinaryIcon, className: "text-teal-300", label: "Static library" },
  o: { Icon: BinaryIcon, className: "text-gray-400", label: "Object file" },
  class: { Icon: BinaryIcon, className: "text-orange-400", label: "Java class" },
  wasm: { Icon: BinaryIcon, className: "text-purple-400", label: "WebAssembly" },
  bin: { Icon: BinaryIcon, className: "text-gray-400", label: "Binary" },

  // --- output ------------------------------------------------------------
  log: { Icon: LogIcon, className: "text-gray-400", label: "Log" },
  out: { Icon: LogIcon, className: "text-gray-400", label: "Output" },
  err: { Icon: LogIcon, className: "text-red-300", label: "Error output" },
  pid: { Icon: LogIcon, className: "text-gray-500", label: "PID file" },
};

/**
 * Files known by their whole name rather than an extension.
 *
 * A server is full of these — `Dockerfile`, `Makefile`, `.env`, `authorized_keys` — and
 * matching on extension alone leaves every one of them as a blank page, which is exactly
 * the set of files someone browsing a server is most likely looking for.
 */
const BY_NAME: Record<string, FileKind> = {
  dockerfile: { Icon: ConfigIcon, className: "text-sky-400", label: "Dockerfile" },
  containerfile: { Icon: ConfigIcon, className: "text-sky-400", label: "Containerfile" },
  makefile: { Icon: ConfigIcon, className: "text-amber-400", label: "Makefile" },
  cmakelists: { Icon: ConfigIcon, className: "text-amber-300", label: "CMake build" },
  vagrantfile: { Icon: ConfigIcon, className: "text-sky-300", label: "Vagrantfile" },
  procfile: { Icon: ConfigIcon, className: "text-purple-300", label: "Procfile" },
  readme: { Icon: DocIcon, className: "text-sky-300", label: "Readme" },
  license: { Icon: DocIcon, className: "text-amber-300", label: "Licence" },
  licence: { Icon: DocIcon, className: "text-amber-300", label: "Licence" },
  changelog: { Icon: DocIcon, className: "text-sky-300", label: "Changelog" },
  ".env": { Icon: ConfigIcon, className: "text-amber-400", label: "Environment file" },
  ".gitignore": { Icon: ConfigIcon, className: "text-orange-300", label: "Git ignore rules" },
  ".gitattributes": { Icon: ConfigIcon, className: "text-orange-300", label: "Git attributes" },
  ".dockerignore": { Icon: ConfigIcon, className: "text-sky-300", label: "Docker ignore rules" },
  ".bashrc": { Icon: ShellIcon, className: "text-green-400", label: "Bash configuration" },
  ".bash_profile": { Icon: ShellIcon, className: "text-green-400", label: "Bash profile" },
  ".zshrc": { Icon: ShellIcon, className: "text-green-400", label: "Zsh configuration" },
  ".profile": { Icon: ShellIcon, className: "text-green-300", label: "Shell profile" },
  ".vimrc": { Icon: ConfigIcon, className: "text-green-400", label: "Vim configuration" },
  authorized_keys: { Icon: KeyFileIcon, className: "text-yellow-400", label: "Authorised SSH keys" },
  known_hosts: { Icon: KeyFileIcon, className: "text-yellow-200", label: "Known SSH hosts" },
  id_rsa: { Icon: KeyFileIcon, className: "text-yellow-400", label: "Private key" },
  id_ed25519: { Icon: KeyFileIcon, className: "text-yellow-400", label: "Private key" },
  passwd: { Icon: ConfigIcon, className: "text-rose-300", label: "Account database" },
  shadow: { Icon: KeyFileIcon, className: "text-rose-400", label: "Password hashes" },
  hosts: { Icon: ConfigIcon, className: "text-gray-300", label: "Hosts file" },
  "package.json": { Icon: DataIcon, className: "text-red-400", label: "npm manifest" },
  "composer.json": { Icon: DataIcon, className: "text-indigo-400", label: "Composer manifest" },
  "cargo.toml": { Icon: DataIcon, className: "text-orange-400", label: "Cargo manifest" },
  "go.mod": { Icon: DataIcon, className: "text-cyan-300", label: "Go module" },
};

const FOLDER: FileKind = {
  Icon: FolderIcon,
  className: "text-cyan-400",
  label: "Directory",
};
const PLAIN: FileKind = {
  Icon: PlainFileIcon,
  className: "text-gray-500",
  label: "File",
};

/** Everything after the last dot, lowercased. Empty for a dotfile or a bare name. */
function extensionOf(name: string): string {
  const dot = name.lastIndexOf(".");
  // `dot <= 0` covers both "no dot" and a leading dot, which makes `.env` a *name*, not
  // an extension — otherwise every dotfile would be classified by its own name as a suffix.
  if (dot <= 0 || dot === name.length - 1) return "";
  return name.slice(dot + 1).toLowerCase();
}

/**
 * The icon, colour and hover label for one entry.
 *
 * Directories and symlinks are settled first: what a thing *is* outranks what it is
 * called, and a directory named `styles.css` is still a directory.
 */
export function fileKindFor(entry: {
  name: string;
  is_dir: boolean;
  is_symlink?: boolean;
  link_broken?: boolean;
}): FileKind {
  if (entry.link_broken) {
    return { Icon: SymlinkIcon, className: "text-red-400", label: "Broken symlink" };
  }
  if (entry.is_symlink) {
    return {
      Icon: SymlinkIcon,
      className: "text-violet-400",
      label: entry.is_dir ? "Symlink to a directory" : "Symlink",
    };
  }
  if (entry.is_dir) return FOLDER;

  const lower = entry.name.toLowerCase();
  const byName = BY_NAME[lower];
  if (byName) return byName;
  // `README.md` and `Dockerfile.prod` should still read as readme and Dockerfile, so try
  // the stem too — but only after the exact name, so `license.js` stays JavaScript.
  const stem = lower.split(".")[0];
  const ext = extensionOf(entry.name);
  const byExt = BY_EXTENSION[ext];
  if (byExt) return byExt;
  const byStem = BY_NAME[stem];
  if (byStem) return byStem;

  return PLAIN;
}
