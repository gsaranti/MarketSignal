import type { CSSProperties, ReactNode } from "react";

export interface KeyFigureItem {
  /** Tracked-caps label shown above the value. */
  label: ReactNode;
  /** The value — mono tabular by default. */
  value: ReactNode;
  /** Render the value in Public Sans instead of mono (for words, not figures). */
  sans?: boolean;
}

export interface KeyFigureStripProps {
  items?: KeyFigureItem[];
  style?: CSSProperties;
}

/** A flat, hairline-delimited row of label-over-value pairs. */
export function KeyFigureStrip(props: KeyFigureStripProps): JSX.Element;
