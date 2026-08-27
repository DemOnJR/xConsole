/**
 * A distinct icon for every kind of file the browser can show.
 *
 * The listing used three emoji — folder, page, link — so a PHP file, a tarball and a
 * private key were the same picture, and scanning a directory meant reading every
 * filename.
 *
 * # Two kinds of icon, because files come in two kinds
 *
 * Things with a physical shape get a drawing of that shape: an archive is a box, an image
 * a framed picture, a key a key, a database a cylinder. Those are recognisable at a
 * glance and need no reading.
 *
 * Text formats have no shape. A PHP file and a YAML file are both "a page of text", so a
 * drawing can only ever say *page*, which is how forty languages ended up sharing one
 * glyph separated by colour alone. They get a sheet whose lower band carries their own
 * name instead — PHP, YML, TSX, SCSS — built per extension at module load. That is what
 * every file manager does with them, and it is the only mark that stays both legible and
 * unambiguous this small.
 *
 * Colour still runs through both, using each ecosystem's own where it has one: PHP indigo,
 * Go cyan, Rust orange, Ruby red. And every kind carries a hover label, so the picture
 * never has to be the whole explanation.
 *
 * Icons follow the house style in `icons.tsx`: 24×24, `currentColor` stroke, 1.8 wide,
 * round caps. Colour is applied by the caller through `className`, so a row can dim or
 * highlight the whole thing without the icon knowing.
 */
import type { ComponentType, SVGProps } from "react";
type IconProps = SVGProps<SVGSVGElement> & {
    size?: number;
};
/** A plain sheet with a folded corner — anything we have no better idea about. */
export declare function PlainFileIcon(props: IconProps): import("react").JSX.Element;
/**
 * A sheet whose lower band carries the format's own name.
 *
 * Text formats do not have distinguishable silhouettes — a PHP file and a YAML file are
 * both "a page of text" — so a shared glyph plus a colour was the best that shape alone
 * could do, and it left PHP and Go with the same picture. Their name is the mark: that is
 * how every file manager renders them, and it is the only thing that stays legible at this
 * size while remaining unambiguous.
 *
 * Built once per format at module load, so each extension gets a real component rather
 * than a shape shared by forty languages.
 */
export declare function labelled(tag: string): ComponentType<IconProps>;
/** Source code: angle brackets. */
export declare function CodeFileIcon(props: IconProps): import("react").JSX.Element;
/** Markup — HTML and template languages: a tag. */
export declare function MarkupIcon(props: IconProps): import("react").JSX.Element;
/** Stylesheets: a droplet, for the paint. */
export declare function StyleIcon(props: IconProps): import("react").JSX.Element;
/** Structured data — JSON, YAML, TOML: braces. */
export declare function DataIcon(props: IconProps): import("react").JSX.Element;
/** Settings files: sliders. */
export declare function ConfigIcon(props: IconProps): import("react").JSX.Element;
/** Archives: a box with a band across the lid. */
export declare function ArchiveIcon(props: IconProps): import("react").JSX.Element;
/** Images: the classic frame with a horizon and a sun. */
export declare function ImageFileIcon(props: IconProps): import("react").JSX.Element;
/** Audio: a note. */
export declare function AudioIcon(props: IconProps): import("react").JSX.Element;
/** Video: a film strip. */
export declare function VideoIcon(props: IconProps): import("react").JSX.Element;
/** Documents that are meant to be read: a sheet with lines of text. */
export declare function DocIcon(props: IconProps): import("react").JSX.Element;
/** Spreadsheets and delimited data: a grid. */
export declare function SheetIcon(props: IconProps): import("react").JSX.Element;
/** Slide decks: a screen on a stand. */
export declare function SlidesIcon(props: IconProps): import("react").JSX.Element;
/** Databases and dumps: the cylinder. */
export declare function DatabaseFileIcon(props: IconProps): import("react").JSX.Element;
/** Keys, certificates, anything secret. */
export declare function KeyFileIcon(props: IconProps): import("react").JSX.Element;
/** Logs: ruled lines, ragged like output. */
export declare function LogIcon(props: IconProps): import("react").JSX.Element;
/** Shell scripts: a prompt. */
export declare function ShellIcon(props: IconProps): import("react").JSX.Element;
/** Compiled things: a chip. */
export declare function BinaryIcon(props: IconProps): import("react").JSX.Element;
/** Fonts. */
export declare function FontIcon(props: IconProps): import("react").JSX.Element;
/** Disc and filesystem images. */
export declare function DiskIcon(props: IconProps): import("react").JSX.Element;
/** Symbolic links: a chain. */
export declare function SymlinkIcon(props: IconProps): import("react").JSX.Element;
export interface FileKind {
    Icon: ComponentType<IconProps>;
    /** A Tailwind text colour class — the icon inherits it through currentColor. */
    className: string;
    /** Shown on hover, so the picture never has to carry the whole meaning. */
    label: string;
}
/**
 * The icon, colour and hover label for one entry.
 *
 * Directories and symlinks are settled first: what a thing *is* outranks what it is
 * called, and a directory named `styles.css` is still a directory.
 */
export declare function fileKindFor(entry: {
    name: string;
    is_dir: boolean;
    is_symlink?: boolean;
    link_broken?: boolean;
}): FileKind;
export {};
//# sourceMappingURL=fileIcons.d.ts.map