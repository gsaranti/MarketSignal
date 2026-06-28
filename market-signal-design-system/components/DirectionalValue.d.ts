import type { CSSProperties, ReactNode } from "react";

export interface DirectionalValueProps {
  /** Direction — drives hue and chevron. Default "flat". */
  dir?: "up" | "down" | "flat";
  /** The figure to display (e.g. "12.4%", "+1,840"). */
  children?: ReactNode;
  /** Font size in px. Default 13. */
  size?: number;
  style?: CSSProperties;
}

/** Up/down/flat directional value token — sign + weight + chevron + desaturated hue. */
export function DirectionalValue(props: DirectionalValueProps): JSX.Element;
