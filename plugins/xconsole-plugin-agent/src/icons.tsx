/**
 * Icons this plugin needs that the core set does not have.
 *
 * AGENTS.md: pure SVG, imported from `src/components/icons.tsx` or the plugin's local
 * `icons.tsx`. Anything the rest of the app also wants belongs in the core file; this is
 * for shapes only the agent surface uses.
 */

interface IconProps {
  size?: number;
  className?: string;
}

/**
 * The warning on an irreversible approval.
 *
 * Deliberately the one shape a person reads as "stop" without reading the words, because
 * the whole failure this card exists to prevent is a destructive command approved at a
 * glance.
 */
export function AlertIcon({ size = 16, className }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
      <line x1="12" y1="9" x2="12" y2="13" />
      <line x1="12" y1="17" x2="12.01" y2="17" />
    </svg>
  );
}
